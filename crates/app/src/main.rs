//! Flek desktop: a thin Tauri shell over `flek-core`. Business logic lives in
//! the core; this crate wires windows, the menu bar and IPC to it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod commands;
mod files;
mod login;
mod state;
mod tray;

use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tracing_subscriber::EnvFilter;

use state::AppState;

const MAIN: &str = "main";
/// Passed by the login item: start in the menu bar, no window.
const HIDDEN_ARG: &str = "--hidden";

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn,flek=info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let hidden = std::env::args().any(|a| a == HIDDEN_ARG);
    let store = flek_core::store::default_path()
        .and_then(|p| flek_core::store::Store::open(&p))
        .expect("cannot open the Flek database");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .register_asynchronous_uri_scheme_protocol(files::SCHEME, files::handle)
        .manage(AppState::new(store))
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::progress,
            commands::conversations,
            commands::messages,
            commands::message,
            commands::thread,
            commands::import_desktop,
            commands::sign_in,
            commands::people,
            commands::avatars,
            commands::open_file,
            commands::pdf_data,
            commands::open_url,
        ])
        .setup(move |app| {
            let handle = app.handle();
            tray::create(handle)?;
            state::forward_changes(handle);
            if hidden {
                hide_main(handle);
            } else {
                show_main(handle);
            }
            tauri::async_runtime::spawn(state::boot(handle.clone()));
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the window keeps Flek syncing in the menu bar.
            WindowEvent::CloseRequested { api, .. } if window.label() == MAIN => {
                api.prevent_close();
                hide_main(window.app_handle());
            }
            WindowEvent::Destroyed if window.label() == login::LABEL => {
                login::closed(window.app_handle());
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error while building Flek");

    app.run(|app, event| match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main(app),
        // Only "Quit" (with an exit code) ends Flek, not a closed window.
        RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
        RunEvent::Exit => app.state::<AppState>().stop_engine(),
        _ => {}
    });
}

pub fn show_main(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_dock_visibility(true);
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn hide_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_dock_visibility(false);
}
