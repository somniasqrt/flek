//! One bad item, one failed write or one busy directory must not stop or
//! misreport everything else (M9.13.2, M9.13.4, M9.14.3–M9.14.7).
//! Temp stores and a mock Slack only.

use std::time::Duration;

use flek_core::api::{AuthTest, Client};
use flek_core::rtm::Event;
use flek_core::store::messages::{self, MessageRow, SyncState};
use flek_core::store::{directory, Store};
use flek_core::sync::backfill::{backfill_conversation, RateLimiter};
use flek_core::sync::directory::DirectoryRefresh;
use flek_core::sync::import::import_path;
use flek_core::sync::live::{apply, Applied};
use flek_core::sync::send::{send, Outgoing};
use flek_core::sync::threads::sync_thread;
use flek_core::sync::SyncOptions;
use flek_core::{Error, Secret};
use serde_json::{json, Value};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing()
}

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U0ME")?;
            c.execute_batch(
                "INSERT INTO conversations (id, team_id, kind, name, label)
                 VALUES ('C1', 'T1', 'public_channel', 'general', 'general');",
            )?;
            Ok(())
        })
        .unwrap();
    store
}

fn ago(seconds: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("{}.000001", now - seconds)
}

fn saved(ts: &str, thread: Option<&str>, text: &str) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        thread_ts: thread.map(str::to_owned),
        text: text.into(),
        raw_json: "{}".into(),
        ..Default::default()
    }
}

fn seed(store: &Store, rows: &[MessageRow]) {
    let (oldest, newest) = (
        rows.iter().map(|r| r.ts.clone()).min(),
        rows.iter().map(|r| r.ts.clone()).max(),
    );
    store
        .with_conn(|c| {
            messages::upsert_messages(c, "C1", rows)?;
            messages::set_sync_state(
                c,
                "C1",
                &SyncState {
                    oldest_ts: oldest,
                    newest_ts: newest,
                    complete: true,
                },
            )
        })
        .unwrap();
}

fn stored(store: &Store) -> Vec<(String, String, bool)> {
    store
        .with_read(|c| {
            let mut stmt = c.prepare("SELECT ts, text, deleted FROM messages ORDER BY ts")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .unwrap()
}

async fn history(server: &MockServer, messages: Value) {
    Mock::given(path("/conversations.history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "messages": messages, "has_more": false
        })))
        .mount(server)
        .await;
}

async fn refresh(client: &Client, store: &Store) -> flek_core::Result<()> {
    backfill_conversation(
        client,
        store,
        &mut RateLimiter::new(Duration::ZERO),
        "C1",
        &SyncOptions::refresh_days(7),
    )
    .await
    .map(|_| ())
}

#[tokio::test]
async fn an_unreadable_message_is_kept_and_its_neighbours_still_sync() {
    // M9.14.3: before, this page failed forever and the conversation stuck.
    let server = MockServer::start().await;
    let (broken, fresh, gone) = (ago(7200), ago(60), ago(3600));
    history(
        &server,
        json!([
            {"ts": fresh, "user": "U1", "text": "new"},
            {"ts": broken, "user": "U1", "text": 42}
        ]),
    )
    .await;
    let store = store();
    seed(
        &store,
        &[
            saved(&broken, None, "saved before Slack broke it"),
            saved(&gone, None, "really deleted"),
        ],
    );

    refresh(&client(&server), &store).await.unwrap();

    assert_eq!(
        stored(&store),
        [
            (broken, "saved before Slack broke it".to_owned(), false),
            // Absence is still evidence when every identity on the page is known.
            (gone, "really deleted".to_owned(), true),
            (fresh.clone(), "new".to_owned(), false),
        ]
    );
    let state = store.with_read(|c| messages::sync_state(c, "C1")).unwrap();
    assert_eq!(state.newest_ts.as_deref(), Some(fresh.as_str()));
}

#[tokio::test]
async fn an_item_without_identity_disables_deletion_for_the_window() {
    let server = MockServer::start().await;
    let (kept, fresh) = (ago(3600), ago(60));
    history(
        &server,
        json!([
            {"ts": fresh, "user": "U1", "text": "new"},
            {"user": "U1", "text": "which message is this?"},
            {"ts": {"not": "a ts"}, "text": "nor this"}
        ]),
    )
    .await;
    let store = store();
    seed(&store, &[saved(&kept, None, "may be the unreadable one")]);

    refresh(&client(&server), &store).await.unwrap();

    assert_eq!(
        stored(&store),
        [
            (kept, "may be the unreadable one".to_owned(), false),
            (fresh, "new".to_owned(), false),
        ]
    );
}

