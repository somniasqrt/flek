//! A local reverse proxy keeps HTTP API and websocket mocks on one exact origin.
use flek_core::{api::Client, Secret};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

pub async fn websocket_client(api: &MockServer, websocket: &str) -> Client {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let ws_url = origin.replacen("http:", "ws:", 1) + "/ws";
    Mock::given(path("/rtm.connect"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok":true,"url":ws_url})),
        )
        .with_priority(1)
        .mount(api)
        .await;
    let api_addr = reqwest::Url::parse(&api.uri())
        .unwrap()
        .socket_addrs(|| None)
        .unwrap()[0];
    let ws_addr = reqwest::Url::parse(websocket)
        .unwrap()
        .socket_addrs(|| None)
        .unwrap()[0];
    tokio::spawn(async move {
        loop {
            let (mut incoming, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut peek = [0; 4];
                if incoming.read_exact(&mut peek).await.is_err() {
                    return;
                }
                let target = if &peek == b"GET " { ws_addr } else { api_addr };
                let mut outgoing = tokio::net::TcpStream::connect(target).await.unwrap();
                outgoing.write_all(&peek).await.unwrap();
                let _ = tokio::io::copy_bidirectional(&mut incoming, &mut outgoing).await;
            });
        }
    });
    Client::with_base_url(Secret::new("xoxc-fake"), Secret::new("xoxd-fake"), &origin).unwrap()
}
