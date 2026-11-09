//! QA: archive import (docs/import-formats.md).

mod qa_common;

use std::fs;
use std::path::Path;

use qa_common::*;
use serde_json::{json, Value};

use flek_core::store::messages;
use flek_core::store::Store;
use flek_core::sync::import::{import_path, ImportStats};

fn write(path: &Path, v: &Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec(v).unwrap()).unwrap();
}

fn import(store: &Store, path: &Path) -> flek_core::Result<ImportStats> {
    store.with_conn(|c| import_path(c, "T1", path, None))
}

fn simple(store: &Store, v: Value) -> flek_core::Result<ImportStats> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("general.json");
    write(&file, &v);
    import(store, &file)
}

#[test]
fn malformed_json_fails_cleanly_and_writes_nothing() {
    let store = store();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("general.json");
    fs::write(&file, b"[{\"ts\": \"1700000000.000100\", \"text\": \"half").unwrap();
    assert!(import(&store, &file).is_err());
    fs::write(&file, b"{\"not\": \"an array\"}").unwrap();
    assert!(import(&store, &file).is_err());
    assert_eq!(count(&store, "SELECT COUNT(*) FROM messages"), 0);
}

#[test]
fn entries_without_ts_never_become_rows() {
    let store = store();
    let _ = simple(
        &store,
        json!([
            {"author": "Alice", "text": "no ts at all"},
            {"ts": "", "author": "Alice", "text": "empty ts"},
            {"ts": null, "author": "Alice", "text": "null ts"},
            {"ts": "1700000000.000100", "author": "Alice", "text": "good"}
        ]),
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM messages WHERE ts IS NULL OR ts = '' OR text != 'good'"
        ),
        0
    );
}

#[test]
fn numeric_ts_is_normalised_to_fixed_width() {
    let store = store();
    simple(
        &store,
        json!([
            {"ts": 1700000000.1, "author": "Alice", "text": "tenth"},
            {"ts": 1700000001.0001, "author": "Alice", "text": "micro"},
            {"ts": 1700000002, "author": "Alice", "text": "integer"}
        ]),
    )
    .unwrap();
    let mut ts: Vec<String> = store
        .with_conn(|c| {
            let mut s = c.prepare("SELECT ts FROM messages ORDER BY ts")?;
            let v = s
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            Ok(v)
        })
        .unwrap();
    ts.sort();
    assert_eq!(
        ts,
        vec![
            "1700000000.100000",
            "1700000001.000100",
            "1700000002.000000"
        ]
    );
}

#[test]
fn duplicates_within_and_across_imports() {
    let store = store();
    let archive = json!([
        {"ts": "1700000000.000100", "author": "Alice", "text": "first copy"},
        {"ts": "1700000000.000100", "author": "Alice", "text": "second copy"},
        {"ts": "1700000050.000200", "author": "bob", "text": "other"}
    ]);
    simple(&store, archive.clone()).unwrap();
    assert_eq!(count(&store, "SELECT COUNT(*) FROM messages"), 2);
    let again = simple(&store, archive).unwrap();
    assert_eq!(again.written, 0);
    assert_eq!(count(&store, "SELECT COUNT(*) FROM messages"), 2);
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'other'"
        ),
        1
    );
}

// FINDING: importing over a hidden stub drops what Slack still told us about
// it (thread_ts / reply_count / reactions), so the thread root loses its replies.
#[test]
fn filling_a_hidden_stub_keeps_thread_and_reactions() {
    let store = store();
    let mut stub = row("1700000000.000100", "");
    stub.hidden_by_limit = true;
    stub.thread_ts = Some("1700000000.000100".into());
    stub.reply_count = 3;
    stub.reactions = vec![("tada".into(), "U2".into())];
    upsert(&store, &[stub]);
    assert_eq!(stored(&store, "1700000000.000100").unwrap().reply_count, 3);
    assert_eq!(count(&store, "SELECT COUNT(*) FROM reactions"), 1);
    simple(
        &store,
        json!([{"ts": "1700000000.000100", "author": "Alice", "text": "recovered from archive"}]),
    )
    .unwrap();
    let s = stored(&store, "1700000000.000100").unwrap();
    assert_eq!(s.text, "recovered from archive");
    assert!(!s.hidden);
    let mut lost = Vec::new();
    if s.reply_count != 3 {
        lost.push(format!("reply_count {}", s.reply_count));
    }
    let thread_ts: Option<String> = store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT thread_ts FROM messages WHERE ts = '1700000000.000100'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    if thread_ts.as_deref() != Some("1700000000.000100") {
        lost.push(format!("thread_ts {thread_ts:?}"));
    }
    let reactions = store
        .with_conn(|c| messages::reactions(c, "C1", "1700000000.000100"))
        .unwrap();
    if reactions != vec![("tada".to_string(), 1)] {
        lost.push(format!("reactions {reactions:?}"));
    }
    assert!(lost.is_empty(), "lost: {}", lost.join(", "));
}

