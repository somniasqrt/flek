//! Freshness never renders API responses; every assertion reads the DB.
mod support;
use flek_core::{
    api::Client,
    store::{
        directory,
        messages::{self, MessageRow, SyncState},
        Store,
    },
    sync::fresh_conversation,
    views, Secret,
};
use serde_json::json;
use support::{field, has_field};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn store() -> Store {
    let db = Store::open_in_memory().unwrap();
    db.with_conn(|c| {
        directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U1")?;
        let conv = serde_json::from_value(json!({"id":"C1","name":"general","is_channel":true}))?;
        directory::upsert_conversations(c, "T1", "U1", &[conv])?;
        messages::upsert_messages(
            c,
            "C1",
            &[MessageRow {
                ts: "1700000000.000001".into(),
                text: "old".into(),
                raw_json: "{}".into(),
                ..Default::default()
            }],
        )?;
        messages::set_sync_state(
            c,
            "C1",
            &SyncState {
                newest_ts: Some("1700000000.000001".into()),
                oldest_ts: Some("1700000000.000001".into()),
                complete: true,
            },
        )?;
        Ok(())
    })
    .unwrap();
    db
}
fn client(server: &MockServer) -> Client {
    Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing()
}
fn ok(value: serde_json::Value) -> ResponseTemplate {
    let mut v = value;
    v["ok"] = json!(true);
    ResponseTemplate::new(200).set_body_json(v)
}

#[tokio::test]
async fn catch_up_then_refresh_explicit_thread_including_all_pages() {
    let server = MockServer::start().await;
    let db = store();
    Mock::given(path("/conversations.history"))
        .and(field("channel", "C1"))
        .and(field("oldest", "1700000000.000001"))
        .respond_with(ok(
            json!({"messages":[{"ts":"1700000001.000001","text":"new"}],"has_more":false}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.replies")).and(field("ts","1700000000.000001")).and(field("cursor","next"))
        .respond_with(ok(json!({"messages":[{"ts":"1700000003.000001","thread_ts":"1700000000.000001","text":"reply two"}],"has_more":false}))).expect(1).with_priority(1).mount(&server).await;
    Mock::given(path("/conversations.replies")).and(field("ts","1700000000.000001"))
        .respond_with(ok(json!({"messages":[{"ts":"1700000000.000001","thread_ts":"1700000000.000001","text":"edited root","reply_count":2},{"ts":"1700000002.000001","thread_ts":"1700000000.000001","text":"reply one"}],"has_more":true,"response_metadata":{"next_cursor":"next"}}))).expect(1).with_priority(2).mount(&server).await;
    fresh_conversation(&client(&server), &db, "C1", Some("1700000000.000001"))
        .await
        .unwrap();
    db.with_read(|c| {
        let rows = views::history(c, "C1", 30, Some("1700000000.000001"))?;
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].text, "edited root");
        assert_eq!(rows[2].text, "reply two");
        assert_eq!(
            messages::sync_state(c, "C1")?.newest_ts.as_deref(),
            Some("1700000001.000001")
        );
        assert!(views::conversation(c, "C1")?.unwrap().synced_at.is_some());
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn fresh_without_thread_fetches_stale_roots_and_history_pages() {
    let server = MockServer::start().await;
    let db = store();
    Mock::given(path("/conversations.history")).and(has_field("cursor"))
        .respond_with(ok(json!({"messages":[{"ts":"1700000001.000001","text":"new root","reply_count":1}],"has_more":false}))).expect(1).with_priority(1).mount(&server).await;
    Mock::given(path("/conversations.history")).and(field("oldest","1700000000.000001"))
        .respond_with(ok(json!({"messages":[{"ts":"1700000004.000001","text":"newest"}],"has_more":true,"response_metadata":{"next_cursor":"next"}}))).expect(1).with_priority(2).mount(&server).await;
    Mock::given(path("/conversations.replies")).and(field("ts","1700000001.000001"))
        .respond_with(ok(json!({"messages":[{"ts":"1700000001.000001","text":"new root","reply_count":1},{"ts":"1700000002.000001","thread_ts":"1700000001.000001","text":"reply"}],"has_more":false}))).expect(1).mount(&server).await;
    fresh_conversation(&client(&server), &db, "C1", None)
        .await
        .unwrap();
    assert_eq!(
        db.with_read(|c| messages::thread(c, "C1", "1700000001.000001"))
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn failed_fresh_does_not_advance_sync_marker() {
    let server = MockServer::start().await;
    let db = store();
    Mock::given(path("/conversations.history"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":"invalid_auth"})),
        )
        .mount(&server)
        .await;
    assert!(fresh_conversation(&client(&server), &db, "C1", None)
        .await
        .unwrap_err()
        .is_auth_error());
    assert_eq!(
        db.with_read(|c| messages::sync_state(c, "C1"))
            .unwrap()
            .newest_ts
            .as_deref(),
        Some("1700000000.000001")
    );
}
