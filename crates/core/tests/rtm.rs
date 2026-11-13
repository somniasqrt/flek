//! RTM websocket against a local fake server.

mod common;

use std::sync::{Arc, Mutex};

use flek_core::api::Client;
use flek_core::rtm::Connection;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message as Frame;
use wiremock::MockServer;

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
    let client = common::websocket_client(&api, ws_url).await;
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

    use flek_core::api::AuthTest;
    use flek_core::store::{messages, Store};
    use flek_core::sync::follow::{follow, FollowConfig};
    use flek_core::sync::live::{bus, Change, ConnectionState};
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message as Frame;
    use wiremock::MockServer;

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
        let client = crate::common::websocket_client(&api, &ws_url).await;
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
    #[tokio::test]
    async fn catch_up_retries_without_dropping_a_healthy_socket() {
        let (url, connections) = scripted_server(vec![]).await;
        let api = MockServer::start().await;
        let client = crate::common::websocket_client(&api, &url).await;
        let store = Store::open_in_memory().unwrap();
        let bus = bus();
        let (stop, rx) = tokio::sync::watch::channel(false);
        let attempts = AtomicUsize::new(0);
        let cfg = FollowConfig {
            backoff_min: Duration::from_millis(10),
            backoff_max: Duration::from_millis(40),
            ..Default::default()
        };
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            follow(
                &client,
                &store,
                &me(),
                &bus,
                &cfg,
                async || {
                    if attempts.fetch_add(1, Ordering::SeqCst) < 2 {
                        return Err(flek_core::Error::Store("temporary failure".into()));
                    }
                    stop.send(true).unwrap();
                    Ok(())
                },
                rx,
            ),
        )
        .await
        .expect("catch-up was never retried");
        result.unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(connections.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn stale_presence_failure_restarts_follow_and_exposes_reconnecting() {
        let (url, _) = scripted_server(vec![]).await;
        let api = MockServer::start().await;
        let client = crate::common::websocket_client(&api, &url).await;
        let store = Store::open_in_memory().unwrap();
        store.with_conn(|c| {c.execute_batch("INSERT INTO presence(user_id) VALUES('U1'); CREATE TRIGGER fail_stale BEFORE UPDATE ON presence BEGIN SELECT RAISE(ABORT,'temporary failure'); END;")?;Ok(())}).unwrap();
        let bus = bus();
        let mut events = bus.subscribe();
        let (stop, rx) = tokio::sync::watch::channel(false);
        let cfg = FollowConfig {
            backoff_min: Duration::from_millis(10),
            backoff_max: Duration::from_millis(40),
            ..Default::default()
        };
        let identity = me();
        let runner = follow(
            &client,
            &store,
            &identity,
            &bus,
            &cfg,
            async || {
                stop.send(true).unwrap();
                Ok(())
            },
            rx,
        );
        let repair = async {
            loop {
                if matches!(
                    events.recv().await.unwrap(),
                    Change::Connection(ConnectionState::Reconnecting { .. })
                ) {
                    break;
                }
            }
            store
                .call(|c| {
                    c.execute_batch("DROP TRIGGER fail_stale")?;
                    Ok(())
                })
                .await
                .unwrap();
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(runner, repair)
        })
        .await
        .expect("engine never recovered");
        result.unwrap();
    }
    #[tokio::test]
    async fn catch_up_auth_failure_is_not_retried_or_masked_by_presence_failure() {
        let (url, connections) = scripted_server(vec![]).await;
        let api = MockServer::start().await;
        let client = crate::common::websocket_client(&api, &url).await;
        let store = Store::open_in_memory().unwrap();
        let attempts = AtomicUsize::new(0);
        let (_stop, rx) = tokio::sync::watch::channel(false);
        let result = follow(&client,&store,&me(),&bus(),&FollowConfig::default(),async || {
            attempts.fetch_add(1,Ordering::SeqCst);
            store.with_conn(|c| {
                c.execute_batch("INSERT INTO presence(user_id) VALUES('U1'); CREATE TRIGGER fail_stale BEFORE UPDATE ON presence BEGIN SELECT RAISE(ABORT,'temporary failure'); END;")?;
                Ok(())
            })?;
            Err(flek_core::Error::Slack {code:"invalid_auth".into(),needed:None})
        },rx).await;
        assert!(result.unwrap_err().is_auth_error());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(connections.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn shutdown_cancels_engine_restart_backoff() {
        let api = MockServer::start().await;
        let client = flek_core::api::Client::with_base_url(
            flek_core::Secret::new("xoxc-fake"),
            flek_core::Secret::new("xoxd-fake"),
            &api.uri(),
        )
        .unwrap();
        let store = Store::open_in_memory().unwrap();
        store.with_conn(|c| {c.execute_batch("INSERT INTO presence(user_id) VALUES('U1'); CREATE TRIGGER fail_stale BEFORE UPDATE ON presence BEGIN SELECT RAISE(ABORT,'temporary failure'); END;")?; Ok(())}).unwrap();
        let events = bus();
        let mut changes = events.subscribe();
        let (stop, rx) = tokio::sync::watch::channel(false);
        let cfg = FollowConfig {
            backoff_min: Duration::from_secs(60),
            ..Default::default()
        };
        let identity = me();
        let runner = follow(
            &client,
            &store,
            &identity,
            &events,
            &cfg,
            async || panic!("must not connect"),
            rx,
        );
        let cancel = async {
            loop {
                if matches!(
                    changes.recv().await.unwrap(),
                    Change::Connection(ConnectionState::Reconnecting { .. })
                ) {
                    stop.send(true).unwrap();
                    break;
                }
            }
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(runner, cancel)
        })
        .await
        .unwrap();
        result.unwrap();
    }
}
