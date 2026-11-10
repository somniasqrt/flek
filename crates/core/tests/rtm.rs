//! RTM websocket against a local fake server.

use std::sync::{Arc, Mutex};

use flek_core::api::Client;
use flek_core::rtm::Connection;
use flek_core::Secret;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message as Frame;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

// The handshake callback's error type is fixed by tungstenite.
#[allow(clippy::result_large_err)]
/// Starts a websocket server that sends `frames` to the first client, then
/// keeps the socket open. Returns its URL and the handshake headers it saw.
async fn ws_server(frames: Vec<serde_json::Value>) -> (String, Arc<Mutex<Vec<(String, String)>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/websocket/fake", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = Arc::clone(&seen);
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_hdr_async(tcp, |req: &Request, resp: Response| {
            let mut s = seen2.lock().unwrap();
            for (k, v) in req.headers() {
                s.push((k.to_string(), v.to_str().unwrap_or_default().to_owned()));
            }
            Ok(resp)
        })
        .await
        .unwrap();
        for f in frames {
            ws.send(Frame::text(f.to_string())).await.unwrap();
        }
        while ws.next().await.is_some() {}
    });
    (url, seen)
}

async fn client_for(ws_url: &str) -> (MockServer, Client) {
    let api = MockServer::start().await;
    Mock::given(path("/rtm.connect"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "url": ws_url})))
        .mount(&api)
        .await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &api.uri(),
    )
    .unwrap();
    (api, client)
}

#[tokio::test]
async fn connects_with_session_headers_and_reads_events() {
    let (url, seen) = ws_server(vec![
        json!({"type": "hello"}),
        json!({"type": "message", "channel": "C1", "ts": "1.000000", "user": "U1", "text": "hi"}),
    ])
    .await;
    let (_api, client) = client_for(&url).await;

    let mut conn = Connection::open(&client).await.unwrap();
    let ev = conn.next_event().await.unwrap().unwrap();
    assert_eq!(ev["type"], "message");
    assert_eq!(ev["text"], "hi");

    let headers = seen.lock().unwrap().clone();
    let get = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(get("cookie").as_deref(), Some("d=xoxd-fake"));
    assert_eq!(get("origin").as_deref(), Some("https://app.slack.com"));
    assert!(get("user-agent").unwrap().contains("Mozilla"));
}

#[tokio::test]
async fn slack_error_instead_of_hello_fails() {
    let (url, _) = ws_server(vec![
        json!({"type": "error", "error": {"code": 1, "msg": "Socket URL has expired"}}),
    ])
    .await;
    let (_api, client) = client_for(&url).await;
    let err = Connection::open(&client).await.err().unwrap();
    assert!(err.to_string().contains("Socket URL has expired"), "{err}");
}

mod follow {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use flek_core::api::{AuthTest, Client};
    use flek_core::store::{messages, Store};
    use flek_core::sync::follow::{follow, FollowConfig};
    use flek_core::sync::live::{bus, Change, ConnectionState};
    use flek_core::Secret;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message as Frame;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    enum Step {
        Send(serde_json::Value),
        /// Stop talking but keep the socket open (dead connection).
        GoSilent,
    }

    /// Serves `scripts[i]` to the i-th connection; later ones get hello and stay quiet.
    async fn scripted_server(scripts: Vec<Vec<Step>>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/ws", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let count2 = Arc::clone(&count);
        let mut scripts = scripts.into_iter();
        tokio::spawn(async move {
            loop {
                let (tcp, _) = listener.accept().await.unwrap();
                count2.fetch_add(1, Ordering::SeqCst);
                let script = scripts
                    .next()
                    .unwrap_or_else(|| vec![Step::Send(json!({"type": "hello"}))]);
                tokio::spawn(async move {
                    let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                    let mut silent = false;
                    for step in script {
                        match step {
                            Step::Send(v) => ws.send(Frame::text(v.to_string())).await.unwrap(),
                            Step::GoSilent => silent = true,
                        }
                    }
                    // Answer RTM pings unless silent; keep reading until the client leaves.
                    while let Some(Ok(frame)) = ws.next().await {
                        if let (false, Frame::Text(t)) = (silent, &frame) {
                            let v: serde_json::Value =
                                serde_json::from_str(t.as_str()).unwrap_or_default();
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

    #[tokio::test]
    async fn reconnects_after_goodbye_and_dead_connection() {
        let (ws_url, connections) = scripted_server(vec![
            vec![Step::Send(json!({"type": "hello"})), Step::Send(json!({"type": "goodbye"}))],
            vec![
                Step::Send(json!({"type": "hello"})),
                Step::Send(json!({"type": "message", "channel": "C1", "ts": "5.000000", "user": "U1", "text": "live!"})),
                Step::GoSilent,
            ],
        ])
        .await;
        let api = MockServer::start().await;
        Mock::given(path("/rtm.connect"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"ok": true, "url": ws_url})),
            )
            .mount(&api)
            .await;
        let client = Client::with_base_url(
            Secret::new("xoxc-fake"),
            Secret::new("xoxd-fake"),
            &api.uri(),
        )
        .unwrap();
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

        let bus = bus();
        let mut changes = bus.subscribe();
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let cfg = FollowConfig {
            presence: None,
            ping_every: Duration::from_millis(50),
            dead_after: Duration::from_millis(300),
            backoff_min: Duration::from_millis(10),
            backoff_max: Duration::from_millis(40),
        };

        let catch_ups = AtomicUsize::new(0);
        let me = me();
        let watcher = async {
            let mut connected = 0;
            let mut reconnecting = 0;
            let mut got_message = false;
            while connected < 3 || !got_message {
                match changes.recv().await.unwrap() {
                    Change::Connection(ConnectionState::Connected) => connected += 1,
                    Change::Connection(ConnectionState::Reconnecting { .. }) => reconnecting += 1,
                    Change::Message { ts, .. } => got_message = ts == "5.000000",
                    _ => {}
                }
            }
            stop_tx.send(true).unwrap();
            (connected, reconnecting)
        };
        let runner = follow(
            &client,
            &store,
            &me,
            &bus,
            &cfg,
            async || {
                catch_ups.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            stop_rx,
        );
        let (result, (connected, reconnecting)) =
            tokio::time::timeout(Duration::from_secs(10), async {
                tokio::join!(runner, watcher)
            })
            .await
            .expect("follow loop did not reconnect in time");
        result.unwrap();

        assert!(connected >= 3);
        assert!(reconnecting >= 2);
        assert!(connections.load(Ordering::SeqCst) >= 3);
        // The watcher stops right after the 3rd connect, so count the first two.
        assert!(catch_ups.load(Ordering::SeqCst) >= 2);
        let rows = store.with_conn(|c| messages::recent(c, "C1", 10)).unwrap();
        assert_eq!(rows[0].text, "live!");
    }
}
