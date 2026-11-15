//! Event journal (triggers, retention), replay-to-live handoff, filters and
//! `wait`. Temp/in-memory SQLite, wiremock and a scripted websocket only.
#![allow(clippy::cloned_ref_to_slice_refs)]
mod common;

use flek_core::api::AuthTest;
use flek_core::store::messages::{self, MessageRow};
use flek_core::store::Store;
use flek_core::sync::events::{Filter, ReplyTarget, Stream};
use flek_core::sync::follow::FollowConfig;
use flek_core::Error;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message as Frame;
use wiremock::{matchers::path, Mock, MockServer, Request, ResponseTemplate};

const ROOT: &str = "1700000000.000100";

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name, self_user_id) VALUES ('T1', 'Team', 'U0ME');
                 INSERT INTO conversations (id, team_id, kind, name, label) VALUES
                    ('C1', 'T1', 'public_channel', 'general', 'general'),
                    ('C2', 'T1', 'public_channel', 'random', 'random'),
                    ('D1', 'T1', 'im', NULL, 'Bob');",
            )?;
            Ok(())
        })
        .unwrap();
    store
}

fn msg(ts: &str, user: &str, text: &str) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        user_id: Some(user.into()),
        text: text.into(),
        raw_json: "{}".into(),
        ..Default::default()
    }
}

fn reply(ts: &str, user: &str, text: &str) -> MessageRow {
    MessageRow {
        thread_ts: Some(ROOT.into()),
        ..msg(ts, user, text)
    }
}

fn put(store: &Store, conversation: &str, rows: &[MessageRow]) {
    store
        .with_conn(|c| messages::upsert_messages(c, conversation, rows))
        .unwrap();
}

fn kinds(store: &Store) -> Vec<String> {
    store
        .with_conn(|c| {
            let mut stmt = c.prepare("SELECT kind FROM event_log ORDER BY id")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<Result<Vec<String>, _>>()?)
        })
        .unwrap()
}

async fn drain(stream: &mut Stream) -> Vec<Value> {
    let mut out = Vec::new();
    loop {
        let batch = stream.poll().await.unwrap();
        if batch.is_empty() {
            return out;
        }
        out.extend(batch.iter().map(|e| serde_json::to_value(e).unwrap()));
    }
}

async fn replayed(stream: &mut Stream) -> Vec<Value> {
    let mut out = Vec::new();
    while let Some(batch) = stream.replay().await.unwrap() {
        out.extend(batch.iter().map(|e| serde_json::to_value(e).unwrap()));
    }
    out
}

fn texts(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                e["type"].as_str().unwrap(),
                e["message"]["text"].as_str().unwrap()
            )
        })
        .collect()
}

async fn open(store: &Store, filter: Filter) -> Stream {
    Stream::open(store.clone(), "T1".into(), filter)
        .await
        .unwrap()
}

#[test]
fn journal_records_each_change_once_and_ignores_unchanged_rewrites() {
    let store = store();
    let hello = msg("1700000001.000100", "U1", "hello");
    put(&store, "C1", &[hello.clone()]);
    put(&store, "C1", &[hello.clone()]);
    assert_eq!(kinds(&store), ["message"]);

    let edited = MessageRow {
        text: "hello!".into(),
        edited_ts: Some("1700000002.000100".into()),
        ..hello.clone()
    };
    put(&store, "C1", &[edited.clone()]);
    put(&store, "C1", &[edited.clone()]);
    // A stale, unedited version never replaces the edit and leaves no event.
    put(&store, "C1", &[hello.clone()]);
    assert_eq!(kinds(&store), ["message", "message_changed"]);

    let reacted = MessageRow {
        reactions: vec![("wave".into(), "U2".into())],
        ..edited.clone()
    };
    put(&store, "C1", &[reacted.clone()]);
    put(&store, "C1", &[reacted]);
    put(&store, "C1", &[edited.clone()]); // sync carries the full set: removed
    put(&store, "C1", &[edited]);
    assert_eq!(
        kinds(&store),
        [
            "message",
            "message_changed",
            "reaction_added",
            "reaction_removed"
        ]
    );

    store
        .with_conn(|c| messages::mark_deleted(c, "C1", &["1700000001.000100".to_string()]))
        .unwrap();
    store
        .with_conn(|c| messages::mark_deleted(c, "C1", &["1700000001.000100".to_string()]))
        .unwrap();
    assert_eq!(kinds(&store).last().unwrap(), "message_deleted");
    assert_eq!(kinds(&store).len(), 5);

    let (actor, reaction): (String, String) = store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT actor_id, reaction FROM event_log WHERE kind = 'reaction_added'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!((actor.as_str(), reaction.as_str()), ("U2", "wave"));
}

