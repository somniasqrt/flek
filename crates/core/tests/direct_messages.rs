//! Opening 1:1 direct messages and group DMs (M4.8.1) against a temp SQLite
//! database and a mock Slack server. Never touches real Slack or Keychain.
mod support;

use flek_core::{
    api::Client,
    store::{directory, Store},
    sync::{
        direct,
        live::{self, Change},
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
        Secret::new("xoxc-fake-token"),
        Secret::new("xoxd-fake-cookie"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing()
}

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.with_conn(|c| {
        directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U1")?;
        c.execute_batch(
            "INSERT INTO users (id, team_id, name, real_name, display_name, is_bot, deleted) VALUES
               ('U1','T1','me','Me','Me',0,0),
               ('U2','T1','alice','Alice','Alice',0,0),
               ('U3','T1','bob','Bob','Bob',0,0),
               ('U4','T1','dan','Dan','Dan',0,0),
               ('U5','T1','eve','Eve','Eve',0,0),
               ('U6','T1','frank','Frank','Frank',0,0),
               ('U7','T1','grace','Grace','Grace',0,0),
               ('U8','T1','heidi','Heidi','Heidi',0,0),
               ('U9','T1','ivan','Ivan','Ivan',0,0),
               ('U10','T1','judy','Judy','Judy',0,0),
               ('UGONE','T1','gone','Gone','Gone',0,1),
               ('UBOT','T1','helper','Helper Bot','Helper Bot',1,0);
             INSERT INTO conversations (id, team_id, kind, label, dm_user_id, is_member, is_archived, closed_at) VALUES
               ('D2','T1','im','Alice','U2',1,0,NULL),
               ('D5','T1','im','Eve','U5',1,0,1700000000);
             INSERT INTO teams (id, name) VALUES ('T2','Other');
             INSERT INTO users (id, team_id, name, real_name, display_name, deleted) VALUES
               ('UX','T2','foreign','Foreign','Foreign',0);",
        )?;
        Ok(())
    })
    .unwrap();
    s
}

fn empty_history() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "ok": true,
        "messages": [],
        "has_more": false,
    }))
}

#[tokio::test]
async fn cached_open_dm_returns_without_network_or_bus_events() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .respond_with(empty_history())
        .expect(0)
        .mount(&server)
        .await;

    let (s, bus, c) = (store(), live::bus(), client(&server));
    let mut rx = bus.subscribe();
    let result = direct::open(&c, &s, &bus, "T1", "U1", &["U2".into()])
        .await
        .unwrap();
    assert_eq!(result.id, "D2");
    assert_eq!(result.warning, None);
    assert!(rx.try_recv().is_err(), "no bus events for cached DM");
}

