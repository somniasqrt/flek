//! Flek desktop: a thin Tauri shell over `flek-core`. Business logic lives in
//! the core; this crate wires windows, the menu bar/tray and IPC to it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod commands;
mod files;
mod i18n;
mod login;
mod platform;
mod state;
mod tray;
mod window_size;

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
            platform::autostart_launcher(),
            Some(vec![HIDDEN_ARG]),
        ))
        .register_asynchronous_uri_scheme_protocol(files::SCHEME, files::handle)
        .manage(AppState::new(store))
        .invoke_handler(tauri::generate_handler![
            platform::platform,
            commands::get_settings,
            commands::set_settings,
            commands::storage_info,
            commands::clear_file_cache,
            commands::full_resync,
            commands::sign_out,
            commands::account_origin,
            commands::show_storage,
            commands::team_icon,
            commands::mark_read,
            commands::conversation_preference,
            commands::dismiss_conversation,
            commands::channel_info,
            commands::channel_directory,
            commands::channel_preview,
            commands::join_channel,
            commands::create_channel,
            commands::validate_channel_name,
            commands::conversation_link,
            commands::direct_messages,
            commands::activity,
            commands::browse_files,
            commands::fetch_file,
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
            commands::custom_emoji,
            commands::profile,
            commands::profile_photo,
            commands::open_file,
            commands::pdf_data,
            commands::open_url,
            commands::search,
            commands::send_message,
            commands::set_reaction,
            commands::presence_for,
            commands::statuses_for,
            commands::subscribe_presence,
            commands::set_status,
            commands::clear_status,
            commands::set_presence,
            commands::snooze,
            commands::end_snooze,
        ])
        .setup(move |app| {
            let handle = app.handle();
            window_size::restore(handle);
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
                window_size::capture(window);
                window_size::save(window.app_handle());
                if window
                    .app_handle()
                    .state::<AppState>()
                    .settings()
                    .keep_in_menu_bar
                {
                    hide_main(window.app_handle());
                } else {
                    window.app_handle().exit(0);
                }
            }
            WindowEvent::Moved(_)
            | WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. } => {
                window_size::capture(window);
            }
            WindowEvent::Destroyed if window.label() == login::LABEL => {
                login::closed(window.app_handle());
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error while building Flek");

    app.run(|app, event| match event {
        // Only "Quit" (with an exit code) ends Flek, not a closed window.
        RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
        RunEvent::Exit => {
            window_size::save(app);
            app.state::<AppState>().stop_engine();
        }
        _ => platform::on_reopen(app, &event),
    });
}

pub fn show_main(app: &AppHandle) {
    platform::set_dock_visible(app, true);
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
    platform::set_dock_visible(app, false);
}
