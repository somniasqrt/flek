//! Credential store logic against the in-memory backend.

use flek_core::auth::keychain::{CredentialStore, Credentials, MemoryBackend};
use flek_core::Secret;

fn creds(team: &str) -> Credentials {
    Credentials {
        team_id: team.into(),
        user_id: format!("U_{team}"),
        token: Secret::new(format!("xoxc-fake-{team}")),
        cookie: Secret::new("xoxd-fake"),
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