#[tokio::test]
async fn thread_sync_survives_an_unreadable_reply() {
    let server = MockServer::start().await;
    let (root, broken, gone, fresh) = (ago(7200), ago(3600), ago(1800), ago(60));
    Mock::given(path("/conversations.replies"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "has_more": false, "messages": [
                {"ts": root, "thread_ts": root, "text": "root"},
                {"ts": broken, "thread_ts": root, "text": 42},
                {"ts": fresh, "thread_ts": root, "text": "new reply"}
            ]
        })))
        .mount(&server)
        .await;
    let store = store();
    seed(
        &store,
        &[
            saved(&root, Some(&root), "root"),
            saved(&broken, Some(&root), "saved reply"),
            saved(&gone, Some(&root), "deleted reply"),
        ],
    );

    sync_thread(
        &client(&server),
        &store,
        &mut RateLimiter::new(Duration::ZERO),
        "C1",
        &root,
    )
    .await
    .unwrap();

    assert_eq!(
        stored(&store),
        [
            (root, "root".to_owned(), false),
            (broken, "saved reply".to_owned(), false),
            (gone, "deleted reply".to_owned(), true),
            (fresh, "new reply".to_owned(), false),
        ]
    );
}

#[tokio::test]
async fn a_thread_with_an_unidentifiable_reply_deletes_nothing() {
    let server = MockServer::start().await;
    let (root, kept) = (ago(7200), ago(3600));
    Mock::given(path("/conversations.replies"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "has_more": false, "messages": [
                {"ts": root, "thread_ts": root, "text": "root"},
                {"thread_ts": root, "text": []}
            ]
        })))
        .mount(&server)
        .await;
    let store = store();
    seed(
        &store,
        &[
            saved(&root, Some(&root), "root"),
            saved(&kept, Some(&root), "saved reply"),
        ],
    );
    sync_thread(
        &client(&server),
        &store,
        &mut RateLimiter::new(Duration::ZERO),
        "C1",
        &root,
    )
    .await
    .unwrap();
    assert!(stored(&store).iter().all(|(_, _, deleted)| !deleted));
}

