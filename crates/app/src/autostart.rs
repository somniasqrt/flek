//! Launch at login (a LaunchAgent on macOS), started with `--hidden` so Flek
//! comes up in the menu bar only.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

pub fn is_enabled(app: &AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

pub fn set(app: &AppHandle, on: bool) -> Result<(), String> {
    let result = if on {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    };
    result.map_err(|e| e.to_string())
}
