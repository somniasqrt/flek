//! IPC commands for the UI. Thin: they read the database through `flek-core`
//! (reader connections, so they never wait for the sync engine).

use flek_core::store::directory::{self, ConversationRow};
use flek_core::store::emoji;
use flek_core::store::messages::{self, HistoryRow};
use flek_core::store::search::{self, Hit, Sort};
use flek_core::sync::live::Progress;
use flek_core::sync::send::{self, Outgoing, Sent};
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

/// Most results one search returns.
const MAX_HITS: usize = 200;

/// Marks around matched terms in snippets: control characters, which never
/// occur in message text, so the UI can split on them without escaping.
const MARK_OPEN: &str = "\u{2}";
const MARK_CLOSE: &str = "\u{3}";

/// Full-text search over the local archive. `query` takes the same filters
/// as `flek search` (`in:#channel from:@user has:link before:… after:…`).
#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    recent: bool,
    limit: usize,
) -> CmdResult<Vec<Hit>> {
    let Some(team) = state.team_id() else {
        return Ok(Vec::new());
    };
    // A query error is the user's typo: show the reason, not the layer.
    let q = search::parse(&query).map_err(|e| match e {
        flek_core::Error::Store(reason) => reason,
        e => e.to_string(),
    })?;
    let sort = if recent {
        Sort::Recent
    } else {
        Sort::Relevance
    };
    let limit = limit.clamp(1, MAX_HITS);
    state
        .store
        .read(move |c| search::search(c, &team, &q, sort, limit, (MARK_OPEN, MARK_CLOSE)))
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

/// Workspace emoji: `name → cached image hash` (served at
/// `flekfile://localhost/link/<hash>`) and aliases of standard emoji.
#[tauri::command]
pub async fn custom_emoji(state: State<'_, AppState>) -> CmdResult<emoji::CustomEmoji> {
    let Some(team) = state.team_id() else {
        return Ok(Default::default());
    };
    state
        .store
        .read(move |c| emoji::custom_emoji(c, &team))
        .await
        .map_err(text)
}

/// One person's profile for the side panel; `None` for an unknown id.
#[tauri::command]
pub async fn profile(
    state: State<'_, AppState>,
    user_id: String,
) -> CmdResult<Option<directory::Profile>> {
    state
        .store
        .read(move |c| directory::user_profile(c, &user_id))
        .await
        .map_err(text)
}

/// Fetches a person's large photo into the image cache if missing (Slack's
/// public CDN, no session) and returns its hash for `flekfile://…/link/`.
#[tauri::command]
pub async fn profile_photo(
    state: State<'_, AppState>,
    user_id: String,
) -> CmdResult<Option<String>> {
    let cache = flek_core::files::default_cache_dir().map_err(text)?;
    flek_core::sync::files::fetch_profile_photo(&state.user_agent(), &state.store, &cache, &user_id)
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

/// Bytes of a cached PDF, for the in-app viewer. Goes over IPC rather than
/// `fetch(flekfile://…)`: reading a custom-scheme response from the page is
/// a cross-origin request WebKit refuses (images only display, so they can
/// keep using the scheme).
#[tauri::command]
pub async fn pdf_data(
    state: State<'_, AppState>,
    file_id: String,
) -> CmdResult<tauri::ipc::Response> {
    let file = state
        .store
        .read(move |c| messages::cached_file(c, &file_id))
        .await
        .map_err(text)?
        .ok_or("unknown file")?;
    if file.mimetype.as_deref() != Some("application/pdf") {
        return Err("not a PDF".into());
    }
    let path = file.local_path.ok_or("the file is not downloaded yet")?;
    let path = crate::files::inside_cache(std::path::Path::new(&path))
        .ok_or("the file is outside the cache")?;
    let bytes = tokio::fs::read(path).await.map_err(text)?;
    Ok(tauri::ipc::Response::new(bytes))
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

/// Sends a message as the user. Slack's copy is written to the database and
/// announced like any other change; the UI shows it from there and matches
/// it to its optimistic copy by `client_msg_id`.
#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    conversation_id: String,
    text: String,
    thread_ts: Option<String>,
    broadcast: Option<bool>,
    client_msg_id: Option<String>,
    blocks: Option<Vec<serde_json::Value>>,
) -> CmdResult<Sent> {
    let (Some(client), Status::Ready { user_id, .. }) = (state.client(), state.status()) else {
        return Err("not signed in to Slack".into());
    };
    let out = Outgoing {
        conversation_id,
        blocks,
        text,
        thread_ts,
        broadcast: broadcast.unwrap_or(false),
        client_msg_id,
    };
    let sent = send::send(&client, &state.store, &user_id, out)
        .await
        .map_err(|e| flek_core::api::chat::describe_send_error(&e))?;
    if let Some(change) = sent.change.clone() {
        let _ = state.bus.send(change);
    }
    Ok(sent)
}

/// Change only the signed-in user's reaction; publish after it is stored.
#[tauri::command]
pub async fn set_reaction(
    state: State<'_, AppState>,
    conversation_id: String,
    ts: String,
    name: String,
    on: bool,
) -> CmdResult<()> {
    let (Some(client), Status::Ready { user_id, .. }) = (state.client(), state.status()) else {
        return Err("not signed in to Slack".into());
    };
    let applied = flek_core::sync::reactions::set(
        &client,
        &state.store,
        &user_id,
        &conversation_id,
        &ts,
        &name,
        on,
    )
    .await
    .map_err(text)?;
    if let flek_core::sync::live::Applied::Changed(change) = applied {
        let _ = state.bus.send(change);
    }
    Ok(())
}
