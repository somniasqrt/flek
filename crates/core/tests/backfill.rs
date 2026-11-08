//! History backfill and the hidden-by-limit rule.

mod support;
use support::field;

use std::time::Duration;

use flek_core::api::Client;
use flek_core::store::messages::{self, MessageRow, SyncState};
use flek_core::store::Store;
use flek_core::sync::backfill::{backfill_conversation, RateLimiter};
use flek_core::sync::SyncOptions;
use flek_core::Secret;
use serde_json::{json, Value};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
                 INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');",
            )?;
            Ok(())
        })
        .unwrap();
    store
}

fn row(ts: &str, text: &str, hidden: bool) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        text: text.into(),
        hidden_by_limit: hidden,
        raw_json: "{}".into(),
        ..Default::default()
    }
}

fn text_of(store: &Store, ts: &str) -> (String, bool) {
    store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT text, hidden_by_limit FROM messages WHERE conversation_id = 'C1' AND ts = ?1",
                [ts],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

#[test]
fn hidden_version_never_overwrites_full_message() {
    let store = store();
    store
        .with_conn(|c| {
            messages::upsert_messages(c, "C1", &[row("1.000001", "original text", false)])?;
            messages::upsert_messages(c, "C1", &[row("1.000001", "", true)])
        })
        .unwrap();
    assert_eq!(text_of(&store, "1.000001"), ("original text".into(), false));
}

#[test]
fn full_version_replaces_hidden_and_edits_apply() {
    let store = store();
    store
        .with_conn(|c| {
            messages::upsert_messages(c, "C1", &[row("1.000002", "", true)])?;
            messages::upsert_messages(c, "C1", &[row("1.000002", "recovered", false)])?;
            messages::upsert_messages(c, "C1", &[row("1.000002", "edited", false)])
        })
        .unwrap();
    assert_eq!(text_of(&store, "1.000002"), ("edited".into(), false));
}

fn msg(ts: &str) -> Value {
    json!({"type": "message", "ts": ts, "user": "U1", "text": format!("message {ts}")})
}

fn page(messages: &[Value], has_more: bool) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_json(json!({"ok": true, "messages": messages, "has_more": has_more}))
}

fn state(store: &Store) -> SyncState {
    store.with_conn(|c| messages::sync_state(c, "C1")).unwrap()
}

fn count(store: &Store) -> i64 {
    store
        .with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?))
        .unwrap()
}

#[tokio::test]
async fn resumes_after_interruption_and_catches_up() {
    let server = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    let store = store();
    let mut limiter = RateLimiter::new(Duration::ZERO);

    // Run 1: first page ok, second page fails (network drop).
    Mock::given(path("/conversations.history"))
        .and(field("latest", "104.000000"))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.history"))
        .respond_with(page(&[msg("105.000000"), msg("104.000000")], true))
        .with_priority(2)
        .mount(&server)
        .await;

    assert!(
        backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
            .await
            .is_err()
    );
    assert_eq!(
        state(&store),
        SyncState {
            oldest_ts: Some("104.000000".into()),
            newest_ts: Some("105.000000".into()),
            complete: false
        }
    );
    assert_eq!(count(&store), 2);

    // Run 2: a new message arrived; the older page now loads.
    server.reset().await;
    Mock::given(path("/conversations.history"))
        .and(field("oldest", "105.000000"))
        .respond_with(page(&[msg("106.000000")], false))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.history"))
        .and(field("latest", "104.000000"))
        .respond_with(page(&[msg("103.000000"), msg("102.000000")], false))
        .expect(1)
        .mount(&server)
        .await;

    let stats = backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(stats.messages, 3);
    assert!(stats.complete);
    assert_eq!(
        state(&store),
        SyncState {
            oldest_ts: Some("102.000000".into()),
            newest_ts: Some("106.000000".into()),
            complete: true
        }
    );
    assert_eq!(count(&store), 5);

    // Run 3: nothing new → only the catch-up request, no backfill.
    server.reset().await;
    Mock::given(path("/conversations.history"))
        .and(field("oldest", "106.000000"))
        .respond_with(page(&[], false))
        .expect(1)
        .mount(&server)
        .await;
    let stats = backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    assert_eq!((stats.pages, stats.messages), (1, 0));
}

#[tokio::test]
async fn hidden_messages_from_api_are_flagged_and_do_not_clobber() {
    let server = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    let store = store();
    store
        .with_conn(|c| {
            messages::upsert_messages(c, "C1", &[row("100.000000", "kept from archive", false)])
        })
        .unwrap();

    Mock::given(path("/conversations.history"))
        .respond_with(page(
            &[
                json!({"type": "message", "ts": "101.000000", "user": "U1", "text": "",
                       "files": [{"id": "F1", "mode": "hidden_by_limit"}]}),
                json!({"type": "message", "ts": "100.000000", "user": "U1", "text": ""}),
            ],
            false,
        ))
        .mount(&server)
        .await;

    backfill_conversation(
        &client,
        &store,
        &mut RateLimiter::new(Duration::ZERO),
        "C1",
        &SyncOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        text_of(&store, "100.000000"),
        ("kept from archive".into(), false)
    );
    assert_eq!(text_of(&store, "101.000000"), ("".into(), true));
}

#[tokio::test(start_paused = true)]
async fn limiter_spaces_requests() {
    let mut limiter = RateLimiter::new(Duration::from_secs(1));
    let start = tokio::time::Instant::now();
    for _ in 0..3 {
        limiter.wait().await;
    }
    assert_eq!(start.elapsed(), Duration::from_secs(2));
}

/// Regression (found live 2026-11-08): a conversation that was empty when
/// its backfill completed never caught up on new messages.
#[tokio::test]
async fn empty_conversation_catches_up_later() {
    let server = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    let store = store();
    let mut limiter = RateLimiter::new(Duration::ZERO);

    Mock::given(path("/conversations.history"))
        .respond_with(page(&[], false))
        .mount(&server)
        .await;
    backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    assert!(state(&store).complete);

    server.reset().await;
    Mock::given(path("/conversations.history"))
        // No `oldest` at all: Slack rejects a zero ts with invalid_ts_oldest.
        .and(|req: &wiremock::Request| !String::from_utf8_lossy(&req.body).contains("oldest"))
        .respond_with(page(&[msg("50.000000")], false))
        .expect(1)
        .mount(&server)
        .await;
    let stats = backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(stats.messages, 1);
    assert_eq!(state(&store).newest_ts.as_deref(), Some("50.000000"));
}

/// Regression: after a reconnect, a live message that lands before catch-up
/// reaches the conversation must not hide what was sent while offline.
#[tokio::test]
async fn live_message_before_catch_up_does_not_skip_offline_ones() {
    use flek_core::rtm::Event;
    use flek_core::sync::live::apply;

    let server = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    let store = store();
    let mut limiter = RateLimiter::new(Duration::ZERO);
    Mock::given(path("/conversations.history"))
        .respond_with(page(&[msg("10.000000")], false))
        .mount(&server)
        .await;
    backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();

    // Offline: "20" sent. Back online: live "30" arrives first.
    apply(&store, Event::parse(json!({"type": "message", "channel": "C1", "ts": "30.000000", "user": "U1", "text": "live"})))
        .await
        .unwrap();

    server.reset().await;
    Mock::given(path("/conversations.history"))
        .and(field("oldest", "10.000000"))
        .respond_with(page(&[msg("30.000000"), msg("20.000000")], false))
        .expect(1)
        .mount(&server)
        .await;
    backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(count(&store), 3);
}
