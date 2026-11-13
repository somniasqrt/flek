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

#[tokio::test]
async fn webview_saves_only_first_validated_workspace_and_stops_validation() {
    let store = store();
    let mut attempted = Vec::new();
    let selected = save_first_with(&store, ["TBAD", "T1", "T2"], |team| {
        attempted.push(team);
        std::future::ready(if team == "TBAD" {
            Err(Error::DesktopNotLoggedIn)
        } else {
            Ok(session(team, OriginKind::Webview))
        })
    })
    .await
    .unwrap();
    assert_eq!(selected.credentials.team_id, "T1");
    assert_eq!(store.list().unwrap(), ["T1"]);
    assert!(store.load("T2").unwrap().is_none());
    assert_eq!(attempted, ["TBAD", "T1"]);
}

fn settings_file(import_on_start: bool) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    crate::settings::Settings {
        desktop_import_on_start: import_on_start,
        ..Default::default()
    }
    .save(&path)
    .unwrap();
    (dir, path)
}

#[test]
fn explicit_import_turns_auto_import_back_on_only_when_it_succeeds() {
    // M9.10.2: sign-out switched the preference off; an explicit import restores it.
    let (_dir, path) = settings_file(false);
    let creds = store();
    let mut teams = vec![imported(session("T1", desktop()))];
    save_explicit_import(&creds, None, &mut teams, &path).unwrap();
    assert!(creds.load("T1").unwrap().is_some());
    assert!(
        crate::settings::Settings::load(&path)
            .unwrap()
            .desktop_import_on_start
    );

    let (_dir, path) = settings_file(false);
    let creds = store();
    let mut teams = vec![ImportedTeam {
        team_id: "TBAD".into(),
        name: "Fake".into(),
        result: Err(Error::DesktopNotLoggedIn),
    }];
    save_explicit_import(&creds, None, &mut teams, &path).unwrap();
    assert!(creds.list().unwrap().is_empty());
    assert!(
        !crate::settings::Settings::load(&path)
            .unwrap()
            .desktop_import_on_start
    );
}

#[test]
fn explicit_import_keeps_the_session_and_never_rewrites_broken_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let broken = b"{ not json";
    std::fs::write(&path, broken).unwrap();
    let creds = store();
    let mut teams = vec![imported(session("T1", desktop()))];
    save_explicit_import(&creds, None, &mut teams, &path).unwrap();
    assert!(creds.load("T1").unwrap().is_some());
    assert_eq!(std::fs::read(&path).unwrap(), broken);
}

#[tokio::test]
async fn missing_credentials_with_auto_import_off_name_the_way_back() {
    let error = resume_session(&store(), None, None, false)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::DesktopImportDisabled));
    assert!(error.to_string().contains("flek auth import"));
}

async fn cache_with_identity(team: &str) -> crate::store::Store {
    let db = crate::store::Store::open_in_memory().unwrap();
    let team = team.to_owned();
    db.call(move |c| {
        crate::store::directory::upsert_team(c, &team, "Fake", "https://fake.slack.com/", "UFAKE")?;
        c.execute(
            "INSERT INTO users(id, team_id, name) VALUES ('UFAKE', ?1, 'fake')",
            [&team],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    db
}

fn non_auth_errors() -> Vec<Error> {
    vec![
        // A captive portal answers with HTML.
        serde_json::from_str::<serde_json::Value>("<html>")
            .unwrap_err()
            .into(),
        Error::HttpStatus(403),
        Error::HttpStatus(503),
        Error::Slack {
            code: "ratelimited".into(),
            needed: None,
        },
    ]
}

#[tokio::test]
async fn anything_but_an_auth_error_starts_from_the_cache() {
    // M9.10.3: only Slack rejecting the session may lead to the sign-in screen.
    let db = cache_with_identity("T1").await;
    for error in non_auth_errors() {
        assert!(!error.is_auth_error());
        let creds = store();
        let saved = session("T1", desktop()).credentials;
        creds.save(&saved).unwrap();
        let resumed = resume_validated(&creds, Some(&db), saved, false, Err(error))
            .await
            .unwrap();
        assert_eq!(resumed.identity.team_id, "T1");
        assert_eq!(resumed.identity.user_id, "UFAKE");
        assert!(creds.load("T1").unwrap().is_some());
    }
}

#[tokio::test]
async fn without_a_cached_identity_the_original_error_comes_back() {
    let empty = crate::store::Store::open_in_memory().unwrap();
    for db in [None, Some(&empty)] {
        for error in non_auth_errors() {
            let text = error.to_string();
            let creds = store();
            let saved = session("T1", desktop()).credentials;
            creds.save(&saved).unwrap();
            let returned = resume_validated(&creds, db, saved, true, Err(error))
                .await
                .unwrap_err();
            assert_eq!(returned.to_string(), text);
            // The session may be fine: it stays for the next start.
            assert!(creds.load("T1").unwrap().is_some());
        }
    }
}

#[tokio::test]
async fn a_rejected_session_is_removed_even_with_a_cache() {
    let db = cache_with_identity("T1").await;
    for code in [
        "invalid_auth",
        "token_revoked",
        "not_authed",
        "account_inactive",
    ] {
        let creds = store();
        let saved = session("T1", desktop()).credentials;
        creds.save(&saved).unwrap();
        let rejected = Err(Error::Slack {
            code: code.into(),
            needed: None,
        });
        let error = resume_validated(&creds, Some(&db), saved, false, rejected)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::DesktopImportDisabled));
        assert!(creds.load("T1").unwrap().is_none());
    }
}
