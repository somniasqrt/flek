//! Files deleted in Slack: tombstones mark, never erase (fake ids only).

use flek_core::rtm::Event;
use flek_core::store::files::{self, FileRow};
use flek_core::store::messages::{self, MessageRow};
use flek_core::store::{migrations, Store};
use flek_core::sync::live::{apply, Applied, Change};
use serde_json::json;

const FULL: &str = r#"{"id":"F1","name":"notes.pdf","mimetype":"application/pdf","size":10,"url_private_download":"https://files.slack.com/F1/notes.pdf"}"#;
const TOMB: &str = r#"{"id":"F1","mode":"tombstone"}"#;

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name, self_user_id) VALUES ('T1', 'Team', 'U0ME');
                 INSERT INTO conversations (id, team_id, kind, name, label) VALUES ('C1', 'T1', 'public_channel', 'general', 'general');
                 INSERT INTO conversations (id, team_id, kind, name, label) VALUES ('C2', 'T1', 'public_channel', 'random', 'random');",
            )?;
            Ok(())
        })
        .unwrap();
    store
}

fn message(file: &str) -> MessageRow {
    let v: serde_json::Value = serde_json::from_str(file).unwrap();
    MessageRow {
        ts: "1700000000.000100".into(),
        text: "notes".into(),
        files: vec![FileRow::from_slack(&v).unwrap()],
        raw_json: json!({"ts": "1700000000.000100", "files": [v]}).to_string(),
        ..Default::default()
    }
}

fn row(store: &Store) -> (Option<String>, Option<String>, bool, Option<String>) {
    store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT name, url_private_download, deleted, local_path FROM files WHERE id = 'F1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap()
}

#[test]
fn tombstone_marks_a_stored_file_without_erasing_it() {
    let store = store();
    store
        .with_conn(|c| {
            messages::upsert_messages(c, "C1", &[message(FULL)])?;
            files::set_local_path(c, "F1", "/cache/F1/notes.pdf")?;
            messages::upsert_messages(c, "C1", &[message(TOMB)])
        })
        .unwrap();
    assert_eq!(
        row(&store),
        (
            Some("notes.pdf".into()),
            Some("https://files.slack.com/F1/notes.pdf".into()),
            true,
            Some("/cache/F1/notes.pdf".into())
        )
    );
    let pending = store.with_conn(|c| files::downloadable(c, None)).unwrap();
    assert!(pending.is_empty(), "a deleted file is never downloaded");
}

#[test]
fn deleted_is_sticky_even_when_a_full_version_arrives_later() {
    let store = store();
    store
        .with_conn(|c| {
            messages::upsert_messages(c, "C1", &[message(TOMB)])?;
            // An older archive import still has the full file.
            messages::upsert_messages(c, "C1", &[message(FULL)])
        })
        .unwrap();
    let (name, _, deleted, _) = row(&store);
    assert_eq!(name.as_deref(), Some("notes.pdf"), "metadata fills the gap");
    assert!(deleted, "Slack cannot restore a deleted file");
}

#[test]
fn views_report_deleted_files() {
    let store = store();
    store
        .with_conn(|c| messages::upsert_messages(c, "C1", &[message(TOMB)]))
        .unwrap();
    let listed = store
        .with_conn(|c| {
            flek_core::reads::files(
                c,
                "T1",
                &flek_core::reads::FileFilter {
                    limit: 10,
                    ..Default::default()
                },
            )
        })
        .unwrap();
    assert!(listed[0].file.deleted);
    let err = store
        .with_conn(|c| flek_core::reads::pending_file(c, "F1").map(|_| ()))
        .unwrap_err();
    assert!(err.to_string().contains("deleted in Slack"), "{err}");
}

#[tokio::test]
async fn file_deleted_event_refreshes_every_conversation_that_shows_it() {
    let store = store();
    store
        .with_conn(|c| {
            messages::upsert_messages(c, "C1", &[message(FULL)])?;
            messages::upsert_messages(c, "C2", &[message(FULL)])
        })
        .unwrap();
    let event = Event::parse(json!({"type": "file_deleted", "file_id": "F1", "event_ts": "1.0"}));
    assert!(matches!(&event, Event::FileDeleted { file_id } if file_id == "F1"));
    assert_eq!(
        apply(&store, event.clone()).await.unwrap(),
        Applied::ChangedMany(vec![
            Change::History {
                conversation_id: "C1".into()
            },
            Change::History {
                conversation_id: "C2".into()
            },
        ])
    );
    assert!(row(&store).2);
    assert_eq!(
        apply(&store, event).await.unwrap(),
        Applied::Ignored,
        "already deleted"
    );
    let unknown = Event::parse(json!({"type": "file_deleted", "file_id": "F404"}));
    assert_eq!(apply(&store, unknown).await.unwrap(), Applied::Ignored);
}

#[test]
fn migration_marks_files_deleted_before_it_existed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    {
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        migrations::migrate_to(&mut conn, 25).unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO teams (id, name, self_user_id) VALUES ('T1', 'Team', 'U0ME');
             INSERT INTO conversations (id, team_id, kind, name, label) VALUES ('C1', 'T1', 'public_channel', 'general', 'general');
             INSERT INTO messages (conversation_id, ts, text, raw_json)
                 VALUES ('C1', '1.000100', 'kept', '{{\"files\":[{TOMB}]}}');
             INSERT INTO files (id, name, raw_json) VALUES ('F1', 'notes.pdf', '{FULL}');
             INSERT INTO message_files (conversation_id, ts, file_id) VALUES ('C1', '1.000100', 'F1');
             INSERT INTO files (id, raw_json) VALUES ('F2', '{{\"id\":\"F2\",\"mode\":\"tombstone\"}}');
             INSERT INTO files (id, name, raw_json) VALUES ('F3', 'alive.txt', '{{\"id\":\"F3\"}}');"
        ))
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let deleted: Vec<(String, bool)> = store
        .with_conn(|c| {
            let mut stmt = c.prepare("SELECT id, deleted FROM files ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            Ok(rows)
        })
        .unwrap();
    assert_eq!(
        deleted,
        vec![
            ("F1".into(), true),
            ("F2".into(), true),
            ("F3".into(), false)
        ]
    );
}
