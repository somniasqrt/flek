//! QA: "Never overwrite a stored message with a hidden/empty version"
//! (AGENTS.md §4), checked through every write path.

mod qa_common;
mod support;

use qa_common::*;
use serde_json::json;
use std::time::Duration;
use support::field;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer};

use flek_core::store::messages;
use flek_core::sync::threads::sync_threads;

#[tokio::test]
async fn live_message_event_with_empty_text_keeps_stored_text() {
    let store = live_store();
    upsert(&store, &[row("5.000000", "precious words")]);
    ev(
        &store,
        json!({"type": "message", "channel": "C1", "ts": "5.000000", "user": "U1", "text": ""}),
    )
    .await;
    let s = stored(&store, "5.000000").unwrap();
    assert_eq!(s.text, "precious words");
    assert!(!s.hidden);
}

#[tokio::test]
async fn live_message_changed_to_hidden_stub_keeps_stored_text() {
    let store = live_store();
    upsert(&store, &[row("5.000000", "precious words")]);
    ev(
        &store,
        json!({"type": "message", "subtype": "message_changed", "channel": "C1", "ts": "6.000000",
               "message": {"ts": "5.000000", "user": "U1", "text": "",
                           "files": [{"id": "F1", "mode": "hidden_by_limit"}]}}),
    )
    .await;
    let s = stored(&store, "5.000000").unwrap();
    assert_eq!(s.text, "precious words");
    assert!(!s.hidden);
}

// FINDING: an empty version that carries a subtype (thread_broadcast) is not
// treated as hidden, so a content-stripped broadcast wipes the stored text.
#[tokio::test]
async fn empty_thread_broadcast_from_history_keeps_stored_text() {
    let server = MockServer::start().await;
    let store = store();
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "6.000000", "thread_ts": "5.000000", "subtype": "thread_broadcast",
             "user": "U1", "text": "shared to channel too"},
            {"ts": "5.000000", "thread_ts": "5.000000", "user": "U1", "text": "root",
             "reply_count": 1, "latest_reply": "6.000000"}
        ])))
        .mount(&server)
        .await;
    let c = client(&server);
    backfill(&c, &store, &Default::default()).await;
    assert_eq!(
        stored(&store, "6.000000").unwrap().text,
        "shared to channel too"
    );

    server.reset().await;
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "6.000000", "thread_ts": "5.000000", "subtype": "thread_broadcast",
             "user": "U1", "text": ""},
            {"ts": "5.000000", "thread_ts": "5.000000", "user": "U1", "text": "",
             "reply_count": 1, "latest_reply": "6.000000"}
        ])))
        .mount(&server)
        .await;
    backfill(&c, &store, &full()).await;
    assert_eq!(stored(&store, "5.000000").unwrap().text, "root");
    assert_eq!(
        stored(&store, "6.000000").unwrap().text,
        "shared to channel too"
    );
}

#[tokio::test]
async fn backfill_hidden_stub_keeps_text_and_file_metadata() {
    let server = MockServer::start().await;
    let store = store();
    let base = server.uri();
    let file = json!({"id": "F1", "name": "report.pdf", "title": "Report", "mimetype": "application/pdf",
                      "size": 5, "url_private_download": format!("{base}/dl/F1/report.pdf"),
                      "permalink": "https://fake.slack.com/files/F1"});
    Mock::given(path("/conversations.history"))
        .respond_with(history(
            json!([{"ts": "2.000000", "user": "U1", "text": "see report", "files": [file]}]),
        ))
        .mount(&server)
        .await;
    let c = client(&server);
    backfill(&c, &store, &Default::default()).await;
    store
        .with_conn(|c| flek_core::store::files::set_local_path(c, "F1", "/cache/F1/report.pdf"))
        .unwrap();

    // Later the Free plan hides it: content stripped, file stubbed.
    server.reset().await;
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([{"ts": "2.000000", "user": "U1", "text": "",
                                       "files": [{"id": "F1", "mode": "hidden_by_limit"}]}])))
        .mount(&server)
        .await;
    backfill(&c, &store, &full()).await;

    let s = stored(&store, "2.000000").unwrap();
    assert_eq!(s.text, "see report");
    assert!(!s.hidden);
    let (name, url, local, hidden): (Option<String>, Option<String>, Option<String>, bool) = store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT name, url_private_download, local_path, hidden_by_limit FROM files WHERE id = 'F1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!(name.as_deref(), Some("report.pdf"));
    assert!(url.is_some(), "download URL lost");
    assert_eq!(local.as_deref(), Some("/cache/F1/report.pdf"));
    assert!(!hidden);
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM message_files WHERE ts = '2.000000' AND file_id = 'F1'"
        ),
        1
    );
}