#[tokio::test]
async fn a_message_slack_accepted_is_a_success_even_if_the_database_refuses_it() {
    // M9.14.5: reporting failure here invites a retry and a duplicate.
    let server = MockServer::start().await;
    Mock::given(path("/chat.postMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "channel": "C1", "ts": "1700000001.000200",
            "message": {"type": "message", "user": "U0ME", "text": "hi", "ts": "1700000001.000200"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let store = store();
    store
        .with_conn(|c| {
            c.execute_batch(
                "CREATE TRIGGER refuse BEFORE INSERT ON messages
                 BEGIN SELECT RAISE(ABORT, 'synthetic disk failure'); END;",
            )?;
            Ok(())
        })
        .unwrap();
    let out = Outgoing {
        conversation_id: "C1".into(),
        text: "hi".into(),
        ..Default::default()
    };
    let sent = send(&client(&server), &store, "U0ME", out).await.unwrap();
    assert_eq!(sent.ts, "1700000001.000200");
    assert!(sent.not_stored_locally);
    assert!(sent.change.is_none());
    assert!(stored(&store).is_empty());
    let wire = serde_json::to_value(&sent).unwrap();
    assert_eq!(wire["not_stored_locally"], true);
}

#[tokio::test]
async fn a_stored_message_is_not_flagged() {
    let server = MockServer::start().await;
    Mock::given(path("/chat.postMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "channel": "C1", "ts": "1700000001.000200",
            "message": {"type": "message", "user": "U0ME", "text": "hi", "ts": "1700000001.000200"}
        })))
        .mount(&server)
        .await;
    let store = store();
    let out = Outgoing {
        conversation_id: "C1".into(),
        text: "hi".into(),
        ..Default::default()
    };
    let sent = send(&client(&server), &store, "U0ME", out).await.unwrap();
    assert!(!sent.not_stored_locally);
    assert!(sent.change.is_some());
    assert_eq!(stored(&store).len(), 1);
}

#[tokio::test]
async fn a_committed_write_is_not_reported_as_failed_by_permission_repair() {
    // M9.13.4: the chmod after a write used to replace the write's own result.
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("flek.db");
    let store = Store::open(&db).unwrap();
    let moved = dir.path().join("moved.db");
    let (from, to) = (db.clone(), moved.clone());
    let value = store
        .call(move |c| {
            c.execute("INSERT INTO teams (id, name) VALUES ('T1', 'Fake')", [])?;
            std::fs::rename(&from, &to).unwrap();
            Ok(7)
        })
        .await
        .unwrap();
    assert_eq!(value, 7);
    let again = store
        .with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM teams", [], |r| r.get::<_, i64>(0))?));
    assert_eq!(again.unwrap(), 1);
    // The operation's own error is still what the caller sees.
    let error = store
        .with_conn(|c| Ok(c.execute("INSERT INTO no_such_table VALUES (1)", [])?))
        .unwrap_err();
    assert!(matches!(error, Error::Sqlite(_)));
}

#[tokio::test]
async fn a_user_change_without_a_team_never_creates_an_empty_team() {
    // M9.14.4
    let store = store();
    let event = Event::parse(json!({
        "type": "user_change",
        "user": {"id": "U0NEW", "name": "new", "profile": {"display_name": "New"}}
    }));
    assert!(matches!(event, Event::UserChanged { .. }));
    assert_eq!(apply(&store, event).await.unwrap(), Applied::NeedsDirectory);
    let event = Event::parse(json!({
        "type": "user_change",
        "user": {"id": "U0NEW2", "team_id": "", "name": "new", "profile": {}}
    }));
    assert_eq!(apply(&store, event).await.unwrap(), Applied::NeedsDirectory);
    let teams: Vec<String> = store
        .with_read(|c| {
            let mut stmt = c.prepare("SELECT id FROM teams")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .unwrap();
    assert_eq!(teams, ["T1"]);

    // A known user keeps their team.
    store
        .with_conn(|c| {
            c.execute(
                "INSERT INTO users (id, team_id, name) VALUES ('U0OLD', 'T1', 'old')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let event = Event::parse(json!({
        "type": "user_change",
        "user": {"id": "U0OLD", "name": "renamed", "profile": {}}
    }));
    assert!(matches!(
        apply(&store, event).await.unwrap(),
        Applied::Changed(_)
    ));
}

#[test]
fn users_with_an_empty_team_id_fall_back_to_the_workspace() {
    let store = store();
    let users: Vec<flek_core::api::User> = serde_json::from_value(json!([
        {"id": "U1", "team_id": "", "name": "a", "profile": {}},
        {"id": "U2", "name": "b", "profile": {}}
    ]))
    .unwrap();
    store
        .with_conn(|c| directory::upsert_users(c, "T1", &users))
        .unwrap();
    let rows: Vec<(String, String)> = store
        .with_read(|c| {
            let mut stmt = c.prepare("SELECT id, team_id FROM users ORDER BY id")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .unwrap();
    assert_eq!(
        rows,
        [("U1".into(), "T1".into()), ("U2".into(), "T1".into())]
    );
    let empty: i64 = store
        .with_read(|c| {
            Ok(c.query_row("SELECT COUNT(*) FROM teams WHERE id = ''", [], |r| r.get(0))?)
        })
        .unwrap();
    assert_eq!(empty, 0);
}

#[test]
fn a_simple_archive_is_never_imported_into_a_prefix_match() {
    // M9.14.6: `dev.json` used to land in `dev-backend`.
    let store = store();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO conversations (id, team_id, kind, name, label)
                 VALUES ('C2', 'T1', 'public_channel', 'dev-backend', 'dev-backend');",
            )?;
            Ok(())
        })
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dev.json");
    std::fs::write(
        &file,
        json!([{"ts": "1600000000.000100", "author": "alice", "text": "archived"}]).to_string(),
    )
    .unwrap();

    let outcome = store.with_conn(|c| import_path(c, "T1", &file, None));
    match outcome {
        Err(Error::Ambiguous { candidates }) => {
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].id, "C2");
        }
        other => panic!("expected the candidates, got {other:?}"),
    }
    assert!(stored(&store).is_empty());

    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO conversations (id, team_id, kind, name, label)
                 VALUES ('C3', 'T1', 'public_channel', 'dev', 'dev');",
            )?;
            Ok(())
        })
        .unwrap();
    let stats = store
        .with_conn(|c| import_path(c, "T1", &file, None))
        .unwrap();
    assert_eq!(stats.written, 1);
    let conversation: String = store
        .with_read(|c| Ok(c.query_row("SELECT conversation_id FROM messages", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(conversation, "C3");
}

fn me() -> AuthTest {
    AuthTest {
        url: "https://fake.slack.com/".into(),
        team: "Fake".into(),
        user: "me".into(),
        team_id: "T1".into(),
        user_id: "U0ME".into(),
        enterprise_id: None,
    }
}

async fn conversations(server: &MockServer, channels: Value) {
    Mock::given(path("/users.conversations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "channels": channels, "response_metadata": {"next_cursor": ""}
        })))
        .mount(server)
        .await;
}

