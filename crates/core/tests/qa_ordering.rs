//! QA: ordering and idempotency of sync and live events.

mod qa_common;

use qa_common::*;
use serde_json::json;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer};

use flek_core::store::messages;
use flek_core::sync::live::Applied;

fn reaction_count(store: &flek_core::store::Store, ts: &str) -> Vec<(String, i64)> {
    store
        .with_conn(|c| messages::reactions(c, "C1", ts))
        .unwrap()
}

#[tokio::test]
async fn same_page_synced_twice_changes_nothing() {
    let server = MockServer::start().await;
    let store = store();
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "3.000000", "user": "U1", "text": "three",
             "reactions": [{"name": "tada", "users": ["U1", "U2"], "count": 2}]},
            {"ts": "2.000000", "user": "U2", "text": "two",
             "files": [{"id": "F1", "name": "a.txt", "url_private_download": "https://files.slack.com/a"}]},
            {"ts": "1.000000", "user": "U1", "text": "one"}
        ])))
        .mount(&server)
        .await;
    let c = client(&server);
    backfill(&c, &store, &Default::default()).await;
    let snapshot = |s: &flek_core::store::Store| {
        (
            count(s, "SELECT COUNT(*) FROM messages"),
            count(s, "SELECT COUNT(*) FROM reactions"),
            count(s, "SELECT COUNT(*) FROM files"),
            count(s, "SELECT COUNT(*) FROM message_files"),
            count(
                s,
                "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'three'",
            ),
        )
    };
    let first = snapshot(&store);
    assert_eq!(first, (3, 2, 1, 1, 1));
    backfill(&c, &store, &full()).await;
    backfill(&c, &store, &full()).await;
    assert_eq!(snapshot(&store), first);
}

#[tokio::test]
async fn overlapping_pages_store_each_message_once() {
    let server = MockServer::start().await;
    let store = store();
    // Page 1 and page 2 overlap on 3.000000 (a message arrived between calls).
    Mock::given(path("/conversations.history"))
        .respond_with(ok(json!({"messages": [
            {"ts": "4.000000", "user": "U1", "text": "four"},
            {"ts": "3.000000", "user": "U1", "text": "three"}
        ], "has_more": true, "response_metadata": {"next_cursor": "p2"}})))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "3.000000", "user": "U1", "text": "three"},
            {"ts": "2.000000", "user": "U1", "text": "two"}
        ])))
        .with_priority(2)
        .mount(&server)
        .await;
    let c = client(&server);
    // Page 2 may be requested by cursor or by `latest`; either way the
    // overlapping message must be stored once. Bound the run.
    let r = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        backfill(&c, &store, &Default::default()),
    )
    .await;
    assert!(r.is_ok(), "backfill did not terminate");
    assert_eq!(count(&store, "SELECT COUNT(*) FROM messages"), 3);
}

// FINDING: an edit that arrives before the original message is overwritten
// by the older original (no edited_ts / ordering check).
#[tokio::test]
async fn edit_arriving_before_original_wins() {
    let store = live_store();
    ev(
        &store,
        json!({"type": "message", "subtype": "message_changed", "channel": "C1", "ts": "21.000000",
               "message": {"ts": "20.000000", "user": "U1", "text": "fixed typo", "edited": {"ts": "21.000000"}}}),
    )
    .await;
    ev(
        &store,
        json!({"type": "message", "channel": "C1", "ts": "20.000000", "user": "U1", "text": "fixd typo"}),
    )
    .await;
    let s = stored(&store, "20.000000").expect("message stored");
    assert_eq!(s.text, "fixed typo");
    assert_eq!(s.edited_ts.as_deref(), Some("21.000000"));
}

// FINDING: two edits delivered out of order: the older edit replaces the newer one.
#[tokio::test]
async fn older_edit_does_not_replace_newer_edit() {
    let store = live_store();
    upsert(&store, &[row("20.000000", "v1")]);
    ev(
        &store,
        json!({"type": "message", "subtype": "message_changed", "channel": "C1", "ts": "30.000000",
               "message": {"ts": "20.000000", "user": "U1", "text": "v3", "edited": {"ts": "30.000000"}}}),
    )
    .await;
    ev(
        &store,
        json!({"type": "message", "subtype": "message_changed", "channel": "C1", "ts": "25.000000",
               "message": {"ts": "20.000000", "user": "U1", "text": "v2", "edited": {"ts": "25.000000"}}}),
    )
    .await;
    let s = stored(&store, "20.000000").unwrap();
    assert_eq!(s.text, "v3");
    assert_eq!(s.edited_ts.as_deref(), Some("30.000000"));
}