#[test]
fn journal_is_bounded() {
    let store = store();
    let total: i64 = store
        .with_conn(|c| {
            c.execute(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 50200)
                 INSERT INTO event_log(team_id, conversation_id, ts, event_ts, kind, message_json)
                 SELECT 'T1', 'C1', '1.000000', '1.000000', 'message', '{}' FROM n",
                [],
            )?;
            Ok(c.query_row("SELECT COUNT(*) FROM event_log", [], |r| r.get(0))?)
        })
        .unwrap();
    assert_eq!(total, 50_000);
    let (min, max): (i64, i64) = store
        .with_conn(|c| {
            Ok(
                c.query_row("SELECT MIN(id), MAX(id) FROM event_log", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?,
            )
        })
        .unwrap();
    // Ids are never reused: the oldest rows went, the newest stayed.
    assert_eq!((min, max), (200 + 1, 50_200));
}

#[tokio::test]
async fn replay_then_live_has_no_gap_and_no_duplicate() {
    let store = store();
    put(&store, "C1", &[msg("1700000001.000100", "U1", "one")]);
    put(&store, "C1", &[msg("1700000002.000100", "U1", "two")]);
    let mut stream = open(
        &store,
        Filter {
            since: Some("1700000001.500000".into()),
            ..Default::default()
        },
    )
    .await;
    // Committed after the boundary was captured, before replay started.
    put(&store, "C1", &[msg("1700000003.000100", "U1", "three")]);
    let replay = replayed(&mut stream).await;
    assert_eq!(texts(&replay), ["message:two"]);
    // Finished replay: the next read starts exactly after the boundary.
    assert!(stream.replay().await.unwrap().is_none());
    put(&store, "C1", &[msg("1700000004.000100", "U1", "four")]);
    let live = drain(&mut stream).await;
    assert_eq!(texts(&live), ["message:three", "message:four"]);
    assert!(drain(&mut stream).await.is_empty());
    // Every event id is distinct and increasing across both phases.
    let ids: Vec<u64> = replay
        .iter()
        .chain(&live)
        .map(|e| e["event_id"].as_str().unwrap().parse().unwrap())
        .collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
}

#[tokio::test]
async fn without_since_history_is_not_replayed_and_edits_follow_the_version() {
    let store = store();
    put(&store, "C1", &[msg("1700000001.000100", "U1", "old")]);
    let mut stream = open(&store, Filter::default()).await;
    assert!(stream.replay().await.unwrap().is_none());
    let edited = MessageRow {
        text: "new".into(),
        edited_ts: Some("1700000009.000100".into()),
        ..msg("1700000001.000100", "U1", "old")
    };
    put(&store, "C1", &[edited]);
    put(&store, "C1", &[msg("1700000005.000100", "U1", "later")]);
    let events = drain(&mut stream).await;
    assert_eq!(texts(&events), ["message_changed:new", "message:later"]);
    assert_eq!(events[0]["schema_version"], Value::Null); // added by the CLI emitter
    assert_eq!(events[0]["message"]["edited"], true);
    assert_eq!(events[0]["message"]["conversation_id"], "C1");
    assert_eq!(events[0]["mention"], false);
}

