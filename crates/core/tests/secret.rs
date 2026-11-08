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
