//! IPC commands for the UI. Thin: they read the database through `flek-core`
//! (reader connections, so they never wait for the sync engine).

use flek_core::store::directory::{self, ConversationRow};
use flek_core::store::messages::{self, HistoryRow};
use flek_core::sync::live::Progress;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::state::{self, AppState, Status};

/// Errors cross IPC as text; core errors never carry secrets.
type CmdResult<T> = Result<T, String>;

fn text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Largest page the UI may ask for.
const MAX_PAGE: usize = 500;

#[tauri::command]
pub fn status(state: State<'_, AppState>) -> Status {
    state.status()
}

#[tauri::command]
pub fn progress(state: State<'_, AppState>) -> Option<Progress> {
    state.progress()
}

#[tauri::command]
pub async fn conversations(state: State<'_, AppState>) -> CmdResult<Vec<ConversationRow>> {
    let Some(team) = state.team_id() else {
        return Ok(Vec::new());
    };
    state
        .store
        .read(move |c| directory::list_conversations(c, &team))
        .await
        .map_err(text)
}

/// A page of the timeline. Without `before` the conversation was just
/// opened, so the sync engine fetches it next.
#[tauri::command]
pub async fn messages(
    state: State<'_, AppState>,
    conversation_id: String,
    before: Option<String>,
    limit: usize,
) -> CmdResult<Vec<HistoryRow>> {
    if before.is_none() {
        state.priority.push(&conversation_id);
    }
    let limit = limit.clamp(1, MAX_PAGE);
    state
        .store
        .read(move |c| messages::page(c, &conversation_id, before.as_deref(), limit))
        .await
        .map_err(text)
}

/// One message, after a live change.
#[tauri::command]
pub async fn message(
    state: State<'_, AppState>,
    conversation_id: String,
    ts: String,
) -> CmdResult<Option<HistoryRow>> {
    state
        .store
        .read(move |c| messages::message(c, &conversation_id, &ts))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn thread(
    state: State<'_, AppState>,
    conversation_id: String,
    ts: String,
) -> CmdResult<Vec<HistoryRow>> {
    state
        .store
        .read(move |c| messages::thread(c, &conversation_id, &ts))
        .await
        .map_err(text)
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

/// `user id → display name`, for `<@U…>` mentions.
#[tauri::command]
pub async fn people(
    state: State<'_, AppState>,
) -> CmdResult<std::collections::HashMap<String, String>> {
    let Some(team) = state.team_id() else {
        return Ok(Default::default());
    };
    state
        .store
        .read(move |c| directory::people(c, &team))
        .await
        .map_err(text)
}

/// `user id → cached avatar hash` (served at `flekfile://localhost/link/<hash>`).
#[tauri::command]
pub async fn avatars(
    state: State<'_, AppState>,
) -> CmdResult<std::collections::HashMap<String, String>> {
    let Some(team) = state.team_id() else {
        return Ok(Default::default());
    };
    state
        .store
        .read(move |c| directory::avatars(c, &team))
        .await
        .map_err(text)
}

/// Opens a cached file with its default app. Takes the file id, not a path,
/// so the UI cannot open arbitrary files.
#[tauri::command]
pub async fn open_file(
    app: AppHandle,
    state: State<'_, AppState>,
    file_id: String,
) -> CmdResult<()> {
    let path = state
        .store
        .read(move |c| messages::file_local_path(c, &file_id))
        .await
        .map_err(text)?
        .ok_or("the file is not downloaded yet")?;
    app.opener().open_path(path, None::<&str>).map_err(text)
}

/// Opens a link from a message in the default browser (never in Flek's
/// own webview).
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    let parsed = tauri::Url::parse(&url).map_err(text)?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
        return Err(format!("not opening {} links", parsed.scheme()));
    }
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(text)
}
