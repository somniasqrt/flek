//! Channels IPC adapters.

use super::{text, CmdResult};
use crate::state::AppState;
use flek_core::store::directory::{self, ConversationRow};
use tauri::State;

#[tauri::command]
pub async fn channel_header(
    state: State<'_, AppState>,
    conversation_id: String,
) -> CmdResult<Option<flek_core::reads::ChannelHeader>> {
    state
        .store
        .read(move |c| flek_core::reads::channel_header(c, &conversation_id))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn channel_info(
    state: State<'_, AppState>,
    conversation_id: String,
    fresh: bool,
) -> CmdResult<flek_core::reads::ChannelInfo> {
    if fresh {
        let client = state.client().ok_or("Not connected")?;
        flek_core::sync::channel_details::fresh(&client, &state.store, &conversation_id)
            .await
            .map_err(text)?;
        // Other DB readers, including the header, can now show the cached snapshot.
        let _ = state.bus.send(flek_core::sync::live::Change::Directory);
    }
    state
        .store
        .read(move |c| flek_core::reads::channel_info(c, &conversation_id))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn channel_directory(
    state: State<'_, AppState>,
    refresh: bool,
    force: bool,
) -> CmdResult<flek_core::store::channel_browser::Directory> {
    let team = state.team_id().ok_or("not_signed_in")?;
    if refresh {
        let client = state.client().ok_or("not_connected")?;
        flek_core::sync::channel_browser::refresh(&client, &state.store, &team, force)
            .await
            .map_err(text)?;
    }
    state
        .store
        .read(move |c| flek_core::store::channel_browser::read(c, &team))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn channel_preview(
    state: State<'_, AppState>,
    conversation_id: String,
    fresh: bool,
) -> CmdResult<ConversationRow> {
    let team = state.team_id().ok_or("not_signed_in")?;
    if fresh {
        let client = state.client().ok_or("not_connected")?;
        flek_core::sync::channel_browser::preview(&client, &state.store, &team, &conversation_id)
            .await
            .map_err(text)?;
    }
    state
        .store
        .read(move |c| {
            directory::list_conversations_including(c, &team, true)?
                .into_iter()
                .find(|row| row.id == conversation_id)
                .ok_or_else(|| flek_core::Error::Store("channel_not_found".into()))
        })
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn join_channel(
    state: State<'_, AppState>,
    conversation_id: String,
) -> CmdResult<flek_core::sync::channel_browser::ChannelAction> {
    let team = state.team_id().ok_or("not_signed_in")?;
    let client = state.client().ok_or("not_connected")?;
    flek_core::sync::channel_browser::join(
        &client,
        &state.store,
        &state.bus,
        &team,
        &conversation_id,
    )
    .await
    .map_err(text)
}

#[tauri::command]
pub async fn create_channel(
    state: State<'_, AppState>,
    name: String,
    private: bool,
    purpose: String,
) -> CmdResult<flek_core::sync::channel_browser::ChannelAction> {
    let team = state.team_id().ok_or("not_signed_in")?;
    let client = state.client().ok_or("not_connected")?;
    flek_core::sync::channel_browser::create(
        &client,
        &state.store,
        &state.bus,
        &team,
        &name,
        private,
        &purpose,
    )
    .await
    .map_err(text)
}

#[tauri::command]
pub fn validate_channel_name(name: String) -> CmdResult<String> {
    let normalized = flek_core::sync::channel_browser::normalize_name(&name);
    flek_core::sync::channel_browser::validate_name(&normalized).map_err(text)?;
    Ok(normalized)
}
