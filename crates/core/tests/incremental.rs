//! Refresh window, active threads, full resync.

use std::time::Duration;

use flek_core::api::Client;
use flek_core::store::messages;
use flek_core::store::Store;
use flek_core::sync::backfill::{backfill_conversation, RateLimiter};
use flek_core::sync::threads::sync_threads;
use flek_core::sync::SyncOptions;
use flek_core::Secret;
use serde_json::{json, Value};
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

fn page(messages: Vec<Value>) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_json(json!({"ok": true, "has_more": false, "messages": messages}))
}

fn msg(ts: &str, text: &str) -> Value {
    json!({"ts": ts, "user": "U1", "text": text})
}

fn window(since: &str) -> SyncOptions {
    SyncOptions {
        refresh_since: Some(since.into()),
        ..Default::default()
    }
}

fn texts(store: &Store) -> Vec<(String, String)> {
    store
        .with_conn(|c| {
            Ok(messages::recent(c, "C1", 100)?
                .into_iter()
                .map(|r| (r.ts, r.text))
                .collect())
        })
        .unwrap()
}

async fn setup() -> (MockServer, Client, Store, RateLimiter) {
    let server = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    (server, client, store(), RateLimiter::new(Duration::ZERO))
}

#[tokio::test]
async fn refresh_window_picks_up_edits_reactions_and_deletions() {
    let (server, client, store, mut limiter) = setup().await;
    Mock::given(path("/conversations.history"))
        .respond_with(page(vec![
            msg("30.000000", "c"),
            msg("20.000000", "b"),
            msg("10.000000", "a"),
        ]))
        .mount(&server)
        .await;
    backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();

    // Offline: "b" edited and got a reaction, "c" deleted, "a" (outside the window) deleted too.
    server.reset().await;
    Mock::given(path("/conversations.history"))
        .and(body_string_contains("oldest=15.000000"))
        .respond_with(page(vec![
            json!({"ts": "20.000000", "user": "U1", "text": "b (fixed)", "edited": {"ts": "25.000000"},
                   "reactions": [{"name": "eyes", "users": ["U2"], "count": 1}]}),
        ]))
        .expect(1)
        .mount(&server)
        .await;
    let stats = backfill_conversation(&client, &store, &mut limiter, "C1", &window("15.000000"))
        .await
        .unwrap();
    assert_eq!(stats.deleted, 1);
    assert_eq!(
        texts(&store),
        [
            ("10.000000".into(), "a".into()),
            ("20.000000".into(), "b (fixed)".into())
        ]
    );
    let reactions = store
        .with_conn(|c| messages::reactions(c, "C1", "20.000000"))
        .unwrap();
    assert_eq!(reactions, [("eyes".to_string(), 1)]);
}

#[tokio::test]
async fn active_old_threads_are_refetched_inside_the_window() {
    let (server, client, store, mut limiter) = setup().await;
    let root = json!({"ts": "10.000000", "thread_ts": "10.000000", "user": "U1", "text": "old root",
                      "reply_count": 1, "latest_reply": "50.000000"});
    Mock::given(path("/conversations.history"))
        .respond_with(page(vec![msg("60.000000", "recent"), root.clone()]))
        .mount(&server)
        .await;
    Mock::given(path("/conversations.replies"))
        .respond_with(page(vec![
            root.clone(),
            json!({"ts": "50.000000", "thread_ts": "10.000000", "user": "U2", "text": "r1"}),
        ]))
        .mount(&server)
        .await;
    backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    sync_threads(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();

    // Without a window the thread is up to date.
    let none = store
        .with_conn(|c| messages::threads_to_fetch(c, "C1", None, false))
        .unwrap();
    assert!(none.is_empty());

    // A reply arrived and r1 was deleted while offline; the root is far below the window.
    server.reset().await;
    Mock::given(path("/conversations.history"))
        .respond_with(page(vec![]))
        .mount(&server)
        .await;
    Mock::given(path("/conversations.replies"))
        .respond_with(page(vec![
            root,
            json!({"ts": "70.000000", "thread_ts": "10.000000", "user": "U2", "text": "r2"}),
        ]))
        .expect(1)
        .mount(&server)
        .await;
    let opts = window("40.000000");
    backfill_conversation(&client, &store, &mut limiter, "C1", &opts)
        .await
        .unwrap();
    let stats = sync_threads(&client, &store, &mut limiter, "C1", &opts)
        .await
        .unwrap();
    assert_eq!((stats.threads, stats.replies, stats.deleted), (1, 1, 1));
    let thread: Vec<String> = store
        .with_conn(|c| {
            Ok(messages::thread(c, "C1", "10.000000")?
                .into_iter()
                .map(|r| r.text)
                .collect())
        })
        .unwrap();
    assert_eq!(thread, ["old root", "r2"]);
}

#[tokio::test]
async fn full_resync_walks_everything_again_without_loss() {
    let (server, client, store, mut limiter) = setup().await;
    Mock::given(path("/conversations.history"))
        .respond_with(page(vec![msg("20.000000", "b"), msg("10.000000", "a")]))
        .mount(&server)
        .await;
    backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();

    // Slack now hides "a"; a full pass must not erase it.
    server.reset().await;
    Mock::given(path("/conversations.history"))
        .and(body_string_contains("oldest=20.000000"))
        .respond_with(page(vec![]))
        .mount(&server)
        .await;
    Mock::given(path("/conversations.history"))
        .respond_with(page(vec![
            msg("20.000000", "b"),
            json!({"ts": "10.000000", "user": "U1", "text": ""}),
        ]))
        .expect(1)
        .mount(&server)
        .await;
    let full = SyncOptions {
        full: true,
        ..Default::default()
    };
    let stats = backfill_conversation(&client, &store, &mut limiter, "C1", &full)
        .await
        .unwrap();
    assert!(stats.complete);
    assert_eq!(
        texts(&store),
        [
            ("10.000000".into(), "a".into()),
            ("20.000000".into(), "b".into())
        ]
    );
}