#[tokio::test]
async fn person_without_dm_opens_stores_catches_up_and_announces() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .and(field("users", "U4"))
        .and(field("return_im", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channel": { "id": "D4", "is_im": true, "user": "U4" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .and(field("channel", "D4"))
        .respond_with(empty_history())
        .expect(1)
        .mount(&server)
        .await;

    let (s, bus, c) = (store(), live::bus(), client(&server));
    let mut rx = bus.subscribe();
    let result = direct::open(&c, &s, &bus, "T1", "U1", &["U4".into()])
        .await
        .unwrap();
    assert_eq!(result.id, "D4");
    assert_eq!(result.warning, None);

    let (kind, dm_user_id, is_member, closed_at): (String, Option<String>, bool, Option<i64>) = s
        .with_read(|conn| {
            Ok(conn.query_row(
                "SELECT kind, dm_user_id, is_member, closed_at FROM conversations WHERE id = ?1",
                ["D4"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!(kind, "im");
    assert_eq!(dm_user_id.as_deref(), Some("U4"));
    assert!(is_member);
    assert_eq!(closed_at, None);

    assert_eq!(rx.try_recv().unwrap(), Change::Directory);
    assert_eq!(
        rx.try_recv().unwrap(),
        Change::History {
            conversation_id: "D4".into(),
        }
    );
}

#[tokio::test]
async fn closed_dm_reopens_via_slack_and_clears_closed_at() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .and(field("users", "U5"))
        .and(field("return_im", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channel": { "id": "D5" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .and(field("channel", "D5"))
        .respond_with(empty_history())
        .expect(1)
        .mount(&server)
        .await;

    let (s, bus, c) = (store(), live::bus(), client(&server));
    let result = direct::open(&c, &s, &bus, "T1", "U1", &["U5".into()])
        .await
        .unwrap();
    assert_eq!(result.id, "D5");
    assert_eq!(result.warning, None);

    let (kind, dm_user_id, closed_at): (String, Option<String>, Option<i64>) = s
        .with_read(|conn| {
            Ok(conn.query_row(
                "SELECT kind, dm_user_id, closed_at FROM conversations WHERE id = ?1",
                ["D5"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(kind, "im");
    assert_eq!(dm_user_id.as_deref(), Some("U5"));
    assert_eq!(closed_at, None);
}

#[tokio::test]
async fn group_dm_deduplicates_drops_self_keeps_order_and_stores_mpim() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .and(field("users", "U2,U3"))
        .and(field("return_im", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channel": { "id": "G9", "name": "mpdm-me--alice--bob-1" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .and(field("channel", "G9"))
        .respond_with(empty_history())
        .expect(1)
        .mount(&server)
        .await;

    let (s, bus, c) = (store(), live::bus(), client(&server));
    let result = direct::open(
        &c,
        &s,
        &bus,
        "T1",
        "U1",
        &["U2".into(), "U3".into(), "U2".into(), "U1".into()],
    )
    .await
    .unwrap();
    assert_eq!(result.id, "G9");
    assert_eq!(result.warning, None);

    let (kind, label, is_member): (String, String, bool) = s
        .with_read(|conn| {
            Ok(conn.query_row(
                "SELECT kind, label, is_member FROM conversations WHERE id = ?1",
                ["G9"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(kind, "mpim");
    assert_eq!(label, "Alice, Bob");
    assert!(is_member);
}

#[tokio::test]
async fn self_dm_is_allowed_and_sends_own_user_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .and(field("users", "U1"))
        .and(field("return_im", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channel": { "id": "D1", "is_im": true, "user": "U1" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .and(field("channel", "D1"))
        .respond_with(empty_history())
        .expect(1)
        .mount(&server)
        .await;

    let (s, bus, c) = (store(), live::bus(), client(&server));
    let result = direct::open(&c, &s, &bus, "T1", "U1", &["U1".into()])
        .await
        .unwrap();
    assert_eq!(result.id, "D1");
    assert_eq!(result.warning, None);
}

#[tokio::test]
async fn validation_rejects_bad_inputs_without_any_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .respond_with(empty_history())
        .expect(0)
        .mount(&server)
        .await;

    let (s, bus, c) = (store(), live::bus(), client(&server));

    // 0 ids
    assert!(matches!(
        direct::open(&c, &s, &bus, "T1", "U1", &[]).await,
        Err(Error::Usage(_))
    ));
    // 9 distinct non-self ids
    let nine: Vec<String> = (2..=10).map(|i| format!("U{i}")).collect();
    assert!(matches!(
        direct::open(&c, &s, &bus, "T1", "U1", &nine).await,
        Err(Error::Usage(_))
    ));
    // Unknown id and user from another team
    assert!(matches!(
        direct::open(&c, &s, &bus, "T1", "U1", &["UNOPE".into()]).await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        direct::open(&c, &s, &bus, "T1", "U1", &["UX".into()]).await,
        Err(Error::NotFound(_))
    ));
    // Deleted user
    assert!(matches!(
        direct::open(&c, &s, &bus, "T1", "U1", &["UGONE".into()]).await,
        Err(Error::Usage(_))
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn response_validation_rejects_channel_id_for_single_dm_and_stores_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .and(field("users", "U4"))
        .and(field("return_im", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channel": { "id": "C9", "is_im": true, "user": "U4" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .respond_with(empty_history())
        .expect(0)
        .mount(&server)
        .await;

    let (s, bus, c) = (store(), live::bus(), client(&server));
    let err = direct::open(&c, &s, &bus, "T1", "U1", &["U4".into()])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Store(_)));

    let stored: i64 = s
        .with_read(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM conversations WHERE id = 'C9'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn history_failure_after_open_returns_ok_with_catch_up_failed_warning() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .and(field("users", "UBOT"))
        .and(field("return_im", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "channel": { "id": "DBOT", "is_im": true, "user": "UBOT" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .and(field("channel", "DBOT"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": false,
            "error": "internal_error"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (s, bus, c) = (store(), live::bus(), client(&server));
    let mut rx = bus.subscribe();
    let result = direct::open(&c, &s, &bus, "T1", "U1", &["UBOT".into()])
        .await
        .unwrap();
    assert_eq!(result.id, "DBOT");
    assert_eq!(result.warning.as_deref(), Some("catch_up_failed"));

    let stored: i64 = s
        .with_read(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM conversations WHERE id = 'DBOT' AND dm_user_id = 'UBOT'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(stored, 1);
    assert_eq!(rx.try_recv().unwrap(), Change::Directory);
    assert_eq!(
        rx.try_recv().unwrap(),
        Change::History {
            conversation_id: "DBOT".into(),
        }
    );
}
