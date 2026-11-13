//! Secrets must not leak through formatting or logs.

use std::io::Write;
use std::sync::{Arc, Mutex};

use flek_core::auth::Credentials;
use flek_core::Secret;

const TOKEN: &str = "xoxc-1111-2222-3333-deadbeef";
const COOKIE: &str = "xoxd-FAKE%2Bvalue";

#[test]
fn debug_and_display_are_redacted() {
    let creds = Credentials {
        team_id: "T0FAKE".into(),
        user_id: "U0FAKE".into(),
        token: Secret::new(TOKEN),
        cookie: Secret::new(COOKIE),
        origin: None,
    };
    for out in [
        format!("{creds:?}"),
        format!("{creds:#?}"),
        format!("{}", creds.token),
    ] {
        assert!(!out.contains("deadbeef"), "{out}");
        assert!(!out.contains("FAKE%2B"), "{out}");
    }
    assert!(format!("{creds:?}").contains("xoxc-…***"));
}

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl Write for Buf {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn logs_are_redacted() {
    let buf = Buf::default();
    let writer = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();

    let token = Secret::new(TOKEN);
    let client = flek_core::api::Client::new(token.clone(), Secret::new(COOKIE)).unwrap();
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(?token, "debug field");
        tracing::info!(%token, "display field");
        tracing::info!("inline {token} {token:?}");
        tracing::info!(?client, "client");
    });

    let logged = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(logged.contains("xoxc-…***"), "{logged}");
    assert!(!logged.contains("deadbeef"), "{logged}");
    assert!(!logged.contains("FAKE%2B"), "{logged}");
}

#[test]
fn verbose_logging_caps_every_dependency_even_with_specific_user_directives() {
    use tracing_subscriber::prelude::*;
    for directives in [None, Some("trace,tungstenite::handshake::client=trace,reqwest=trace,unknown_dependency=trace,flek_core=trace,[capture]=trace")] {
        let buf = Buf::default();
        let writer = buf.clone();
        let subscriber = tracing_subscriber::registry()
            .with(flek_core::logging::filter(flek_core::logging::cli_default(u8::MAX), directives))
            .with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(move || writer.clone()));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::trace_span!(target: "flek_core", "capture");
            let _entered = span.enter();
            tracing::trace!(target: "flek_core", "flek verbose marker");
            tracing::trace!(target: "tungstenite::handshake::client", "dependency trace marker");
            tracing::debug!(target: "unknown_dependency", "dependency debug marker");
            tracing::debug!(target: "flek_lookalike", "dependency lookalike marker");
            tracing::info!(target: "reqwest", "dependency info marker");
        });
        let logged = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(logged.contains("flek verbose marker"));
        assert_eq!(logged.contains("dependency info marker"), directives.is_some());
        assert!(!logged.contains("dependency trace marker"));
        assert!(!logged.contains("dependency debug marker"));
        assert!(!logged.contains("dependency lookalike marker"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn verbose_websocket_handshake_does_not_log_session_or_connect_uri() {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
    use tracing::instrument::WithSubscriber;
    use tracing_subscriber::prelude::*;

    // Install the same log-to-tracing bridge that executable .init() installs.
    // This integration binary has no other global subscriber.
    let _ = tracing_subscriber::registry().try_init();
    let buf = Buf::default();
    let writer = buf.clone();
    let subscriber = tracing_subscriber::registry()
        .with(flek_core::logging::filter(
            flek_core::logging::cli_default(u8::MAX),
            Some("trace,tungstenite::handshake::client=trace,tokio_tungstenite=trace"),
        ))
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(move || writer.clone()),
        );
    let dispatch = tracing::Dispatch::new(subscriber);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(
        async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text("hello".into())).await.unwrap();
        }
        .with_subscriber(dispatch.clone()),
    );
    async {
        let mut request = format!("ws://{address}/?token={TOKEN}")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", format!("d={COOKIE}").parse().unwrap());
        let (_ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        tracing::info!(target: "flek_core", "connected to synthetic websocket");
    }
    .with_subscriber(dispatch)
    .await;
    server.await.unwrap();
    let logged = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(logged.contains("connected to synthetic websocket"));
    // Do not include captured handshake text in failures.
    assert!(
        !logged.contains(COOKIE),
        "cookie leaked into handshake logs"
    );
    assert!(!logged.contains(TOKEN), "token leaked into connect logs");
}
