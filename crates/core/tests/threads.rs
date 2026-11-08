//! Thread replies sync.

use std::time::Duration;

use flek_core::api::Client;
use flek_core::store::messages::{self, MessageRow};
use flek_core::store::Store;
use flek_core::sync::backfill::{backfill_conversation, RateLimiter};
use flek_core::sync::threads::sync_threads;
use flek_core::sync::SyncOptions;
use flek_core::Secret;
use serde_json::json;
use wiremock::matchers::{body_string_contains, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
                 INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');",
            )?;
            Ok(())
        })
        .unwrap();
    store
}

fn ok(body: serde_json::Value) -> ResponseTemplate {
    let mut body = body;
    body["ok"] = json!(true);
    ResponseTemplate::new(200).set_body_json(body)
}

#[tokio::test]
async fn fetches_threads_once_and_dedupes_broadcasts() {
    let server = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    let store = store();
    let mut limiter = RateLimiter::new(Duration::ZERO);

    let root = json!({"ts": "100.000000", "thread_ts": "100.000000", "user": "U1", "text": "root",
                      "reply_count": 3, "latest_reply": "103.000000"});
    let broadcast = json!({"ts": "102.000000", "thread_ts": "100.000000", "user": "U2",
                           "subtype": "thread_broadcast", "text": "also in channel"});
    Mock::given(path("/conversations.history"))
        .respond_with(ok(json!({"messages": [
            {"ts": "104.000000", "user": "U1", "text": "plain"},
            broadcast.clone(),
            root.clone()
        ], "has_more": false})))
        .mount(&server)
        .await;
    // Two pages of replies.
    Mock::given(path("/conversations.replies"))
        .and(body_string_contains("cursor=page2"))
        .respond_with(ok(json!({"messages": [
            root.clone(),
            {"ts": "103.000000", "thread_ts": "100.000000", "user": "U1", "text": "reply 3"}
        ], "has_more": false})))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.replies"))
        .and(body_string_contains("ts=100.000000"))
        .respond_with(ok(json!({"messages": [
            root.clone(),
            {"ts": "101.000000", "thread_ts": "100.000000", "user": "U2", "text": "reply 1"},
            broadcast.clone()
        ], "has_more": true, "response_metadata": {"next_cursor": "page2"}})))
        .with_priority(2)
        .mount(&server)
        .await;

    backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    let stats = sync_threads(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    assert_eq!((stats.threads, stats.replies), (1, 3));

    let (thread, recent) = store
        .with_conn(|c| {
            Ok((
                messages::thread(c, "C1", "100.000000")?,
                messages::recent(c, "C1", 50)?,
            ))
        })
        .unwrap();
    let ts = |rows: &[messages::HistoryRow]| rows.iter().map(|r| r.ts.clone()).collect::<Vec<_>>();
    assert_eq!(
        ts(&thread),
        ["100.000000", "101.000000", "102.000000", "103.000000"]
    );
    // Channel view: root, broadcast, plain message; plain replies stay in the thread.
    assert_eq!(ts(&recent), ["100.000000", "102.000000", "104.000000"]);

    // Nothing changed → no replies requests.
    let before = server.received_requests().await.unwrap().len();
    let stats = sync_threads(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(stats.threads, 0);
    assert_eq!(server.received_requests().await.unwrap().len(), before);

    // Root reports a newer reply → thread is stale again.
    store
        .with_conn(|c| {
            messages::upsert_messages(
                c,
                "C1",
                &[MessageRow {
                    ts: "100.000000".into(),
                    thread_ts: Some("100.000000".into()),
                    text: "root".into(),
                    reply_count: 4,
                    latest_reply: Some("105.000000".into()),
                    raw_json: "{}".into(),
                    ..Default::default()
                }],
            )?;
            Ok(())
        })
        .unwrap();
    let stale = store
        .with_conn(|c| messages::stale_threads(c, "C1"))
        .unwrap();
    assert_eq!(stale, ["100.000000"]);
}

#[test]
fn hidden_roots_are_not_fetched() {
    let store = store();
    store
        .with_conn(|c| {
            messages::upsert_messages(
                c,
                "C1",
                &[MessageRow {
                    ts: "1.000000".into(),
                    reply_count: 2,
                    latest_reply: Some("2.000000".into()),
                    hidden_by_limit: true,
                    raw_json: "{}".into(),
                    ..Default::default()
                }],
            )
        })
        .unwrap();
    assert!(store
        .with_conn(|c| messages::stale_threads(c, "C1"))
        .unwrap()
        .is_empty());
}
