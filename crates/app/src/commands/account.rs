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
pub fn sign_in(app: AppHandle) -> CmdResult<()> {
    crate::login::open(&app).map_err(text)
}

#[tauri::command]
pub async fn sign_out(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let team = state.team_id().ok_or("not_signed_in")?;
    let _maintenance = state.maintenance.lock().await;
    state.stop_and_wait().await;
    let settings = flek_core::auth::sign_out(
        &state.creds,
        &team,
        &flek_core::settings::default_path().map_err(text)?,
    )
    .map_err(text)?;
    *state.settings.lock().map_err(text)? = settings;
    state.forget_session();
    state::set_status(
        &app,
        Status::SignIn {
            desktop: false,
            reason: None,
        },
    );
    Ok(())
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
