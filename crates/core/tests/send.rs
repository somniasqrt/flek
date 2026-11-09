//! Sending messages against a mock Slack. Never hits real Slack.

mod support;
use support::{field, has_field};

use flek_core::api::chat::describe_send_error;
use flek_core::api::Client;
use flek_core::rtm::Event;
use flek_core::store::messages::{self, HistoryRow};
use flek_core::store::Store;
use flek_core::sync::live::{apply, Change};
use flek_core::sync::send::{send, Outgoing};
use flek_core::{Error, Secret};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const ME: &str = "U0ME";
const ROOT: &str = "1700000000.000100";

fn client(server: &MockServer) -> Client {
    Client::with_base_url(
        Secret::new("xoxc-fake-token"),
        Secret::new("xoxd-fake-cookie"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing()
}

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name, self_user_id) VALUES ('T1', 'Team', 'U0ME');
                 INSERT INTO conversations (id, team_id, kind, name, label)
                 VALUES ('C1', 'T1', 'public_channel', 'general', 'general');",
            )?;
            Ok(())
        })
        .unwrap();
    store
}

fn row(store: &Store, ts: &str) -> Option<HistoryRow> {
    store.with_read(|c| messages::message(c, "C1", ts)).unwrap()
}

fn count(store: &Store, ts: &str) -> i64 {
    store
        .with_read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id = 'C1' AND ts = ?1",
                [ts],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

fn outgoing(text: &str) -> Outgoing {
    Outgoing {
        conversation_id: "C1".into(),
        text: text.into(),
        client_msg_id: Some("11111111-2222-4333-8444-555555555555".into()),
        ..Default::default()
    }
}

/// What Slack answers to chat.postMessage (shape of the documented response).
fn posted(ts: &str, text: &str, extra: serde_json::Value) -> ResponseTemplate {
    let mut message = json!({
        "type": "message", "user": ME, "text": text, "ts": ts,
        "client_msg_id": "11111111-2222-4333-8444-555555555555", "team": "T1",
    });
    message
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    ResponseTemplate::new(200).set_body_json(json!({
        "ok": true, "channel": "C1", "ts": ts, "message": message,
    }))
}

/// Like Slack: a request without channel or text is refused.
fn post_mock() -> wiremock::MockBuilder {
    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .and(|req: &Request| !req.headers.contains_key("authorization"))
        .and(field("token", "xoxc-fake-token"))
        .and(has_field("channel"))
        .and(has_field("text"))
}

#[tokio::test]
async fn success_stores_the_message() {
    let server = MockServer::start().await;
    post_mock()
        .and(field("channel", "C1"))
        // Reserved characters escaped, mrkdwn left as typed.
        .and(field("text", "*a* &lt; b &amp; c"))
        .and(field(
            "client_msg_id",
            "11111111-2222-4333-8444-555555555555",
        ))
        .and(|req: &Request| {
            let body = String::from_utf8_lossy(&req.body);
            !body.contains("name=\"thread_ts\"") && !body.contains("name=\"reply_broadcast\"")
        })
        .respond_with(posted("1700000001.000200", "*a* &lt; b &amp; c", json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let store = store();
    let sent = send(&client(&server), &store, ME, outgoing("*a* < b & c"))
        .await
        .unwrap();
    assert_eq!(sent.ts, "1700000001.000200");
    assert_eq!(sent.client_msg_id, "11111111-2222-4333-8444-555555555555");
    assert_eq!(
        sent.change,
        Some(Change::Message {
            conversation_id: "C1".into(),
            ts: "1700000001.000200".into()
        })
    );
    let r = row(&store, "1700000001.000200").expect("stored");
    assert_eq!(r.text, "*a* &lt; b &amp; c");
    assert_eq!(r.user_id.as_deref(), Some(ME));
    assert_eq!(
        r.client_msg_id.as_deref(),
        Some("11111111-2222-4333-8444-555555555555")
    );
}

#[tokio::test]
async fn generates_a_client_msg_id_when_none_is_given() {
    let server = MockServer::start().await;
    post_mock()
        .and(has_field("client_msg_id"))
        .respond_with(posted("1700000001.000200", "hi", json!({})))
        .mount(&server)
        .await;
    let mut out = outgoing("hi");
    out.client_msg_id = None;
    let sent = send(&client(&server), &store(), ME, out).await.unwrap();
    assert_eq!(sent.client_msg_id.len(), 36);
}

async fn with_root(store: &Store) {
    apply(
        store,
        Event::parse(
            json!({"type": "message", "channel": "C1", "ts": ROOT, "user": "U2", "text": "root"}),
        ),
    )
    .await
    .unwrap();
}

fn reply_count(store: &Store) -> i64 {
    row(store, ROOT).unwrap().reply_count
}

#[tokio::test]
async fn thread_reply_with_broadcast() {
    let server = MockServer::start().await;
    post_mock()
        .and(field("thread_ts", ROOT))
        .and(field("reply_broadcast", "true"))
        // Slack's answer for a broadcast reply; thread_ts and subtype left
        // out on purpose: the client fills them from the request.
        .respond_with(posted("1700000002.000300", "also here", json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let store = store();
    with_root(&store).await;
    let mut out = outgoing("also here");
    out.thread_ts = Some(ROOT.into());
    out.broadcast = true;
    send(&client(&server), &store, ME, out).await.unwrap();

    let r = row(&store, "1700000002.000300").unwrap();
    assert_eq!(r.thread_ts.as_deref(), Some(ROOT));
    assert_eq!(r.subtype.as_deref(), Some("thread_broadcast"));
    assert_eq!(reply_count(&store), 1);
    // A broadcast shows in the channel timeline too.
    let top: Vec<String> = store
        .with_read(|c| messages::recent(c, "C1", 10))
        .unwrap()
        .into_iter()
        .map(|r| r.ts)
        .collect();
    assert_eq!(top, [ROOT, "1700000002.000300"]);
}

#[tokio::test]
async fn plain_thread_reply_stays_in_the_thread() {
    let server = MockServer::start().await;
    post_mock()
        .and(field("thread_ts", ROOT))
        .and(|req: &Request| !String::from_utf8_lossy(&req.body).contains("reply_broadcast"))
        .respond_with(posted(
            "1700000002.000300",
            "in thread",
            json!({"thread_ts": ROOT}),
        ))
        .expect(1)
        .mount(&server)
        .await;

    let store = store();
    with_root(&store).await;
    let mut out = outgoing("in thread");
    out.thread_ts = Some(ROOT.into());
    send(&client(&server), &store, ME, out).await.unwrap();
    assert_eq!(reply_count(&store), 1);
    let top = store.with_read(|c| messages::recent(c, "C1", 10)).unwrap();
    assert_eq!(top.len(), 1);
}

async fn refused(code: &str) -> (Error, Store) {
    let server = MockServer::start().await;
    post_mock()
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": false, "error": code})))
        .mount(&server)
        .await;
    let store = store();
    let err = send(&client(&server), &store, ME, outgoing("hi"))
        .await
        .unwrap_err();
    (err, store)
}

#[tokio::test]
async fn slack_errors_map_cleanly_and_store_nothing() {
    for (code, says) in [
        ("channel_not_found", "does not know this conversation"),
        ("not_in_channel", "not a member"),
        ("is_archived", "archived"),
        ("some_new_error", "(some_new_error)"),
    ] {
        let (err, store) = refused(code).await;
        assert!(
            matches!(&err, Error::Slack { code: c, .. } if c == code),
            "{err:?}"
        );
        assert!(describe_send_error(&err).contains(says), "{code}");
        assert!(!err.is_auth_error());
        let n: i64 = store
            .with_read(|c| Ok(c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 0);
    }
    let (err, _) = refused("invalid_auth").await;
    assert!(err.is_auth_error());
}

#[tokio::test]
async fn empty_text_is_not_sent() {
    let server = MockServer::start().await;
    let err = send(&client(&server), &store(), ME, outgoing("  \n "))
        .await
        .unwrap_err();
    assert!(matches!(&err, Error::Slack { code, .. } if code == "no_text"));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn ratelimited_is_retried_once_slack_allows() {
    let server = MockServer::start().await;
    post_mock()
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Retry-After", "0")
                .set_body_json(json!({"ok": false, "error": "ratelimited"})),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    post_mock()
        .respond_with(posted("1700000001.000200", "hi", json!({})))
        .with_priority(2)
        .mount(&server)
        .await;

    let store = store();
    let sent = send(&client(&server), &store, ME, outgoing("hi"))
        .await
        .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    assert!(row(&store, &sent.ts).is_some());
}

#[tokio::test]
async fn persistent_rate_limit_gives_up_with_ratelimited() {
    let server = MockServer::start().await;
    post_mock()
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .mount(&server)
        .await;
    let err = send(&client(&server), &store(), ME, outgoing("hi"))
        .await
        .unwrap_err();
    assert!(matches!(&err, Error::Slack { code, .. } if code == "ratelimited"));
    assert!(describe_send_error(&err).contains("rate limiting"));
}

/// The websocket echo of our own reply, as RTM delivers it.
fn echo(text: &str) -> serde_json::Value {
    json!({"type": "message", "channel": "C1", "user": ME, "text": text,
           "ts": "1700000002.000300", "thread_ts": ROOT,
           "client_msg_id": "11111111-2222-4333-8444-555555555555", "team": "T1"})
}

async fn send_reply(server: &MockServer, store: &Store) {
    post_mock()
        .respond_with(posted(
            "1700000002.000300",
            "reply",
            json!({"thread_ts": ROOT}),
        ))
        .mount(server)
        .await;
    let mut out = outgoing("reply");
    out.thread_ts = Some(ROOT.into());
    send(&client(server), store, ME, out).await.unwrap();
}

#[tokio::test]
async fn live_echo_after_send_does_not_duplicate() {
    let server = MockServer::start().await;
    let store = store();
    with_root(&store).await;
    send_reply(&server, &store).await;

    apply(&store, Event::parse(echo("reply"))).await.unwrap();
    assert_eq!(count(&store, "1700000002.000300"), 1);
    assert_eq!(reply_count(&store), 1);
    let thread = store
        .with_read(|c| messages::thread(c, "C1", ROOT))
        .unwrap();
    assert_eq!(thread.len(), 2);
    assert_eq!(
        thread[1].client_msg_id.as_deref(),
        Some("11111111-2222-4333-8444-555555555555")
    );
}

#[tokio::test]
async fn live_echo_before_the_response_does_not_duplicate() {
    let server = MockServer::start().await;
    let store = store();
    with_root(&store).await;
    apply(&store, Event::parse(echo("reply"))).await.unwrap();
    send_reply(&server, &store).await;
    assert_eq!(count(&store, "1700000002.000300"), 1);
    assert_eq!(reply_count(&store), 1);
}