#[tokio::test]
async fn filters_by_channel_dm_and_mentions() {
    let store = store();
    let mut all = open(&store, Filter::default()).await;
    let mut only_c2 = open(
        &store,
        Filter {
            channels: vec!["C2".into()],
            ..Default::default()
        },
    )
    .await;
    let mut dm = open(
        &store,
        Filter {
            dm: true,
            ..Default::default()
        },
    )
    .await;
    let mut mentions = open(
        &store,
        Filter {
            mentions: true,
            ..Default::default()
        },
    )
    .await;
    put(&store, "C1", &[msg("1700000001.000100", "U1", "plain")]);
    put(
        &store,
        "C2",
        &[msg("1700000002.000100", "U1", "hi <@U0ME>")],
    );
    put(&store, "D1", &[msg("1700000003.000100", "U1", "private")]);
    put(
        &store,
        "C1",
        &[msg("1700000004.000100", "U0ME", "note to <@U0ME>")],
    );
    put(
        &store,
        "C1",
        &[msg("1700000005.000100", "U3", "ping <@U9> only")],
    );

    assert_eq!(drain(&mut all).await.len(), 5);
    assert_eq!(texts(&drain(&mut only_c2).await), ["message:hi <@U0ME>"]);
    assert_eq!(texts(&drain(&mut dm).await), ["message:private"]);
    let m = drain(&mut mentions).await;
    // Mentions of me by someone else only; my own message and others' do not count.
    assert_eq!(texts(&m), ["message:hi <@U0ME>"]);
    assert_eq!(m[0]["mention"], true);
}

#[tokio::test]
async fn reaction_events_carry_actor_and_emoji() {
    let store = store();
    let base = msg("1700000001.000100", "U1", "react to me");
    put(&store, "C1", &[base.clone()]);
    let mut stream = open(&store, Filter::default()).await;
    put(
        &store,
        "C1",
        &[MessageRow {
            reactions: vec![("tada".into(), "U7".into())],
            ..base
        }],
    );
    let events = drain(&mut stream).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["type"], "reaction_added");
    assert_eq!(events[0]["actor_id"], "U7");
    assert_eq!(events[0]["reaction"], "tada");
    assert_eq!(events[0]["message"]["ts"], "1700000001.000100");
}

#[tokio::test]
async fn a_second_writer_on_a_file_database_is_seen_through_the_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let reader = Store::open(&path).unwrap();
    reader
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name, self_user_id) VALUES ('T1', 'Team', 'U0ME');
                 INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');",
            )?;
            Ok(())
        })
        .unwrap();
    let mut stream = open(&reader, Filter::default()).await;
    // The desktop app is another process with its own connection.
    let app = Store::open(&path).unwrap();
    put(
        &app,
        "C1",
        &[msg("1700000001.000100", "U1", "from the app")],
    );
    let events = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(events.len(), 1);
}

// ---- wait -----------------------------------------------------------------

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

fn target(store: &Store, conversation: &str, ts: &str) -> ReplyTarget {
    store
        .with_read(|c| ReplyTarget::resolve(c, "T1", conversation, Some(ts)))
        .unwrap()
}

/// Serves `scripts[i]` to the i-th connection; later ones get hello and stay quiet.
async fn ws_server(scripts: Vec<Vec<Value>>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&count);
    let mut scripts = scripts.into_iter();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            seen.fetch_add(1, Ordering::SeqCst);
            let script = scripts
                .next()
                .unwrap_or_else(|| vec![json!({"type": "hello"})]);
            tokio::spawn(async move {
                let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                for frame in script {
                    ws.send(Frame::text(frame.to_string())).await.unwrap();
                }
                while let Some(Ok(frame)) = ws.next().await {
                    if let Frame::Text(t) = &frame {
                        let v: Value = serde_json::from_str(t.as_str()).unwrap_or_default();
                        if v["type"] == "ping" {
                            let _ = ws
                                .send(Frame::text(
                                    json!({"type": "pong", "reply_to": v["id"]}).to_string(),
                                ))
                                .await;
                        }
                    }
                }
            });
        }
    });
    (url, count)
}

