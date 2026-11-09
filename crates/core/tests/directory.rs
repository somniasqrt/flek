//! users.list + users.conversations → database, against a mock server.

mod support;
use support::field;

use flek_core::api::{AuthTest, Client};
use flek_core::store::{directory, Store};
use flek_core::sync::directory::sync_directory;
use flek_core::Secret;
use serde_json::json;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn me() -> AuthTest {
    AuthTest {
        url: "https://fake.slack.com/".into(),
        team: "Fake".into(),
        user: "me".into(),
        team_id: "T0FAKE".into(),
        user_id: "U0ME".into(),
        enterprise_id: None,
    }
}

async fn mock(server: &MockServer) {
    // users.list in two pages
    Mock::given(path("/users.list"))
        .and(field("cursor", "page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "members": [
                {"id": "U0BOB", "team_id": "T0FAKE", "name": "bob", "real_name": "Bob Builder",
                 "tz": "Europe/Kyiv", "tz_label": "Eastern European Time", "tz_offset": 7200,
                 "profile": {"display_name": "", "real_name": "Bob Builder", "title": " Доцент ",
                    "email": "bob@example.com", "phone": "", "image_72": "https://cdn.example/b72.png",
                    "image_512": "https://cdn.example/b512.png"}},
                {"id": "U0BOT", "team_id": "T0FAKE", "name": "helper", "is_bot": true, "profile": {}},
                {"id": "U0EXT", "team_id": "T0OTHER", "name": "ext", "profile": {"display_name": "External"}}
            ],
            "response_metadata": {"next_cursor": ""}
        })))
        .with_priority(1)
        .mount(server)
        .await;
    Mock::given(path("/users.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "members": [
                {"id": "U0ME", "team_id": "T0FAKE", "name": "me", "profile": {"display_name": "Me"}},
                {"id": "U0ALICE", "team_id": "T0FAKE", "name": "alice", "deleted": true, "profile": {"display_name": "Alice"}}
            ],
            "response_metadata": {"next_cursor": "page2"}
        })))
        .with_priority(2)
        .mount(server)
        .await;
    Mock::given(path("/users.conversations"))
        .and(field("types", "public_channel,private_channel,mpim,im"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channels": [
                {"id": "C0GEN", "name": "general", "is_channel": true, "topic": {"value": "Всі тут"}, "purpose": {"value": ""}, "created": 1600000000},
                {"id": "G0SEC", "name": "secret", "is_private": true, "is_archived": true},
                {"id": "D0BOB", "is_im": true, "user": "U0BOB"},
                {"id": "D0GONE", "is_im": true, "user": "U0UNKNOWN"},
                {"id": "D0SYS", "is_im": true, "user": "USLACK"},
                {"id": "G0MP", "name": "mpdm-me--alice--bob-1", "is_mpim": true, "is_private": true}
            ],
            "response_metadata": {"next_cursor": ""}
        })))
        .mount(server)
        .await;
}