#[tokio::test]
async fn hidden_file_stub_on_visible_message_keeps_file_metadata() {
    let server = MockServer::start().await;
    let store = store();
    let base = server.uri();
    let file = json!({"id": "F1", "name": "report.pdf", "mimetype": "application/pdf", "size": 5,
                      "url_private_download": format!("{base}/dl/F1/report.pdf")});
    Mock::given(path("/conversations.history"))
        .respond_with(history(
            json!([{"ts": "2.000000", "user": "U1", "text": "see report", "files": [file]}]),
        ))
        .mount(&server)
        .await;
    let c = client(&server);
    backfill(&c, &store, &Default::default()).await;
    store
        .with_conn(|c| flek_core::store::files::set_local_path(c, "F1", "/cache/F1/report.pdf"))
        .unwrap();

    // The same file shared again in a later, visible message, but by then
    // Slack hides the file itself.
    server.reset().await;
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "3.000000", "user": "U1", "text": "re-sharing the report",
             "files": [{"id": "F1", "mode": "hidden_by_limit"}]},
            {"ts": "2.000000", "user": "U1", "text": "see report",
             "files": [{"id": "F1", "mode": "hidden_by_limit"}]}
        ])))
        .mount(&server)
        .await;
    backfill(&c, &store, &full()).await;

    let (name, local): (Option<String>, Option<String>) = store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT name, local_path FROM files WHERE id = 'F1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(name.as_deref(), Some("report.pdf"));
    assert_eq!(local.as_deref(), Some("/cache/F1/report.pdf"));
}

#[tokio::test]
async fn thread_replies_hidden_stub_keeps_stored_reply() {
    let server = MockServer::start().await;
    let store = store();
    let root = json!({"ts": "100.000000", "thread_ts": "100.000000", "user": "U1", "text": "root",
                      "reply_count": 1, "latest_reply": "101.000000"});
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([root.clone()])))
        .mount(&server)
        .await;
    Mock::given(path("/conversations.replies"))
        .and(field("ts", "100.000000"))
        .respond_with(ok(json!({"messages": [root.clone(),
            {"ts": "101.000000", "thread_ts": "100.000000", "user": "U2", "text": "a precious reply"}],
            "has_more": false})))
        .mount(&server)
        .await;
    let c = client(&server);
    let mut limiter = flek_core::sync::backfill::RateLimiter::new(Duration::ZERO);
    backfill(&c, &store, &Default::default()).await;
    sync_threads(&c, &store, &mut limiter, "C1", &Default::default())
        .await
        .unwrap();
    assert_eq!(
        stored(&store, "101.000000").unwrap().text,
        "a precious reply"
    );

    server.reset().await;
    Mock::given(path("/conversations.replies"))
        .respond_with(ok(json!({"messages": [root.clone(),
            {"ts": "101.000000", "thread_ts": "100.000000", "user": "U2", "text": ""}],
            "has_more": false})))
        .mount(&server)
        .await;
    sync_threads(&c, &store, &mut limiter, "C1", &full())
        .await
        .unwrap();
    let s = stored(&store, "101.000000").unwrap();
    assert_eq!(s.text, "a precious reply");
    assert!(!s.hidden);
    assert!(!s.deleted);
}