#[test]
fn archived_reply_lands_in_thread_not_channel() {
    let store = store();
    simple(
        &store,
        json!([
            {"ts": "1700000000.000100", "author": "Alice", "text": "root"},
            {"ts": "1700000050.000200", "author": "bob", "text": "reply", "thread_ts": "1700000000.000100"}
        ]),
    )
    .unwrap();
    let channel: Vec<String> = store
        .with_conn(|c| messages::page(c, "C1", None, 50))
        .unwrap()
        .into_iter()
        .map(|r| r.ts)
        .collect();
    assert_eq!(channel, vec!["1700000000.000100"]);
    let thread = store
        .with_conn(|c| messages::thread(c, "C1", "1700000000.000100"))
        .unwrap();
    assert_eq!(thread.len(), 2);
}

#[test]
fn links_are_appended_once_and_unknown_author_kept_as_name() {
    let store = store();
    simple(
        &store,
        json!([
            {"ts": "1700000000.000100", "author": "Mallory Unknown",
             "text": "see https://example.com/a",
             "links": ["https://example.com/a", "https://example.com/b"]}
        ]),
    )
    .unwrap();
    let text = stored(&store, "1700000000.000100").unwrap().text;
    assert_eq!(text.matches("https://example.com/a").count(), 1);
    assert!(text.contains("https://example.com/b"));
    let author: Option<String> = store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT author_name FROM messages WHERE ts = '1700000000.000100'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(author.as_deref(), Some("Mallory Unknown"));
}

#[test]
fn unknown_channel_is_reported_not_guessed() {
    let store = store();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("no-such-channel.json");
    write(
        &file,
        &json!([{"ts": "1700000000.000100", "author": "Alice", "text": "x"}]),
    );
    let stats = import(&store, &file);
    if let Ok(stats) = stats {
        assert_eq!(stats.written, 0);
        assert_eq!(stats.unknown_channels, vec!["no-such-channel".to_string()]);
    }
    assert_eq!(count(&store, "SELECT COUNT(*) FROM messages"), 0);
}

#[test]
fn slack_export_with_a_broken_day_file_does_not_panic() {
    let store = store();
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("channels.json"),
        &json!([{"id": "C1", "name": "general"}]),
    );
    fs::create_dir_all(dir.path().join("general")).unwrap();
    fs::write(dir.path().join("general/2023-11-13.json"), b"[{oops").unwrap();
    write(
        &dir.path().join("general/2023-11-14.json"),
        &json!([{"type": "message", "ts": "1700000000.000100", "user": "U1", "text": "fine"},
                {"type": "message", "user": "U1", "text": "no ts"}]),
    );
    let _ = import(&store, dir.path());
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM messages WHERE ts IS NULL OR ts = ''"
        ),
        0
    );
}

#[test]
fn slack_export_does_not_clobber_newer_synced_edit() {
    let store = store();
    let mut synced = row("1700000000.000100", "edited later in Slack");
    synced.edited_ts = Some("1700000900.000000".into());
    upsert(&store, &[synced]);
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("channels.json"),
        &json!([{"id": "C1", "name": "general"}]),
    );
    write(
        &dir.path().join("general/2023-11-14.json"),
        &json!([{"type": "message", "ts": "1700000000.000100", "user": "U1", "text": "original",
                 "reactions": [{"name": "eyes", "users": ["U2"], "count": 1}]}]),
    );
    import(&store, dir.path()).unwrap();
    let s = stored(&store, "1700000000.000100").unwrap();
    assert_eq!(s.text, "edited later in Slack");
    assert_eq!(s.edited_ts.as_deref(), Some("1700000900.000000"));
}
