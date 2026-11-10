//! One websocket session: handshake, `hello`, frames in and out.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message as Frame;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::api::Client;
use crate::{Error, Result};

const HELLO_TIMEOUT: Duration = Duration::from_secs(15);

fn ws_err(e: impl std::fmt::Display) -> Error {
    Error::WebSocket(e.to_string())
}

/// An open RTM session that has received `hello`.
pub struct Connection {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl Connection {
    /// `rtm.connect`, then the websocket handshake, then waits for `hello`.
    pub async fn open(client: &Client) -> Result<Self> {
        let url = client.rtm_connect().await?.url;
        client.validate_ws_url(&url)?;
        let mut req = url.as_str().into_client_request().map_err(ws_err)?;
        for (name, value) in client.ws_headers() {
            let mut v = HeaderValue::from_str(&value)
                .map_err(|_| Error::WebSocket(format!("bad {name} header")))?;
            v.set_sensitive(true);
            req.headers_mut().insert(name, v);
        }
        let (ws, _) = tokio_tungstenite::connect_async(req)
            .await
            .map_err(ws_err)?;
        let mut conn = Self { ws, next_id: 1 };

        let first = tokio::time::timeout(HELLO_TIMEOUT, conn.next_event())
            .await
            .map_err(|_| Error::WebSocket("no hello from Slack".into()))??;
        match first
            .as_ref()
            .and_then(|v| v.get("type"))
            .and_then(Value::as_str)
        {
            Some("hello") => Ok(conn),
            Some("error") => Err(Error::WebSocket(format!(
                "Slack refused the connection: {}",
                first
                    .as_ref()
                    .and_then(|v| v.pointer("/error/msg"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            ))),
            other => Err(Error::WebSocket(format!("expected hello, got {other:?}"))),
        }
    }

    /// Next JSON event; `None` when the server closed the connection.
    /// Control frames are handled here.
    pub async fn next_event(&mut self) -> Result<Option<Value>> {
        while let Some(frame) = self.ws.next().await {
            match frame.map_err(ws_err)? {
                Frame::Text(text) => match serde_json::from_str(text.as_str()) {
                    Ok(v) => return Ok(Some(v)),
                    Err(e) => tracing::debug!(error = %e, "ignoring non-JSON frame"),
                },
                Frame::Ping(data) => self.ws.send(Frame::Pong(data)).await.map_err(ws_err)?,
                Frame::Close(_) => return Ok(None),
                _ => {}
            }
        }
        Ok(None)
    }

    /// RTM-level ping; Slack answers with `{"type":"pong","reply_to":id}`.
    pub async fn ping(&mut self) -> Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        let frame = json!({"id": id, "type": "ping"}).to_string();
        self.ws.send(Frame::text(frame)).await.map_err(ws_err)?;
        Ok(id)
    }

    /// Replace the legacy RTM presence set and request its immediate snapshot.
    pub async fn subscribe_presence(&mut self, ids: &[String]) -> Result<()> {
        for kind in ["presence_sub", "presence_query"] {
            let id = self.next_id;
            self.next_id += 1;
            self.ws
                .send(Frame::text(
                    json!({"id": id, "type": kind, "ids": ids}).to_string(),
                ))
                .await
                .map_err(ws_err)?;
        }
        Ok(())
    }

    pub async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}
