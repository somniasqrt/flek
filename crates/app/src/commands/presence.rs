//! Presence IPC adapters.

use super::{text, CmdResult};
use crate::state::{AppState, Status};
use flek_core::store::{directory, emoji};
use tauri::State;

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

/// DB-only state and status fields for the bounded set shown by the UI.
#[tauri::command]
pub async fn presence_for(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> CmdResult<Vec<flek_core::store::presence::Presence>> {
    state
        .store
        .read(move |c| flek_core::store::presence::for_users(c, &ids))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn statuses_for(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> CmdResult<std::collections::HashMap<String, flek_core::store::directory::CustomStatus>> {
    state
        .store
        .read(move |c| directory::statuses_for(c, &ids))
        .await
        .map_err(text)
}

#[tauri::command]
pub fn subscribe_presence(state: State<'_, AppState>, ids: Vec<String>) {
    let Status::Ready { user_id, .. } = state.status() else {
        return;
    };
    let ids = flek_core::sync::presence::subscription(ids, &user_id);
    state.presence.send_if_modified(|current| {
        if *current == ids {
            false
        } else {
            *current = ids;
            true
        }
    });
}

fn presence_session(
    state: &AppState,
) -> CmdResult<(std::sync::Arc<flek_core::api::Client>, String)> {
    match (state.client(), state.status()) {
        (Some(client), Status::Ready { user_id, .. }) => Ok((client, user_id)),
        _ => Err("not signed in to Slack".into()),
    }
}

#[tauri::command]
pub async fn set_status(
    state: State<'_, AppState>,
    status: flek_core::api::CustomStatus,
) -> CmdResult<()> {
    let (client, me) = presence_session(&state)?;
    flek_core::sync::presence::set_status(&client, &state.store, &state.bus, &me, status)
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn clear_status(state: State<'_, AppState>) -> CmdResult<()> {
    set_status(
        state,
        flek_core::api::CustomStatus {
            status_text: String::new(),
            status_emoji: String::new(),
            status_expiration: 0,
        },
    )
    .await
}

#[tauri::command]
pub async fn set_presence(state: State<'_, AppState>, away: bool) -> CmdResult<()> {
    let (client, me) = presence_session(&state)?;
    let value = if away {
        flek_core::api::SetPresence::Away
    } else {
        flek_core::api::SetPresence::Auto
    };
    flek_core::sync::presence::set_presence(&client, &state.store, &state.bus, &me, value)
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn snooze(state: State<'_, AppState>, minutes: u32) -> CmdResult<()> {
    let (client, me) = presence_session(&state)?;
    flek_core::sync::presence::snooze(&client, &state.store, &state.bus, &me, Some(minutes))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn end_snooze(state: State<'_, AppState>) -> CmdResult<()> {
    let (client, me) = presence_session(&state)?;
    flek_core::sync::presence::snooze(&client, &state.store, &state.bus, &me, None)
        .await
        .map_err(text)
}