async fn mock_counts(server: &MockServer) {
    Mock::given(path("/client.counts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channels": [{"id": "C0GEN", "last_read": "100.000000", "has_unreads": true}],
            "ims": [{"id": "D0BOB", "last_read": "0000000000.000000"}],
            "mpims": []
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn unread_counts_from_read_markers() {
    use flek_core::store::messages::{self, MessageRow};

    let server = MockServer::start().await;
    mock(&server).await;
    mock_counts(&server).await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    let store = Store::open_in_memory().unwrap();
    sync_directory(&client, &store, &me()).await.unwrap();

    let row = |ts: &str, user: &str| MessageRow {
        ts: ts.into(),
        user_id: Some(user.into()),
        text: "x".into(),
        raw_json: "{}".into(),
        ..Default::default()
    };
    store
        .with_conn(|c| {
            // Read up to 100: 50 is read, 150 and 200 are new, 300 is mine.
            messages::upsert_messages(
                c,
                "C0GEN",
                &[
                    row("050.000000", "U0BOB"),
                    row("150.000000", "U0BOB"),
                    row("200.000000", "U0BOT"),
                    row("300.000000", "U0ME"),
                ],
            )?;
            let rows = directory::list_conversations(c, "T0FAKE")?;
            let unread = |id: &str| rows.iter().find(|r| r.id == id).unwrap().unread;
            assert_eq!(unread("C0GEN"), Some(2));
            assert_eq!(unread("D0BOB"), Some(0));
            assert_eq!(unread("G0SEC"), None);
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn syncs_users_and_conversations() {
    let server = MockServer::start().await;
    mock(&server).await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    let store = Store::open_in_memory().unwrap();

    let stats = sync_directory(&client, &store, &me()).await.unwrap();
    assert_eq!(stats.users, 5);
    assert_eq!(stats.conversations, 6);

    store
        .with_conn(|c| {
            assert_eq!(
                directory::user_label(c, "U0BOB")?.as_deref(),
                Some("Bob Builder")
            );
            assert_eq!(
                directory::user_label(c, "U0BOT")?.as_deref(),
                Some("helper")
            );
            let deleted: bool =
                c.query_row("SELECT deleted FROM users WHERE id = 'U0ALICE'", [], |r| {
                    r.get(0)
                })?;
            assert!(deleted);
            let ext_team: String =
                c.query_row("SELECT team_id FROM users WHERE id = 'U0EXT'", [], |r| {
                    r.get(0)
                })?;
            assert_eq!(ext_team, "T0OTHER");

            let rows = directory::list_conversations(c, "T0FAKE")?;
            let labels: Vec<(&str, &str)> = rows
                .iter()
                .map(|r| (r.id.as_str(), r.label.as_str()))
                .collect();
            assert_eq!(
                labels,
                [
                    ("C0GEN", "general"),
                    ("G0SEC", "secret"),
                    ("G0MP", "Alice, Bob Builder"),
                    ("D0BOB", "Bob Builder"),
                    ("D0SYS", "Slack"),
                    ("D0GONE", "U0UNKNOWN"),
                ]
            );
            let gen = &rows[0];
            assert_eq!(gen.kind, "public_channel");
            assert_eq!(gen.topic.as_deref(), Some("Всі тут"));
            assert!(rows[1].is_archived);
            assert_eq!(rows[1].kind, "private_channel");
            Ok(())
        })
        .unwrap();

    // Second run updates in place.
    sync_directory(&client, &store, &me()).await.unwrap();
    let n: i64 = store
        .with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 6);
}

#[tokio::test]
async fn profiles_come_from_the_stored_directory() {
    let server = MockServer::start().await;
    mock(&server).await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    let store = Store::open_in_memory().unwrap();
    sync_directory(&client, &store, &me()).await.unwrap();

    let bob = store
        .with_read(|c| directory::user_profile(c, "U0BOB"))
        .unwrap()
        .unwrap();
    assert_eq!(bob.name, "bob");
    assert_eq!(bob.real_name.as_deref(), Some("Bob Builder"));
    assert_eq!(bob.display_name, None, "empty display name is no name");
    assert_eq!(bob.title.as_deref(), Some("Доцент"));
    assert_eq!(bob.email.as_deref(), Some("bob@example.com"));
    assert_eq!(bob.phone, None);
    assert_eq!(bob.tz.as_deref(), Some("Europe/Kyiv"));
    assert_eq!(bob.tz_label.as_deref(), Some("Eastern European Time"));
    assert_eq!(bob.tz_offset, Some(7200));
    assert_eq!(bob.dm_conversation_id.as_deref(), Some("D0BOB"));
    assert!(bob.has_photo);
    assert_eq!(bob.photo, None, "not fetched yet");
    assert_eq!(bob.avatar, None, "not fetched yet");
    assert!(!bob.is_bot && !bob.deleted);
    // Personal data stays out of logs.
    let debug = format!("{bob:?}");
    assert!(!debug.contains("bob@example.com"), "{debug}");

    let alice = store
        .with_read(|c| directory::user_profile(c, "U0ALICE"))
        .unwrap()
        .unwrap();
    assert!(alice.deleted);
    assert_eq!(alice.display_name.as_deref(), Some("Alice"));
    assert_eq!(alice.dm_conversation_id, None);
    assert!(!alice.has_photo);
    assert!(store
        .with_read(|c| directory::user_profile(c, "U0NOBODY"))
        .unwrap()
        .is_none());
}
