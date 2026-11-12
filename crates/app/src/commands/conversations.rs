//! Conversations IPC adapters.

use super::{text, CmdResult};
use crate::state::AppState;
use flek_core::store::directory::{self, ConversationRow};
use tauri::State;

use super::MAX_PAGE;

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

#[tauri::command]
pub async fn team_icon(state: State<'_, AppState>) -> CmdResult<Option<String>> {
    let Some(team) = state.team_id() else {
        return Ok(None);
    };
    state
        .store
        .read(move |c| directory::team_icon(c, &team))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn direct_messages(
    state: State<'_, AppState>,
) -> CmdResult<Vec<flek_core::rail::DirectMessage>> {
    let Some(team) = state.team_id() else {
        return Ok(Vec::new());
    };
    state
        .store
        .read(move |c| flek_core::rail::direct_messages(c, &team))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn activity(
    state: State<'_, AppState>,
    tab: String,
    unread: bool,
    limit: usize,
    offset: usize,
) -> CmdResult<flek_core::rail::ActivityPage> {
    let team = state.team_id().ok_or("no stored workspace")?;
    state
        .store
        .read(move |c| {
            flek_core::rail::activity(c, &team, &tab, unread, limit.clamp(1, MAX_PAGE), offset)
        })
        .await
        .map_err(text)
}

/// Local Activity read state: an opened Activity item or a viewed thread.
#[tauri::command]
pub async fn activity_seen(
    state: State<'_, AppState>,
    conversation_id: String,
    thread_ts: Option<String>,
    ts: String,
) -> CmdResult<()> {
    let id = conversation_id.clone();
    state
        .store
        .call(move |c| flek_core::rail::seen(c, &id, thread_ts.as_deref(), &ts))
        .await
        .map_err(text)?;
    let _ = state
        .bus
        .send(flek_core::sync::live::Change::ReadMarker { conversation_id });
    Ok(())
}

#[tauri::command]
pub async fn mark_read(
    state: State<'_, AppState>,
    conversation_id: String,
    ts: String,
) -> CmdResult<()> {
    let client = state.client().ok_or("Not connected")?;
    flek_core::sync::read::mark(&client, &state.store, &state.bus, &conversation_id, &ts)
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn conversation_preference(
    state: State<'_, AppState>,
    conversation_id: String,
    starred: Option<bool>,
    notification: Option<String>,
) -> CmdResult<()> {
    flek_core::sync::conversation::preference(
        &state.store,
        &state.bus,
        &conversation_id,
        starred,
        notification,
    )
    .await
    .map_err(text)
}

#[tauri::command]
pub async fn dismiss_conversation(
    state: State<'_, AppState>,
    conversation_id: String,
) -> CmdResult<()> {
    let client = state.client().ok_or("Not connected")?;
    flek_core::sync::conversation::dismiss(&client, &state.store, &state.bus, &conversation_id)
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn conversation_link(
    state: State<'_, AppState>,
    conversation_id: String,
) -> CmdResult<Option<String>> {
    state
        .store
        .read(move |c| flek_core::views::conversation_link(c, &conversation_id))
        .await
        .map_err(text)
}
