//! Settings IPC adapters.

use super::{text, CmdResult};
use crate::state::{self, AppState};
use tauri::Manager;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn get_settings(app: AppHandle, state: State<'_, AppState>) -> flek_core::settings::Settings {
    let mut settings = state.settings();
    settings.launch_at_login = crate::autostart::is_enabled(&app);
    settings
}

#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: flek_core::settings::Settings,
) -> CmdResult<flek_core::settings::Settings> {
    use tauri::Emitter;
    settings.validate().map_err(text)?;
    let mut current = state.settings.lock().map_err(text)?;
    // Auth state is owned by core, never by a stale UI preferences snapshot.
    settings.desktop_import_on_start = current.desktop_import_on_start;
    let was_enabled = crate::autostart::is_enabled(&app);
    if settings.launch_at_login != was_enabled {
        crate::autostart::set(&app, settings.launch_at_login)?;
    }
    let path = flek_core::settings::default_path().map_err(text)?;
    if let Err(e) = settings.save(&path) {
        let _ = crate::autostart::set(&app, was_enabled);
        return Err(text(e));
    }
    *current = settings.clone();
    drop(current);
    crate::tray::refresh(&app);
    if let Some(window) = app.get_webview_window(crate::login::LABEL) {
        let _ = window.set_title(crate::i18n::t(settings.language, crate::i18n::Key::SignIn));
    }
    app.emit("flek://settings", settings.clone())
        .map_err(text)?;
    Ok(settings)
}

#[tauri::command]
pub async fn storage_info(
    state: State<'_, AppState>,
) -> CmdResult<flek_core::files::storage::StorageInfo> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        flek_core::files::storage::info(&store, &flek_core::files::default_cache_dir()?)
    })
    .await
    .map_err(text)?
    .map_err(text)
}

#[tauri::command]
pub fn show_storage(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let path = state.store.path().ok_or("no database path")?;
    app.opener().reveal_item_in_dir(path).map_err(text)
}

#[tauri::command]
pub async fn clear_file_cache(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let _maintenance = state.maintenance.lock().await;
    state.stop_and_wait().await;
    let result = flek_core::files::storage::clear(
        &state.store,
        &flek_core::files::default_cache_dir().map_err(text)?,
    )
    .await
    .map_err(text);
    let session = state.session.lock().map_err(text)?.clone();
    if let Some(session) = session {
        state::start_with_options(&app, session, false, true);
    }
    use tauri::Emitter;
    let _ = app.emit(
        state::CHANGE_EVENT,
        flek_core::sync::live::Change::Directory,
    );
    for conversation_id in result? {
        let _ = state
            .bus
            .send(flek_core::sync::live::Change::History { conversation_id });
    }
    Ok(())
}

#[tauri::command]
pub async fn full_resync(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let _maintenance = state.maintenance.lock().await;
    state.stop_and_wait().await;
    let session = state
        .session
        .lock()
        .map_err(text)?
        .clone()
        .ok_or("not_signed_in")?;
    state::start_with_options(&app, session, true, false);
    Ok(())
}