fn fast() -> FollowConfig {
    FollowConfig {
        ping_every: Duration::from_millis(50),
        dead_after: Duration::from_secs(5),
        backoff_min: Duration::from_millis(10),
        backoff_max: Duration::from_millis(40),
        ..Default::default()
    }
}

async fn mock_api(
    api: &MockServer,
    replies: impl Fn(&Request) -> Vec<Value> + Send + Sync + 'static,
) {
    Mock::given(path("/conversations.history"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"messages":[],"has_more":false})),
        )
        .mount(api)
        .await;
    Mock::given(path("/conversations.replies"))
        .respond_with(move |req: &Request| {
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"messages":replies(req),"has_more":false}))
        })
        .mount(api)
        .await;
}

fn root_json() -> Value {
    json!({"ts": ROOT, "thread_ts": ROOT, "user": "U0ME", "text": "question?", "reply_count": 0})
}

fn live_reply(user: &str) -> Value {
    json!({"type": "message", "channel": "C1", "ts": "1700000050.000100", "thread_ts": ROOT,
           "user": user, "text": "the answer"})
}

fn thread_store() -> Store {
    let store = store();
    let mut root = msg(ROOT, "U0ME", "question?");
    root.thread_ts = Some(ROOT.into());
    put(&store, "C1", &[root]);
    store
}

#[tokio::test]
async fn wait_returns_a_stored_reply_without_touching_the_network() {
    let store = thread_store();
    put(
        &store,
        "C1",
        &[reply("1700000010.000100", "U0ME", "my own follow-up")],
    );
    let t = target(&store, "C1", ROOT);
    assert!(
        t.stored_reply(&store).await.unwrap().is_none(),
        "own replies do not count"
    );
    put(
        &store,
        "C1",
        &[reply("1700000011.000100", "U5", "from Alice")],
    );
    // A different thread in the same channel does not count either.
    put(&store, "C1", &[msg("1700000012.000100", "U6", "unrelated")]);
    let found = t.stored_reply(&store).await.unwrap().unwrap();
    assert_eq!(
        (found.ts.as_str(), found.text.as_str()),
        ("1700000011.000100", "from Alice")
    );

    // `wait` itself answers from the DB; a client for a dead address proves no network use.
    let api = MockServer::start().await;
    let client = common::websocket_client(&api, "ws://127.0.0.1:9/ws").await;
    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    let message = t
        .wait(&client, &store, &me(), &fast(), None, shutdown)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(message.text, "from Alice");
    assert!(api.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn wait_in_a_dm_accepts_any_later_message_from_the_other_side() {
    let store = store();
    put(&store, "D1", &[msg("1700000001.000100", "U0ME", "ping?")]);
    put(
        &store,
        "D1",
        &[msg("1700000000.000100", "U8", "older than the question")],
    );
    let t = target(&store, "D1", "1700000001.000100");
    assert!(t.stored_reply(&store).await.unwrap().is_none());
    put(&store, "D1", &[msg("1700000002.000100", "U8", "pong")]);
    assert_eq!(t.stored_reply(&store).await.unwrap().unwrap().text, "pong");
}

#[tokio::test]
async fn wait_sees_a_reply_that_arrives_live() {
    let store = thread_store();
    let (ws, _) = ws_server(vec![vec![json!({"type": "hello"}), live_reply("U5")]]).await;
    let api = MockServer::start().await;
    mock_api(&api, |_| vec![root_json()]).await; // catch-up knows no reply
    let client = common::websocket_client(&api, &ws).await.without_pacing();
    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    let t = target(&store, "C1", ROOT);
    let message = tokio::time::timeout(
        Duration::from_secs(10),
        t.wait(&client, &store, &me(), &fast(), None, shutdown),
    )
    .await
    .expect("wait never returned")
    .unwrap()
    .unwrap();
    assert_eq!(message.text, "the answer");
    assert_eq!(message.user_id.as_deref(), Some("U5"));
}

#[tokio::test]
async fn wait_ignores_my_own_live_reply() {
    let store = thread_store();
    let (ws, _) = ws_server(vec![vec![json!({"type": "hello"}), live_reply("U0ME")]]).await;
    let api = MockServer::start().await;
    mock_api(&api, |_| vec![root_json()]).await;
    let client = common::websocket_client(&api, &ws).await.without_pacing();
    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    let t = target(&store, "C1", ROOT);
    let deadline = Instant::now() + Duration::from_millis(600);
    let err = t
        .wait(&client, &store, &me(), &fast(), Some(deadline), shutdown)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Timeout), "{err:?}");
}