fn users_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "ok": true,
        "members": [{"id": "U0ME", "team_id": "T1", "name": "me", "profile": {}}],
        "response_metadata": {"next_cursor": ""}
    }))
}

async fn calls(server: &MockServer, method: &str) -> usize {
    let wanted = format!("/{method}");
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == wanted)
        .count()
}

#[tokio::test]
async fn reconnects_reuse_the_people_list_but_always_reload_conversations() {
    // M9.14.7
    let server = MockServer::start().await;
    Mock::given(path("/users.list"))
        .respond_with(users_ok())
        .mount(&server)
        .await;
    conversations(
        &server,
        json!([{"id": "C1", "name": "general", "is_channel": true}]),
    )
    .await;
    let (client, store, me) = (client(&server), store(), me());
    let directory = DirectoryRefresh::default();

    for _ in 0..3 {
        directory
            .refresh(&client, &store, &me, false)
            .await
            .unwrap();
    }
    assert_eq!(calls(&server, "users.list").await, 1);
    assert_eq!(calls(&server, "users.conversations").await, 3);
}

#[tokio::test]
async fn a_new_dm_is_resolved_at_once_without_the_people_list() {
    let server = MockServer::start().await;
    Mock::given(path("/users.list"))
        .respond_with(users_ok())
        .mount(&server)
        .await;
    conversations(
        &server,
        json!([
            {"id": "C1", "name": "general", "is_channel": true},
            {"id": "D0NEW", "is_im": true, "user": "U0ME"}
        ]),
    )
    .await;
    let (client, store, me) = (client(&server), store(), me());
    let directory = DirectoryRefresh::default();
    directory
        .refresh(&client, &store, &me, false)
        .await
        .unwrap();
    store
        .with_conn(|c| Ok(c.execute("DELETE FROM conversations WHERE id = 'D0NEW'", [])?))
        .unwrap();

    let message = || {
        Event::parse(json!({
            "type": "message", "channel": "D0NEW", "user": "U0ME",
            "text": "hello", "ts": "1700000002.000100"
        }))
    };
    assert_eq!(
        apply(&store, message()).await.unwrap(),
        Applied::NeedsDirectory
    );
    let started = std::time::Instant::now();
    assert!(directory.refresh(&client, &store, &me, true).await.unwrap());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "a live refresh must not wait for the people-list interval"
    );
    assert_eq!(calls(&server, "users.list").await, 1);
    assert!(matches!(
        apply(&store, message()).await.unwrap(),
        Applied::Changed(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn live_refreshes_are_spaced_out() {
    // No network here: an unreachable base URL fails fast, the spacing still applies.
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        "http://127.0.0.1:9",
    )
    .unwrap()
    .without_pacing();
    let (store, me) = (store(), me());
    let directory = DirectoryRefresh::default();
    let before = tokio::time::Instant::now();
    let _ = directory.refresh(&client, &store, &me, true).await;
    let _ = directory.refresh(&client, &store, &me, true).await;
    assert!(before.elapsed() >= Duration::from_secs(10));
}

#[tokio::test]
async fn a_failed_people_list_does_not_block_the_next_pass() {
    let server = MockServer::start().await;
    Mock::given(path("/users.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": false, "error": "fatal_error"
        })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/users.list"))
        .respond_with(users_ok())
        .with_priority(2)
        .mount(&server)
        .await;
    conversations(
        &server,
        json!([{"id": "C1", "name": "general", "is_channel": true}]),
    )
    .await;
    let (client, store, me) = (client(&server), store(), me());
    let directory = DirectoryRefresh::default();

    assert!(directory
        .refresh(&client, &store, &me, false)
        .await
        .is_err());
    directory
        .refresh(&client, &store, &me, false)
        .await
        .unwrap();
    assert_eq!(calls(&server, "users.list").await, 2);
    // Now it is cached.
    directory
        .refresh(&client, &store, &me, false)
        .await
        .unwrap();
    assert_eq!(calls(&server, "users.list").await, 2);
}
