//! Reading the timeline page by page, and what the app gets from the bus.

use flek_core::store::directory;
use flek_core::store::messages::{self, MessageRow};
use flek_core::store::Store;
use flek_core::sync::live::{Change, ConnectionState, Progress};

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name, url, self_user_id) VALUES ('T1', 'Team', 'https://t.slack.com/', 'U0ME');
                 INSERT INTO users (id, team_id, name) VALUES ('U0ME', 'T1', 'me');
                 INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');",
            )?;
            let rows: Vec<MessageRow> = (1..=25)
                .map(|i| MessageRow {
                    ts: format!("{:03}.000000", i * 10),
                    user_id: Some("U0ME".into()),
                    text: format!("m{i}"),
                    raw_json: "{}".into(),
                    // Every fifth message is a thread reply: not in the timeline.
                    thread_ts: (i % 5 == 0).then(|| "010.000000".into()),
                    ..Default::default()
                })
                .collect();
            messages::upsert_messages(c, "C1", &rows)?;
            Ok(())
        })
        .unwrap();
    store
}

#[test]
fn pages_walk_back_without_gaps_or_replies() {
    let store = store();
    let mut seen = Vec::new();
    let mut before: Option<String> = None;
    loop {
        let page = store
            .with_read(|c| messages::page(c, "C1", before.as_deref(), 7))
            .unwrap();
        if page.is_empty() {
            break;
        }
        assert!(page.windows(2).all(|w| w[0].ts < w[1].ts), "oldest first");
        before = Some(page[0].ts.clone());
        seen.splice(0..0, page.into_iter().map(|r| r.ts));
    }
    assert_eq!(seen.len(), 20);
    assert!(seen.windows(2).all(|w| w[0] < w[1]));

    let one = store
        .with_read(|c| messages::message(c, "C1", "050.000000"))
        .unwrap()
        .unwrap();
    assert_eq!(one.thread_ts.as_deref(), Some("010.000000"));
    assert!(store
        .with_read(|c| messages::message(c, "C1", "999.000000"))
        .unwrap()
        .is_none());
}

#[test]
fn identity_is_stored_for_offline_start() {
    let store = store();
    let me = store
        .with_read(|c| directory::stored_identity(c, "T1"))
        .unwrap()
        .unwrap();
    assert_eq!((me.team.as_str(), me.user.as_str()), ("Team", "me"));
    assert_eq!(me.user_id, "U0ME");
    assert!(store
        .with_read(|c| directory::stored_identity(c, "T9"))
        .unwrap()
        .is_none());
}

#[test]
fn changes_serialize_for_the_ui() {
    let json = |c: Change| serde_json::to_value(c).unwrap();
    assert_eq!(
        json(Change::Message {
            conversation_id: "C1".into(),
            ts: "1.0".into()
        }),
        serde_json::json!({"type": "message", "data": {"conversation_id": "C1", "ts": "1.0"}})
    );
    assert_eq!(
        json(Change::Progress(Progress::Files { done: 1, total: 2 })),
        serde_json::json!({"type": "progress", "data": {"stage": "files", "done": 1, "total": 2}})
    );
    assert_eq!(
        json(Change::Connection(ConnectionState::Connected)),
        serde_json::json!({"type": "connection", "data": {"state": "connected"}})
    );
    assert_eq!(
        json(Change::Directory),
        serde_json::json!({"type": "directory"})
    );
}

#[test]
fn files_come_with_their_message() {
    let store = store();
    store
        .with_conn(|c| {
            c.execute_batch(
                r#"INSERT INTO files (id, name, mimetype, size, local_path, raw_json) VALUES
                       ('F1', 'cat.png', 'image/png', 10, '/cache/F1/cat.png', '{"original_w": 640, "original_h": 480}'),
                       ('F2', 'notes.pdf', 'application/pdf', 20, NULL, '{}');
                   INSERT INTO message_files VALUES ('C1', '020.000000', 'F1'), ('C1', '020.000000', 'F2');"#,
            )?;
            Ok(())
        })
        .unwrap();
    let page = store
        .with_read(|c| messages::page(c, "C1", None, 100))
        .unwrap();
    let with = page.iter().find(|m| m.ts == "020.000000").unwrap();
    let names: Vec<_> = with
        .files
        .iter()
        .map(|f| f.name.as_deref().unwrap())
        .collect();
    assert_eq!(names, ["cat.png", "notes.pdf"]);
    assert_eq!(
        (with.files[0].width, with.files[0].height),
        (Some(640), Some(480))
    );
    assert!(with.files[1].local_path.is_none());
    assert!(page
        .iter()
        .filter(|m| m.ts != "020.000000")
        .all(|m| m.files.is_empty()));
    assert_eq!(
        store
            .with_read(|c| messages::file_local_path(c, "F1"))
            .unwrap()
            .as_deref(),
        Some("/cache/F1/cat.png")
    );
    let people = store.with_read(|c| directory::people(c, "T1")).unwrap();
    assert_eq!(people.get("U0ME").map(String::as_str), Some("me"));
}
