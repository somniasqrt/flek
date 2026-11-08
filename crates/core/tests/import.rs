//! Archive import.

use std::fs;

use flek_core::store::messages::{self, MessageRow};
use flek_core::store::Store;
use flek_core::sync::import::import_path;
use serde_json::json;

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
                 INSERT INTO users (id, team_id, name, display_name) VALUES ('U1', 'T1', 'alice', 'Alice');
                 INSERT INTO conversations (id, team_id, kind, name, label) VALUES ('C1', 'T1', 'public_channel', 'general', 'general');",
            )?;
            Ok(())
        })
        .unwrap();
    store
}

fn write(path: &std::path::Path, v: serde_json::Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec(&v).unwrap()).unwrap();
}

#[test]
fn simple_format_fills_gaps_and_is_idempotent() {
    let store = store();
    // Already stored: one live message and one Slack now hides.
    store
        .with_conn(|c| {
            messages::upsert_messages(
                c,
                "C1",
                &[
                    MessageRow {
                        ts: "1.000000".into(),
                        text: "hidden".into(),
                        hidden_by_limit: true,
                        raw_json: "{}".into(),
                        ..Default::default()
                    },
                    MessageRow {
                        ts: "2.000000".into(),
                        text: "live text".into(),
                        raw_json: "{}".into(),
                        ..Default::default()
                    },
                ],
            )
        })
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("general.json"),
        json!([
            {"ts": "1.000000", "author": "alice", "text": "recovered", "links": ["https://x.example"], "files": [{"name": "a.pdf", "url": "https://f.example/a.pdf"}]},
            {"ts": "2.000000", "author": "Alice", "text": "archive text"},
            {"ts": "3.0", "author": "Ghost", "text": "see https://y.example", "links": ["https://y.example"], "thread_ts": "1.000000"}
        ]),
    );
    write(
        &dir.path().join("nowhere.json"),
        json!([{"ts": "9.000000", "text": "?"}]),
    );

    let stats = store
        .with_conn(|c| import_path(c, "T1", dir.path(), None))
        .unwrap();
    assert_eq!((stats.format, stats.read, stats.written), ("simple", 3, 2));
    assert_eq!(stats.unknown_channels, ["nowhere"]);

    let rows = store
        .with_conn(|c| messages::thread(c, "C1", "1.000000"))
        .unwrap();
    assert_eq!(
        rows[0].text,
        "recovered\nhttps://x.example\n[file] a.pdf https://f.example/a.pdf"
    );
    assert_eq!(rows[0].author.as_deref(), Some("Alice"));
    assert!(!rows[0].hidden_by_limit);
    assert_eq!(rows[1].ts, "3.000000");
    assert_eq!(rows[1].text, "see https://y.example");
    assert_eq!(rows[1].author.as_deref(), Some("Ghost"));
    let live = store.with_conn(|c| messages::recent(c, "C1", 10)).unwrap();
    assert_eq!(
        live.iter().find(|r| r.ts == "2.000000").unwrap().text,
        "live text"
    );

    let again = store
        .with_conn(|c| import_path(c, "T1", dir.path(), None))
        .unwrap();
    assert_eq!(again.written, 0);
}

#[test]
fn channel_override_for_a_single_file() {
    let store = store();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("export-2023.json");
    write(&file, json!([{"ts": "5.000000", "text": "hi"}]));
    let stats = store
        .with_conn(|c| import_path(c, "T1", &file, Some("#general")))
        .unwrap();
    assert_eq!(stats.written, 1);
}

#[test]
fn slack_export_format() {
    let store = store();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("users.json"),
        json!([
            {"id": "U1", "name": "alice-renamed", "profile": {"display_name": "Should not override"}},
            {"id": "U9", "team_id": "T1", "name": "old", "deleted": true, "profile": {"display_name": "Old Timer"}}
        ]),
    );
    write(
        &root.join("channels.json"),
        json!([
            {"id": "C1", "name": "general"},
            {"id": "C2", "name": "left-channel", "is_archived": true, "topic": {"value": "gone"}}
        ]),
    );
    write(
        &root.join("dms.json"),
        json!([{"id": "D1", "members": ["U9", "U1"]}]),
    );
    write(
        &root.join("general/2023-05-01.json"),
        json!([
            {"type": "message", "ts": "1683000000.000100", "user": "U1", "text": "hello",
             "reactions": [{"name": "wave", "users": ["U9"], "count": 1}],
             "files": [{"id": "F1", "name": "a.png", "url_private_download": "https://files.slack.com/a.png"}]},
            {"type": "message", "ts": "1683000001.000100", "user": "U9", "text": "reply", "thread_ts": "1683000000.000100"}
        ]),
    );
    write(
        &root.join("left-channel/2022-01-01.json"),
        json!([{"type": "message", "ts": "1641000000.000100", "user": "U9", "text": "old"}]),
    );
    write(
        &root.join("D1/2022-02-02.json"),
        json!([{"type": "message", "ts": "1643000000.000100", "user": "U9", "text": "dm"}]),
    );

    let stats = store
        .with_conn(|c| import_path(c, "T1", root, None))
        .unwrap();
    assert_eq!(
        (stats.format, stats.read, stats.written),
        ("slack-export", 4, 4)
    );

    store
        .with_conn(|c| {
            let label: String =
                c.query_row("SELECT display_name FROM users WHERE id = 'U1'", [], |r| {
                    r.get(0)
                })?;
            assert_eq!(label, "Alice");
            let left: (String, bool) = c.query_row(
                "SELECT topic, is_archived FROM conversations WHERE id = 'C2'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(left, ("gone".into(), true));
            let dm_label: String =
                c.query_row("SELECT label FROM conversations WHERE id = 'D1'", [], |r| {
                    r.get(0)
                })?;
            assert_eq!(dm_label, "Old Timer");
            assert_eq!(
                messages::reactions(c, "C1", "1683000000.000100")?,
                [("wave".to_string(), 1)]
            );
            assert_eq!(messages::thread(c, "C1", "1683000000.000100")?.len(), 2);
            let files: i64 = c.query_row("SELECT COUNT(*) FROM message_files", [], |r| r.get(0))?;
            assert_eq!(files, 1);
            Ok(())
        })
        .unwrap();

    let again = store
        .with_conn(|c| import_path(c, "T1", root, None))
        .unwrap();
    assert_eq!(again.written, 0);
}
