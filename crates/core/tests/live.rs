//! Live events → database.

use flek_core::rtm::Event;
use flek_core::store::messages::{self, SyncState};
use flek_core::store::{directory, Store};
use flek_core::sync::live::{apply, Applied, Change};
use serde_json::json;

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name, self_user_id) VALUES ('T1', 'Team', 'U0ME');
                 INSERT INTO conversations (id, team_id, kind, name, label) VALUES ('C1', 'T1', 'public_channel', 'general', 'general');",
            )?;
            messages::set_sync_state(c, "C1", &SyncState { oldest_ts: Some("1.000000".into()), newest_ts: Some("10.000000".into()), complete: true })?;
            c.execute("INSERT INTO thread_sync VALUES ('C1', '10.000000', '10.000000', 0)", [])?;
            Ok(())
        })
        .unwrap();
    store
}

async fn ev(store: &Store, v: serde_json::Value) -> Applied {
    apply(store, Event::parse(v)).await.unwrap()
}

fn text(store: &Store, ts: &str) -> Option<String> {
    store
        .with_conn(|c| {
            Ok(messages::thread(c, "C1", ts)?
                .into_iter()
                .find(|r| r.ts == ts)
                .map(|r| r.text))
        })
        .unwrap()
}

#[tokio::test]
async fn message_lifecycle() {
    let store = store();
    let a = ev(&store, json!({"type": "message", "channel": "C1", "ts": "20.000000", "user": "U1", "text": "hello"})).await;
    assert_eq!(
        a,
        Applied::Changed(Change::Message {
            conversation_id: "C1".into(),
            ts: "20.000000".into()
        })
    );
    assert_eq!(text(&store, "20.000000").as_deref(), Some("hello"));
    let newest = store
        .with_conn(|c| messages::sync_state(c, "C1"))
        .unwrap()
        .newest_ts;
    assert_eq!(newest.as_deref(), Some("20.000000"));

    ev(&store, json!({"type": "message", "subtype": "message_changed", "channel": "C1", "ts": "21.000000",
                      "message": {"ts": "20.000000", "user": "U1", "text": "hello!", "edited": {"ts": "21.000000"}}})).await;
    assert_eq!(text(&store, "20.000000").as_deref(), Some("hello!"));

    let r = ev(&store, json!({"type": "reaction_added", "user": "U2", "reaction": "wave", "item": {"type": "message", "channel": "C1", "ts": "20.000000"}})).await;
    assert!(matches!(r, Applied::Changed(Change::Reactions { .. })));
    assert_eq!(
        store
            .with_conn(|c| messages::reactions(c, "C1", "20.000000"))
            .unwrap(),
        [("wave".to_string(), 1)]
    );
    ev(&store, json!({"type": "reaction_removed", "user": "U2", "reaction": "wave", "item": {"type": "message", "channel": "C1", "ts": "20.000000"}})).await;
    assert!(store
        .with_conn(|c| messages::reactions(c, "C1", "20.000000"))
        .unwrap()
        .is_empty());

    ev(&store, json!({"type": "message", "subtype": "message_deleted", "channel": "C1", "ts": "22.000000", "deleted_ts": "20.000000"})).await;
    assert_eq!(
        store
            .with_conn(|c| messages::recent(c, "C1", 10))
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn thread_reply_advances_thread_marker_not_history() {
    let store = store();
    ev(&store, json!({"type": "message", "channel": "C1", "ts": "15.000000", "thread_ts": "10.000000", "user": "U1", "text": "reply"})).await;
    let (newest, thread): (Option<String>, String) = store
        .with_conn(|c| {
            Ok((
                messages::sync_state(c, "C1")?.newest_ts,
                c.query_row(
                    "SELECT latest_reply FROM thread_sync WHERE thread_ts = '10.000000'",
                    [],
                    |r| r.get(0),
                )?,
            ))
        })
        .unwrap();
    assert_eq!(newest.as_deref(), Some("10.000000"));
    assert_eq!(thread, "15.000000");
}

#[tokio::test]
async fn unknown_conversation_and_directory_events() {
    let store = store();
    assert_eq!(
        ev(
            &store,
            json!({"type": "message", "channel": "D9", "ts": "1.000000", "text": "new dm"})
        )
        .await,
        Applied::NeedsDirectory
    );
    assert_eq!(
        ev(
            &store,
            json!({"type": "channel_rename", "channel": {"id": "C1", "name": "x"}})
        )
        .await,
        Applied::DirectoryStale
    );
    assert_eq!(
        ev(&store, json!({"type": "reaction_added", "user": "U2", "reaction": "x", "item": {"type": "message", "channel": "C1", "ts": "999.000000"}})).await,
        Applied::Ignored
    );
}

#[tokio::test]
async fn read_markers_and_users() {
    let store = store();
    ev(
        &store,
        json!({"type": "channel_marked", "channel": "C1", "ts": "20.000000"}),
    )
    .await;
    let last_read: String = store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT last_read FROM conversations WHERE id = 'C1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(last_read, "20.000000");

    ev(&store, json!({"type": "user_change", "user": {"id": "U1", "team_id": "T1", "name": "alice", "profile": {"display_name": "Alice B."}}})).await;
    assert_eq!(
        store
            .with_conn(|c| directory::user_label(c, "U1"))
            .unwrap()
            .as_deref(),
        Some("Alice B.")
    );
}