#[test]
fn import_of_hidden_slack_export_message_keeps_stored_text() {
    let store = store();
    upsert(&store, &[row("1700000000.000100", "synced text")]);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("channels.json"),
        json!([{"id": "C1", "name": "general"}]).to_string(),
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("general")).unwrap();
    std::fs::write(
        dir.path().join("general/2023-11-14.json"),
        json!([{"type": "message", "ts": "1700000000.000100", "user": "U1", "text": "",
                "files": [{"id": "F9", "mode": "hidden_by_limit"}]}])
        .to_string(),
    )
    .unwrap();
    store
        .with_conn(|c| flek_core::sync::import::import_path(c, "T1", dir.path(), None))
        .unwrap();
    assert_eq!(
        stored(&store, "1700000000.000100").unwrap().text,
        "synced text"
    );
}

#[test]
fn import_does_not_resurrect_a_deleted_message() {
    // Spec: archives only fill gaps (new, or stored copy hidden by Slack).
    // A message the author deleted is neither.
    let store = store();
    upsert(&store, &[row("1700000000.000100", "since deleted")]);
    store
        .with_conn(|c| messages::mark_deleted(c, "C1", &["1700000000.000100".into()]))
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("general.json");
    std::fs::write(
        &file,
        json!([{"ts": "1700000000.000100", "author": "Alice", "text": "since deleted"}])
            .to_string(),
    )
    .unwrap();
    store
        .with_conn(|c| flek_core::sync::import::import_path(c, "T1", &file, None))
        .unwrap();
    assert!(stored(&store, "1700000000.000100").unwrap().deleted);
}

#[tokio::test]
async fn live_message_replied_with_hidden_root_keeps_root_text() {
    let store = live_store();
    upsert(
        &store,
        &[{
            let mut r = row("5.000000", "root text");
            r.thread_ts = Some("5.000000".into());
            r
        }],
    );
    ev(
        &store,
        json!({"type": "message", "subtype": "message_replied", "channel": "C1", "ts": "7.000000",
               "message": {"ts": "5.000000", "thread_ts": "5.000000", "user": "U1", "text": "",
                           "reply_count": 2, "latest_reply": "7.000000"}}),
    )
    .await;
    assert_eq!(stored(&store, "5.000000").unwrap().text, "root text");
}

#[test]
fn hidden_message_is_never_marked_deleted_and_full_version_recovers() {
    let store = store();
    let mut hidden = row("3.000000", "");
    hidden.hidden_by_limit = true;
    upsert(&store, &[hidden]);
    store
        .with_conn(|c| messages::mark_deleted(c, "C1", &["3.000000".into()]))
        .unwrap();
    assert!(!stored(&store, "3.000000").unwrap().deleted);
    upsert(&store, &[row("3.000000", "back from archive")]);
    let s = stored(&store, "3.000000").unwrap();
    assert_eq!(s.text, "back from archive");
    assert!(!s.hidden);
}

// FINDING: a live content-stripped message with a subtype (here a
// thread_broadcast with empty text) wipes the stored text.
#[tokio::test]
async fn live_empty_thread_broadcast_keeps_stored_text() {
    let store = live_store();
    let mut b = row("6.000000", "shared to channel too");
    b.thread_ts = Some("5.000000".into());
    b.subtype = Some("thread_broadcast".into());
    upsert(&store, &[b]);
    ev(
        &store,
        json!({"type": "message", "subtype": "message_changed", "channel": "C1", "ts": "8.000000",
               "message": {"ts": "6.000000", "thread_ts": "5.000000", "subtype": "thread_broadcast",
                           "user": "U1", "text": ""}}),
    )
    .await;
    assert_eq!(
        stored(&store, "6.000000").unwrap().text,
        "shared to channel too"
    );
}
