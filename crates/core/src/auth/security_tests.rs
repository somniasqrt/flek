//! Recovery is injected: no desktop profile, Keychain or live Slack access.
use super::*;

fn session(team: &str, kind: OriginKind) -> Session {
    Session {
        credentials: Credentials {
            team_id: team.into(),
            user_id: "UFAKE".into(),
            token: crate::Secret::new("xoxc-synthetic-recovery"),
            cookie: crate::Secret::new("xoxd-synthetic-recovery"),
            origin: Some(SessionOrigin::new(kind, "Synthetic UA")),
        },
        identity: AuthTest {
            url: "https://fake.slack.com/".into(),
            team: "Fake".into(),
            user: "Fake".into(),
            team_id: team.into(),
            user_id: "UFAKE".into(),
            enterprise_id: None,
        },
    }
}
fn desktop() -> OriginKind {
    OriginKind::Desktop {
        app: "fake".into(),
        electron: "fake".into(),
        chrome: "fake".into(),
    }
}
fn imported(session: Session) -> ImportedTeam {
    ImportedTeam {
        team_id: session.credentials.team_id.clone(),
        name: "Fake".into(),
        result: Ok(session),
    }
}
fn store() -> CredentialStore {
    CredentialStore::with_backend(keychain::MemoryBackend::default())
}

#[test]
fn import_saves_only_selected_or_first_successful_team() {
    for selected in [None, Some("T2")] {
        let store = store();
        let mut teams = vec![
            imported(session("T1", desktop())),
            imported(session("T2", desktop())),
        ];
        save_imported(&store, selected, &mut teams).unwrap();
        let expected = selected.unwrap_or("T1");
        assert_eq!(store.list().unwrap(), [expected]);
        assert!(store
            .load(if expected == "T1" { "T2" } else { "T1" })
            .unwrap()
            .is_none());
    }
    let store = store();
    let mut teams = vec![
        ImportedTeam {
            team_id: "TBAD".into(),
            name: "Fake".into(),
            result: Err(Error::DesktopNotLoggedIn),
        },
        imported(session("T2", desktop())),
    ];
    save_imported(&store, None, &mut teams).unwrap();
    assert_eq!(store.list().unwrap(), ["T2"]);
}

#[tokio::test]
async fn recovery_rejects_webview_manual_unknown_and_disabled_without_import() {
    for origin in [
        Some(OriginKind::Webview),
        Some(OriginKind::Manual),
        None,
        Some(desktop()),
    ] {
        let enabled = origin != Some(desktop());
        let store = store();
        let mut creds = session("T1", OriginKind::Webview).credentials;
        creds.origin = origin.map(|o| SessionOrigin::new(o, "Synthetic UA"));
        store.save(&creds).unwrap();
        let error = recover_with(&store, &creds, enabled, || async {
            panic!("desktop import must not run")
        })
        .await
        .unwrap_err();
        assert!(store.load("T1").unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
        for text in [error.to_string(), format!("{error:?}")] {
            assert!(!text.contains("xoxc-synthetic-recovery"));
            assert!(!text.contains("xoxd-synthetic-recovery"));
        }
    }
}

#[tokio::test]
async fn recovery_accepts_only_same_team_and_never_saves_another_workspace() {
    for matching in [false, true] {
        let store = store();
        let creds = session("T1", desktop()).credentials;
        store.save(&creds).unwrap();
        let result = recover_with(&store, &creds, true, || async {
            let mut teams = vec![imported(session("T2", desktop()))];
            if matching {
                teams.push(imported(session("T1", desktop())));
            }
            Ok(teams)
        })
        .await;
        assert_eq!(result.is_ok(), matching);
        assert!(store.load("T2").unwrap().is_none());
        assert_eq!(store.load("T1").unwrap().is_some(), matching);
    }
}

#[tokio::test]
async fn rejected_credentials_stay_deleted_when_import_fails() {
    for code in ["invalid_auth", "token_revoked", "account_inactive"] {
        let store = store();
        let creds = session("T1", desktop()).credentials;
        store.save(&creds).unwrap();
        let error = Error::Slack {
            code: code.into(),
            needed: None,
        };
        assert!(error.is_auth_error());
        let result = recover_with(&store, &creds, true, || async { Err(error) }).await;
        assert!(result.is_err());
        assert!(store.load("T1").unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
    }
}
