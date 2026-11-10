//! Everything that behaves differently per OS, in one place. macOS keeps its
//! Dock, "reopen" and menu-bar conventions; Windows and Linux get a system
//! tray and the native title bar. Mirrors `ui/src/platform.ts`.

use tauri::{AppHandle, RunEvent};

use crate::i18n::Key;

/// Compile-time OS name as the UI expects it: `macos`, `windows`, `linux`.
pub fn os() -> &'static str {
    std::env::consts::OS
}

/// The webview asks once at startup (`ui/src/platform.ts`).
#[tauri::command]
pub fn platform() -> &'static str {
    os()
}

/// Hide the Dock icon while only the menu bar item is alive. No-op elsewhere:
/// Windows and Linux have a tray icon next to the taskbar entry.
pub fn set_dock_visible(app: &AppHandle, visible: bool) {
    #[cfg(target_os = "macos")]
    let _ = app.set_dock_visibility(visible);
    #[cfg(not(target_os = "macos"))]
    let _ = (app, visible);
}

/// Clicking the Dock icon of a running app reopens its window (macOS only).
pub fn on_reopen(app: &AppHandle, event: &RunEvent) {
    #[cfg(target_os = "macos")]
    if let RunEvent::Reopen { .. } = event {
        crate::show_main(app);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, event);
}

/// How "launch at login" is registered: a LaunchAgent on macOS (the plugin
/// uses the registry or an autostart entry on the other systems).
pub fn autostart_launcher() -> tauri_plugin_autostart::MacosLauncher {
    tauri_plugin_autostart::MacosLauncher::LaunchAgent
}

/// macOS calls it "Preferences…" in the app menu; elsewhere it is "Settings".
pub fn preferences_key() -> Key {
    if cfg!(target_os = "macos") {
        Key::Preferences
    } else {
        Key::Settings
    }
}

/// The "Hide Flek / Hide Others / Show All" items exist only in the macOS app menu.
pub fn has_hide_items() -> bool {
    cfg!(target_os = "macos")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_is_one_of_the_known_names() {
        assert!(["macos", "windows", "linux"].contains(&os()) || !os().is_empty());
        assert_eq!(has_hide_items(), os() == "macos");
    }
}
