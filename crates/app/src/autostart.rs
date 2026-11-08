//! Launch at login (a LaunchAgent on macOS), started with `--hidden` so Flek
//! comes up in the menu bar only.

use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

/// Marks that launch at login was turned on once; later the user decides.
const FLAG_FILE: &str = "autostart-initialized";

pub fn is_enabled(app: &AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

pub fn set(app: &AppHandle, on: bool) {
    let result = if on {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    };
    if let Err(e) = result {
        tracing::warn!("cannot change launch at login: {e}");
    }
}

/// Turns launch at login on after the first successful sign-in, once. Debug
/// builds never register themselves (the LaunchAgent would point at
/// `target/debug`).
pub fn enable_once(app: &AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let flag = dir.join(FLAG_FILE);
    if flag.exists() {
        return;
    }
    set(app, true);
    let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&flag, b""));
    crate::tray::refresh(app);
}
