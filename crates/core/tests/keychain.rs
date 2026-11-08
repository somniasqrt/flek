//! Credential store logic against the in-memory backend.

use flek_core::auth::keychain::{CredentialStore, Credentials, MemoryBackend, SecretBackend};
use flek_core::auth::{OriginKind, SessionOrigin};
use flek_core::Secret;

fn creds(team: &str) -> Credentials {
    Credentials {
        team_id: team.into(),
        user_id: format!("U_{team}"),
        token: Secret::new(format!("xoxc-fake-{team}")),
        cookie: Secret::new("xoxd-fake"),
        origin: None,
    }
}

#[test]
fn save_load_list_delete() {
    let store = CredentialStore::with_backend(MemoryBackend::default());
    assert!(store.list().unwrap().is_empty());
    assert!(store.load("T1").unwrap().is_none());

    store.save(&creds("T1")).unwrap();
    store.save(&creds("T2")).unwrap();
    store.save(&creds("T1")).unwrap(); // overwrite, no duplicate in index
    assert_eq!(store.list().unwrap(), ["T1", "T2"]);

    let loaded = store.load("T2").unwrap().unwrap();
    assert_eq!(loaded.user_id, "U_T2");
    assert_eq!(loaded.token.expose(), "xoxc-fake-T2");
    assert_eq!(loaded.cookie.expose(), "xoxd-fake");

    assert!(store.delete("T1").unwrap());
    assert!(!store.delete("T1").unwrap());
    assert_eq!(store.list().unwrap(), ["T2"]);
    assert!(store.load("T1").unwrap().is_none());

    assert!(store.delete("T2").unwrap());
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn origin_round_trips_and_old_records_still_load() {
    let backend = MemoryBackend::default();
    // A record written before origins existed.
    backend
        .set(
            "T0",
            r#"{"team_id":"T0","user_id":"U0","token":"xoxc-fake","cookie":"xoxd-fake"}"#,
        )
        .unwrap();
    let store = CredentialStore::with_backend(backend);
    assert!(store.load("T0").unwrap().unwrap().origin.is_none());

    let mut c = creds("T1");
    c.origin = Some(SessionOrigin::new(
        OriginKind::Webview,
        "Mozilla/5.0 (Test)",
    ));
    store.save(&c).unwrap();
    let loaded = store.load("T1").unwrap().unwrap();
    assert_eq!(loaded.origin, c.origin);
}
