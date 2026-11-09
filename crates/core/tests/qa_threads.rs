//! QA: thread roots, replies, reply counts and broadcasts.

mod qa_common;

use qa_common::*;
use serde_json::json;

use flek_core::store::messages;
use flek_core::store::Store;

fn channel_ts(store: &Store) -> Vec<String> {
    store
        .with_conn(|c| messages::page(c, "C1", None, 100))
        .unwrap()
        .into_iter()
        .map(|r| r.ts)
        .collect()
}

fn thread_ts(store: &Store, root: &str) -> Vec<String> {
    store
        .with_conn(|c| messages::thread(c, "C1", root))
        .unwrap()
        .into_iter()
        .map(|r| r.ts)
        .collect()
}

fn root_replies(store: &Store, root: &str) -> i64 {
    store
        .with_conn(|c| messages::page(c, "C1", None, 100))
        .unwrap()
        .into_iter()
        .find(|r| r.ts == root)
        .map(|r| r.reply_count)
        .unwrap()
}

async fn seed_root(store: &Store) {
    ev(
        store,
        json!({"type": "message", "channel": "C1", "ts": "20.000000", "user": "U1", "text": "root"}),
    )
    .await;
}

#[tokio::test]
async fn live_reply_shows_in_thread_not_channel_broadcast_in_both() {
    let store = live_store();
    seed_root(&store).await;
    ev(
        &store,
        json!({"type": "message", "channel": "C1", "ts": "21.000000", "thread_ts": "20.000000",
               "user": "U2", "text": "quiet reply"}),
    )
    .await;
    ev(
        &store,
        json!({"type": "message", "subtype": "thread_broadcast", "channel": "C1", "ts": "22.000000",
               "thread_ts": "20.000000", "user": "U2", "text": "loud reply",
               "root": {"ts": "20.000000", "user": "U1", "text": "root"}}),
    )
    .await;
    assert_eq!(channel_ts(&store), vec!["20.000000", "22.000000"]);
    assert_eq!(
        thread_ts(&store, "20.000000"),
        vec!["20.000000", "21.000000", "22.000000"]
    );
}

#[tokio::test]
async fn message_replied_updates_root_reply_count_without_touching_text() {
    let store = live_store();
    seed_root(&store).await;
    ev(
        &store,
        json!({"type": "message", "subtype": "message_replied", "channel": "C1", "ts": "21.000000",
               "message": {"ts": "20.000000", "thread_ts": "20.000000", "user": "U1", "text": "root",
                           "reply_count": 2, "latest_reply": "21.000000"}}),
    )
    .await;
    assert_eq!(root_replies(&store, "20.000000"), 2);
    assert_eq!(stored(&store, "20.000000").unwrap().text, "root");
}

// FINDING: a live reply alone does not bump the root's reply count; the
// channel shows "0 replies" until a message_replied event or a resync
// (Slack does not reliably send message_replied over RTM).
#[tokio::test]
async fn live_reply_bumps_root_reply_count() {
    // Sane-client expectation: the root shows its new reply immediately.
    let store = live_store();
    seed_root(&store).await;
    ev(
        &store,
        json!({"type": "message", "channel": "C1", "ts": "21.000000", "thread_ts": "20.000000",
               "user": "U2", "text": "reply"}),
    )
    .await;
    assert_eq!(root_replies(&store, "20.000000"), 1);
}

#[tokio::test]
async fn reply_to_unknown_root_is_kept_and_hidden_from_channel() {
    let store = live_store();
    ev(
        &store,
        json!({"type": "message", "channel": "C1", "ts": "21.000000", "thread_ts": "15.000000",
               "user": "U2", "text": "orphan reply"}),
    )
    .await;
    assert_eq!(stored(&store, "21.000000").unwrap().text, "orphan reply");
    assert!(channel_ts(&store).is_empty());
}

#[tokio::test]
async fn deleting_a_root_keeps_its_replies_reachable() {
    // Sane-client expectation: Slack keeps replies of a deleted root (the
    // root becomes "This message was deleted"); local replies must not vanish.
    let store = live_store();
    seed_root(&store).await;
    ev(
        &store,
        json!({"type": "message", "channel": "C1", "ts": "21.000000", "thread_ts": "20.000000",
               "user": "U2", "text": "reply survives"}),
    )
    .await;
    ev(
        &store,
        json!({"type": "message", "subtype": "message_deleted", "channel": "C1", "ts": "23.000000",
               "deleted_ts": "20.000000"}),
    )
    .await;
    assert!(!stored(&store, "21.000000").unwrap().deleted);
    assert!(thread_ts(&store, "20.000000").contains(&"21.000000".to_string()));
}

#[tokio::test]
async fn live_reply_count_is_not_double_counted() {
    let store = live_store();
    seed_root(&store).await;
    let reply = json!({"type": "message", "channel": "C1", "ts": "21.000000",
                       "thread_ts": "20.000000", "user": "U2", "text": "reply"});
    ev(&store, reply.clone()).await;
    ev(&store, reply).await;
    assert_eq!(root_replies(&store, "20.000000"), 1);
    // Slack's own count afterwards agrees, and is not added on top.
    ev(
        &store,
        json!({"type": "message", "subtype": "message_replied", "channel": "C1", "ts": "21.000000",
               "message": {"ts": "20.000000", "thread_ts": "20.000000", "user": "U1", "text": "root",
                           "reply_count": 1, "latest_reply": "21.000000"}}),
    )
    .await;
    assert_eq!(root_replies(&store, "20.000000"), 1);
    ev(
        &store,
        json!({"type": "message", "channel": "C1", "ts": "22.000000", "thread_ts": "20.000000",
               "user": "U2", "text": "second"}),
    )
    .await;
    assert_eq!(root_replies(&store, "20.000000"), 2);
}