#[tokio::test]
async fn wait_catches_up_the_thread_after_a_reconnect() {
    let store = thread_store();
    // The first socket drops before the reply is delivered; only the second
    // connection's catch-up can see it.
    let (ws, connections) = ws_server(vec![vec![
        json!({"type": "hello"}),
        json!({"type": "goodbye"}),
    ]])
    .await;
    let api = MockServer::start().await;
    let seen = Arc::clone(&connections);
    mock_api(&api, move |_| {
        let mut messages = vec![root_json()];
        if seen.load(Ordering::SeqCst) >= 2 {
            messages.push(json!({"ts": "1700000060.000100", "thread_ts": ROOT, "user": "U5", "text": "missed it"}));
        }
        messages
    })
    .await;
    let client = common::websocket_client(&api, &ws).await.without_pacing();
    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    let t = target(&store, "C1", ROOT);
    let message = tokio::time::timeout(
        Duration::from_secs(10),
        t.wait(&client, &store, &me(), &fast(), None, shutdown),
    )
    .await
    .expect("wait never returned")
    .unwrap()
    .unwrap();
    assert_eq!(message.text, "missed it");
    assert!(connections.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn wait_times_out_and_stops_on_shutdown() {
    let store = thread_store();
    let (ws, _) = ws_server(vec![]).await;
    let api = MockServer::start().await;
    mock_api(&api, |_| vec![root_json()]).await;
    let client = common::websocket_client(&api, &ws).await.without_pacing();
    let t = target(&store, "C1", ROOT);

    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    let started = Instant::now();
    let err = t
        .wait(
            &client,
            &store,
            &me(),
            &fast(),
            Some(started + Duration::from_millis(300)),
            shutdown,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Timeout));
    assert_eq!(err.kind().exit_code(), 9);
    assert!(started.elapsed() >= Duration::from_millis(300));

    // Ctrl-C ends the wait quietly, with no message and no error.
    let (stop, shutdown) = tokio::sync::watch::channel(false);
    let (identity, config) = (me(), fast());
    let waiter = t.wait(&client, &store, &identity, &config, None, shutdown);
    let stopper = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        stop.send(true).unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(waiter, stopper)
    })
    .await
    .unwrap();
    assert!(result.unwrap().is_none());
}

#[test]
fn migration_seeds_the_journal_from_existing_rows() {
    let store = store();
    let base = msg("1700000001.000100", "U1", "kept");
    put(&store, "C1", &[base.clone()]);
    put(
        &store,
        "C1",
        &[MessageRow {
            reactions: vec![("wave".into(), "U2".into())],
            ..base
        }],
    );
    // Re-run the seeding statements of the migration on a populated database.
    let sql = include_str!("../src/store/migrations/0022_events.sql");
    let seed = &sql[sql.find("INSERT INTO event_log").unwrap()
        ..sql.find("CREATE TRIGGER event_message_insert").unwrap()];
    store
        .with_conn(|c| {
            c.execute("DELETE FROM event_log", [])?;
            c.execute_batch(seed)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(kinds(&store), ["message", "reaction_added"]);
}
