//! Message action IPC: orchestration and persistence belong to core.
use super::{text, CmdResult};
use crate::state::{AppState, Status};
use flek_core::{store::message_actions as cache, sync::message_actions as actions};
use tauri::State;

#[tauri::command]
pub async fn message_action_state(
    state: State<'_, AppState>,
    conversation_id: String,
    ts: String,
) -> CmdResult<cache::State> {
    let Status::Ready { user_id, .. } = state.status() else {
        return Err("not signed in to Slack".into());
    };
    state
        .store
        .read(move |c| cache::state(c, &conversation_id, &ts, &user_id))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn message_permalink(
    state: State<'_, AppState>,
    conversation_id: String,
    ts: String,
) -> CmdResult<String> {
    let client = state.client();
    actions::permalink(client.as_deref(), &state.store, &conversation_id, &ts)
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn mark_unread(
    state: State<'_, AppState>,
    conversation_id: String,
    ts: String,
) -> CmdResult<String> {
    let client = state.client().ok_or("Not connected")?;
    flek_core::sync::read::unread(&client, &state.store, &state.bus, &conversation_id, &ts)
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn save_message(
    state: State<'_, AppState>,
    conversation_id: String,
    ts: String,
    on: bool,
) -> CmdResult<()> {
    let Status::Ready { user_id, .. } = state.status() else {
        return Err("not signed in to Slack".into());
    };
    let client = state.client();
    actions::save(
        client.as_deref(),
        &state.store,
        &state.bus,
        &conversation_id,
        &ts,
        &user_id,
        on,
    )
    .await
    .map_err(text)
}

#[tauri::command]
pub async fn pin_message(
    state: State<'_, AppState>,
    conversation_id: String,
    ts: String,
    on: bool,
) -> CmdResult<()> {
    let Status::Ready { user_id, .. } = state.status() else {
        return Err("not signed in to Slack".into());
    };
    let client = state.client().ok_or("Not connected")?;
    actions::pin(
        &client,
        &state.store,
        &state.bus,
        &conversation_id,
        &ts,
        &user_id,
        on,
    )
    .await
    .map_err(text)
}

#[tauri::command]
pub async fn forward_message(
    state: State<'_, AppState>,
    conversation_id: String,
    ts: String,
    destination: String,
    comment: String,
) -> CmdResult<flek_core::sync::send::Sent> {
    let Status::Ready {
        team_id,
        user_id,
        team,
        user,
    } = state.status()
    else {
        return Err("not signed in to Slack".into());
    };
    let client = state.client().ok_or("Not connected")?;
    let me = flek_core::api::AuthTest {
        team_id,
        user_id,
        team,
        user,
        url: String::new(),
        enterprise_id: None,
    };
    let sent = actions::forward(
        &client,
        &state.store,
        &me,
        &conversation_id,
        &ts,
        &destination,
        &comment,
    )
    .await
    .map_err(text)?;
    if let Some(change) = sent.change.clone() {
        let _ = state.bus.send(change);
    }
    let _ = state.bus.send(flek_core::sync::live::Change::Directory);
    Ok(sent)
}
