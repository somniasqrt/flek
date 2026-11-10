//! Channel directory, join and create against fake Slack and temporary SQLite only.
mod support;
use flek_core::{
    api::Client,
    store::{channel_browser as stored, directory, Store},
    sync::{
        channel_browser::{create, join, normalize_name, refresh, validate_name},
        live::{bus, Change},
    },
    Error, Secret,
};
use serde_json::json;
use support::field;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn client(server: &MockServer) -> Client {
    Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing()
}

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.with_conn(|c| {
        c.execute_batch(
            "INSERT INTO teams(id,name,self_user_id) VALUES ('T1','Fake','U1');
             INSERT INTO conversations(id,team_id,kind,name,label,is_member,raw_json)
             VALUES ('C1','T1','public_channel','general','general',1,'{}');",
        )?;
        Ok(())
    })
    .unwrap();
    s
}

fn channel(id: &str, name: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut v = json!({
        "id": id, "name": name, "is_channel": true, "is_private": false,
        "is_archived": false, "created": 1_700_000_000, "creator": "U9",
        "num_members": 3, "topic": {"value": ""}, "purpose": {"value": ""}
    });
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

#[tokio::test]
async fn directory_pages_store_non_members_outside_the_sidebar() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.list"))
        .and(field("cursor", ""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channels": [channel("C1", "general", json!({"is_member": true}))],
            "response_metadata": {"next_cursor": "page2"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.list"))
        .and(field("cursor", "page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channels": [
                channel("C2", "random", json!({"is_member": false, "num_members": 40})),
                channel("C3", "old", json!({"is_member": false, "is_archived": true})),
            ],
            "response_metadata": {"next_cursor": ""}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    refresh(&client(&server), &s, "T1", false).await.unwrap();
    // Fresh cache: a second open within the hour makes no request (expect(1) above).
    refresh(&client(&server), &s, "T1", false).await.unwrap();
    s.with_read(|c| {
        let dir = stored::read(c, "T1")?;
        assert!(dir.fetched_at.is_some() && !dir.truncated);
        let mut ids: Vec<_> = dir.channels.iter().map(|r| r.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["C1", "C2", "C3"]);
        let random = dir.channels.iter().find(|r| r.id == "C2").unwrap();
        assert!(!random.is_member);
        assert_eq!(random.num_members, Some(40));
        assert!(
            dir.channels
                .iter()
                .find(|r| r.id == "C3")
                .unwrap()
                .is_archived
        );
        let sidebar: Vec<_> = directory::list_conversations(c, "T1")?
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(sidebar, ["C1"]);
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn join_makes_the_channel_a_member_and_notifies() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.join"))
        .and(field("channel", "C2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "channel": channel("C2", "random", json!({"is_member": true}))
        })))
        .expect(1)
        .mount(&server)
        .await;
    // The catch-up after joining may fail; joining still counts.
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "messages": [], "has_more": false
        })))
        .mount(&server)
        .await;
    let s = store();
    s.with_conn(|c| {
        c.execute(
            "INSERT INTO conversations(id,team_id,kind,name,label,is_member,raw_json)
             VALUES ('C2','T1','public_channel','random','random',0,'{}')",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let bus = bus();
    let mut rx = bus.subscribe();
    let action = join(&client(&server), &s, &bus, "T1", "C2").await.unwrap();
    assert_eq!(action.id, "C2");
    assert_eq!(rx.recv().await.unwrap(), Change::Directory);
    s.with_read(|c| {
        let ids: Vec<_> = directory::list_conversations(c, "T1")?
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert!(ids.contains(&"C2".to_owned()));
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn create_normalises_the_name_and_sets_the_purpose() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.create"))
        .and(field("name", "новий-канал"))
        .and(field("is_private", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channel": channel("C7", "новий-канал", json!({"is_private": true, "is_group": true}))
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.setPurpose"))
        .and(field("channel", "C7"))
        .and(field("purpose", "Для тестів"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "purpose": "Для тестів"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    let bus = bus();
    let action = create(
        &client(&server),
        &s,
        &bus,
        "T1",
        "Новий Канал",
        true,
        " Для тестів ",
    )
    .await
    .unwrap();
    assert_eq!(action.id, "C7");
    assert!(action.warning.is_none());
    s.with_read(|c| {
        let row = stored::read(c, "T1")?
            .channels
            .into_iter()
            .find(|r| r.id == "C7")
            .unwrap();
        assert!(row.is_member && row.is_private);
        assert_eq!(row.purpose.as_deref(), Some("Для тестів"));
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn create_rejects_invalid_names_without_calling_slack() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let err = create(&client(&server), &store(), &bus(), "T1", "bad!", false, "")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Slack { ref code, .. } if code == "invalid_name_specials"));
}

#[tokio::test]
async fn taken_name_surfaces_slacks_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.create"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok": false, "error": "name_taken"})),
        )
        .mount(&server)
        .await;
    let err = create(
        &client(&server),
        &store(),
        &bus(),
        "T1",
        "general",
        false,
        "",
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::Slack { ref code, .. } if code == "name_taken"));
}

/// The UI live preview reads the same fixture (`channel-browser-model.test.ts`).
#[test]
fn name_rules_match_the_shared_fixture() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/channel-names.json")).unwrap();
    for case in cases {
        let input = case["input"].as_str().unwrap();
        let name = normalize_name(input);
        assert_eq!(name, case["normalized"].as_str().unwrap(), "{input:?}");
        assert_eq!(
            validate_name(&name).is_ok(),
            case["valid"].as_bool().unwrap(),
            "{input:?}"
        );
    }
}
