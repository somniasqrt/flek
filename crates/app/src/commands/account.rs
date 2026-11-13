//! Account IPC adapters.

use super::{text, CmdResult};
use crate::state::{self, AppState, Status};
use flek_core::sync::live::Progress;
use tauri::{AppHandle, State};

#[tauri::command]
pub fn status(state: State<'_, AppState>) -> Status {
    state.status()
}

#[tauri::command]
pub fn progress(state: State<'_, AppState>) -> Option<Progress> {
    state.progress()
}

/// Retry the Slack desktop import (onboarding screen).
#[tauri::command]
pub async fn import_desktop(app: AppHandle) {
    state::import_desktop(app).await;
}

/// Open the "Sign in to Slack" window.
#[tauri::command]
pub async fn sign_in(app: AppHandle) -> CmdResult<()> {
    crate::browsing_data::upgrade(&app).await?;
    crate::login::open(&app).map_err(text)
}

#[tauri::command]
pub async fn sign_out(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let _maintenance = state.maintenance.lock().await;
    state.stop_and_wait().await;
    let signed_out = purge_stored(
        &state.creds,
        &state.store,
        state.team_id(),
        flek_core::settings::default_path(),
    )
    .await
    .map_err(text);
    {
        let mut current = state.settings.lock().map_err(text)?;
        // Keep stale UI preferences from re-enabling import after a partial
        // settings or credential-store failure.
        current.desktop_import_on_start = false;
        if let Ok(settings) = &signed_out {
            *current = settings.clone();
        }
    }
    state.forget_session();
    let cleanup = crate::browsing_data::clear(&app).await;
    state::set_status(
        &app,
        Status::SignIn {
            desktop: false,
            reason: None,
        },
    );
    signed_out?;
    cleanup
}

async fn purge_stored(
    credentials: &flek_core::auth::CredentialStore,
    store: &flek_core::store::Store,
    active_team: Option<String>,
    settings_path: flek_core::Result<std::path::PathBuf>,
) -> flek_core::Result<flek_core::settings::Settings> {
    let known = store.read(flek_core::reads::team_ids).await;
    let mut ids: Vec<_> = active_team.into_iter().collect();
    if let Ok(teams) = &known {
        ids.extend(teams.iter().cloned());
    }
    let purged = match settings_path {
        Ok(path) => flek_core::auth::sign_out(credentials, &ids, &path),
        Err(error) => {
            let _ = credentials.clear_known(&ids);
            Err(error)
        }
    };
    // A database failure cannot prevent a best-effort credential purge.
    known?;
    purged
}

#[cfg(test)]
mod tests {
    use super::*;
    use flek_core::auth::{
        keychain::{MemoryBackend, SecretBackend},
        CredentialStore, Credentials,
    };

    #[tokio::test]
    async fn sign_out_without_an_active_session_purges_indexed_and_db_known_credentials() {
        for malformed in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let store = flek_core::store::Store::open(&dir.path().join("archive.db")).unwrap();
            store
                .with_conn(|c| {
                    flek_core::store::directory::upsert_team(
                        c,
                        "TUNUSABLE",
                        "Fake",
                        "https://fake.slack.com",
                        "UFAKE",
                    )
                })
                .unwrap();
            let backend = MemoryBackend::default();
            if malformed {
                backend.set("_teams", "malformed index").unwrap();
                backend
                    .set("TUNUSABLE", "unusable stored credentials")
                    .unwrap();
            }
            let credentials = CredentialStore::with_backend(backend);
            if !malformed {
                credentials
                    .save(&Credentials {
                        team_id: "TUNUSABLE".into(),
                        user_id: "UFAKE".into(),
                        token: flek_core::Secret::new("xoxc-fake"),
                        cookie: flek_core::Secret::new("xoxd-fake"),
                        origin: None,
                    })
                    .unwrap();
            }
            let path = dir.path().join("settings.json");
            let result = purge_stored(&credentials, &store, None, Ok(path.clone())).await;
            assert_eq!(result.is_ok(), !malformed);
            assert!(credentials.load("TUNUSABLE").unwrap().is_none());
            assert!(credentials.list().unwrap().is_empty());
            assert!(
                !flek_core::settings::Settings::load(&path)
                    .unwrap()
                    .desktop_import_on_start
            );
            // Repeating sign-out without a running session is a successful no-op.
            purge_stored(&credentials, &store, None, Ok(path))
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn unavailable_settings_path_still_purges_credentials() {
        let credentials = CredentialStore::with_backend(MemoryBackend::default());
        credentials
            .save(&Credentials {
                team_id: "TFAKE".into(),
                user_id: "UFAKE".into(),
                token: flek_core::Secret::new("xoxc-fake"),
                cookie: flek_core::Secret::new("xoxd-fake"),
                origin: None,
            })
            .unwrap();
        let store = flek_core::store::Store::open_in_memory().unwrap();
        let result = purge_stored(
            &credentials,
            &store,
            None,
            Err(flek_core::Error::Store("synthetic missing path".into())),
        )
        .await;
        assert!(result.is_err());
        assert!(credentials.load("TFAKE").unwrap().is_none());
        assert!(credentials.list().unwrap().is_empty());
    }
}

#[tauri::command]
pub fn account_origin(state: State<'_, AppState>) -> String {
    use flek_core::auth::OriginKind;
    let session = state.session.lock().expect("session lock");
    match session
        .as_ref()
        .and_then(|s| s.credentials.origin.as_ref())
        .map(|o| &o.kind)
    {
        Some(OriginKind::Desktop { .. }) => "desktop",
        Some(OriginKind::Webview) => "webview",
        _ => "unknown",
    }
    .into()
}
