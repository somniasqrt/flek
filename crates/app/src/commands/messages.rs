//! Messages IPC adapters.

use super::{text, CmdResult};
use crate::state::{AppState, Status};
use flek_core::store::messages::{self, HistoryRow};
use flek_core::store::search::{self, Hit, Sort};
use flek_core::sync::send::{self, Outgoing, Sent};
use tauri::State;

use super::MAX_PAGE;

/// A page of the timeline. Without `before` the conversation was just
/// opened, so the sync engine fetches it next.
#[tauri::command]
pub async fn messages(
    state: State<'_, AppState>,
    conversation_id: String,
    before: Option<String>,
    limit: usize,
    local_only: Option<bool>,
) -> CmdResult<Vec<HistoryRow>> {
    if before.is_none() && !local_only.unwrap_or(false) {
        state.priority.push(&conversation_id);
    }
    let limit = limit.clamp(1, MAX_PAGE);
    state
        .store
        .read(move |c| messages::page(c, &conversation_id, before.as_deref(), limit))
        .await
        .map_err(text)
}

/// Local-only target window, forward pagination and date lookup.
#[tauri::command]
pub async fn timeline_window(
    state: State<'_, AppState>,
    conversation_id: String,
    target: String,
    limit: usize,
) -> CmdResult<messages::TimelineWindow> {
    state
        .store
        .read(move |c| {
            messages::timeline_window(c, &conversation_id, &target, limit.clamp(2, MAX_PAGE))
        })
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn timeline_after(
    state: State<'_, AppState>,
    conversation_id: String,
    after: String,
    limit: usize,
) -> CmdResult<Vec<HistoryRow>> {
    state
        .store
        .read(move |c| {
            messages::timeline_after(c, &conversation_id, &after, limit.clamp(1, MAX_PAGE))
        })
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn date_target(
    state: State<'_, AppState>,
    conversation_id: String,
    start: String,
    end: String,
) -> CmdResult<messages::DateTarget> {
    state
        .store
        .read(move |c| messages::date_target(c, &conversation_id, &start, &end))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn timeline_range(
    state: State<'_, AppState>,
    conversation_id: String,
    start: String,
    end: String,
) -> CmdResult<Vec<HistoryRow>> {
    state
        .store
        .read(move |c| messages::timeline_range(c, &conversation_id, &start, &end))
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