// FINDING: a message_changed delivered after message_deleted un-deletes the
// message (deleted flag reset), so a deleted message reappears in the timeline.
#[tokio::test]
async fn late_edit_does_not_resurrect_a_deleted_message() {
    let store = live_store();
    upsert(&store, &[row("20.000000", "oops")]);
    ev(
        &store,
        json!({"type": "message", "subtype": "message_deleted", "channel": "C1", "ts": "22.000000",
               "deleted_ts": "20.000000"}),
    )
    .await;
    assert!(stored(&store, "20.000000").unwrap().deleted);
    ev(
        &store,
        json!({"type": "message", "subtype": "message_changed", "channel": "C1", "ts": "21.000000",
               "message": {"ts": "20.000000", "user": "U1", "text": "oops (edited)", "edited": {"ts": "21.000000"}}}),
    )
    .await;
    let s = stored(&store, "20.000000").unwrap();
    assert!(s.deleted, "late edit brought a deleted message back");
    let shown = store
        .with_conn(|c| messages::message(c, "C1", "20.000000"))
        .unwrap();
    assert!(shown.is_none());
}

#[tokio::test]
async fn duplicate_message_event_is_idempotent() {
    let store = live_store();
    let m =
        json!({"type": "message", "channel": "C1", "ts": "20.000000", "user": "U1", "text": "hi"});
    ev(&store, m.clone()).await;
    ev(&store, m).await;
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM messages WHERE conversation_id = 'C1'"
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'hi'"
        ),
        1
    );
}

#[tokio::test]
async fn reactions_added_and_removed_twice_stay_consistent() {
    let store = live_store();
    upsert(&store, &[row("20.000000", "hello")]);
    let r = |kind: &str, user: &str| {
        json!({"type": kind, "user": user, "reaction": "eyes",
               "item": {"type": "message", "channel": "C1", "ts": "20.000000"}})
    };
    ev(&store, r("reaction_added", "U1")).await;
    ev(&store, r("reaction_added", "U1")).await;
    ev(&store, r("reaction_added", "U2")).await;
    assert_eq!(
        reaction_count(&store, "20.000000"),
        vec![("eyes".into(), 2)]
    );
    ev(&store, r("reaction_removed", "U1")).await;
    ev(&store, r("reaction_removed", "U1")).await;
    assert_eq!(
        reaction_count(&store, "20.000000"),
        vec![("eyes".into(), 1)]
    );
    // Remove before add (out of order) must not underflow or error.
    ev(&store, r("reaction_removed", "U3")).await;
    ev(&store, r("reaction_removed", "U2")).await;
    assert!(reaction_count(&store, "20.000000").is_empty());
}

#[tokio::test]
async fn reaction_on_unknown_message_leaves_no_orphan() {
    let store = live_store();
    let a = ev(
        &store,
        json!({"type": "reaction_added", "user": "U1", "reaction": "eyes",
               "item": {"type": "message", "channel": "C1", "ts": "99.000000"}}),
    )
    .await;
    assert!(matches!(a, Applied::Ignored | Applied::Changed(_)));
    assert_eq!(count(&store, "SELECT COUNT(*) FROM reactions"), 0);
}

#[tokio::test]
async fn history_refresh_reflects_reactions_removed_while_offline() {
    let server = MockServer::start().await;
    let store = store();
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "3.000000", "user": "U1", "text": "three",
             "reactions": [{"name": "tada", "users": ["U1", "U2"], "count": 2}]}
        ])))
        .mount(&server)
        .await;
    let c = client(&server);
    backfill(&c, &store, &Default::default()).await;
    server.reset().await;
    Mock::given(path("/conversations.history"))
        .respond_with(history(
            json!([{"ts": "3.000000", "user": "U1", "text": "three"}]),
        ))
        .mount(&server)
        .await;
    backfill(&c, &store, &full()).await;
    assert!(reaction_count(&store, "3.000000").is_empty());
}

#[tokio::test]
async fn live_message_older_than_sync_state_is_still_stored() {
    // Live events never move sync markers, but a message that arrives late
    // (ts below newest_ts) must still be stored.
    let store = live_store();
    ev(
        &store,
        json!({"type": "message", "channel": "C1", "ts": "5.500000", "user": "U1", "text": "late"}),
    )
    .await;
    assert_eq!(stored(&store, "5.500000").unwrap().text, "late");
}
