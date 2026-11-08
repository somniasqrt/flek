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
