# Platforms

Flek is built and used on macOS today. Windows is planned (ROADMAP M7.1) and Linux
may follow. Everything OS-specific is collected in two places so the rest of the code
stays platform-neutral:

- UI: `ui/src/platform.ts`. `initPlatform()` asks the app shell (`platform` command,
  `std::env::consts::OS`) before the first render, falls back to the navigator in the
  browser preview (`preview.html?platform=windows|linux|mac` forces one), and sets
  `html[data-platform]`.
- App shell: `crates/app/src/platform.rs` (Dock, reopen, autostart flavour, menu wording).

## What differs

| Area | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Shortcuts | `⌘` (`⌘⇧K`) | `Ctrl` (`Ctrl+Shift+K`) | same as Windows |
| File manager wording | Finder | File Explorer | file manager |
| Credential store wording | Keychain | Credential Manager | the system keyring |
| Tray wording | menu bar | system tray | system tray |
| Window chrome | overlay title bar, traffic lights (`titleBarStyle: Overlay`) | native title bar | native title bar |
| UI font | `-apple-system` | Segoe UI Variable / Segoe UI | system-ui, Cantarell, Ubuntu, Noto Sans |
| Monospace | SF Mono, Menlo | Cascadia Mono, Consolas | DejaVu Sans Mono |
| Emoji font | Apple Color Emoji | Segoe UI Emoji | Noto Color Emoji |
| Native app menu | "Preferences…", Hide items | "Settings", no Hide items | same as Windows |
| Dock / reopen | hidden Dock icon while only the menu bar item is alive; Dock click reopens | n/a (tray icon) | n/a (tray icon) |
| Custom scheme URL | `flekfile://localhost/` | `http://flekfile.localhost/` (WebView2) | `flekfile://localhost/` |

Per-OS sentences are whole translated strings (en + uk) chosen with `pick({mac, windows,
linux})`; helper names (`fileManagerName`, `credentialStoreName`, `trayName`) exist for
non-translated uses.

Theme needs no per-OS code: CSS follows `prefers-color-scheme` plus the `data-theme`
override.

## Paths

Resolved with the `dirs` crate:

| What | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Database `flek/flek.db` (and `settings.json` next to it) | `~/Library/Application Support` | `%APPDATA%` | `$XDG_DATA_HOME` or `~/.local/share` |
| File cache `flek/files` | `~/Library/Caches` | `%LOCALAPPDATA%` | `$XDG_CACHE_HOME` or `~/.cache` |
| Window position/size | Tauri `app_config_dir` | same | same |

## Credentials

Sessions are stored with `keyring` (service `flek`, account = team id). Only the macOS
backend (`apple-native`) is enabled in `Cargo.toml`. On Windows and Linux `keyring` needs
`windows-native` / a Secret Service feature, otherwise it silently uses an in-memory mock
store (ROADMAP M7.1.3).

## Slack desktop session import

`flek_core::auth::desktop_import::IMPORT_SUPPORTED` is true on macOS only. Elsewhere the
app never reports the desktop app as installed, so onboarding goes straight to the webview
sign-in and the macOS Keychain wording is not shown. Profile directory candidates are
already per OS (`candidate_roots`: macOS, `%APPDATA%\Slack` and the Store build, `~/.config/Slack`
plus Snap and Flatpak); reading the token (LevelDB) works everywhere, the cookie needs:

- Windows: DPAPI-protected AES key from `Local State`, AES-256-GCM cookies (M7.1.2).
- Linux: Secret Service password for "Slack Safe Storage", `v11` cookies (M7).

## Left for M7

- Credential backends for `keyring` on Windows and Linux (M7.1.3).
- Session import for Windows and Linux (M7.1.2).
- Optional custom window buttons on Windows and Linux instead of the native title bar.
- Tray icon, autostart and single-instance verification on a real Windows machine
  (M7.1.4, M7.1.6); Linux tray behaviour varies by desktop environment.
- UI says "Preferences" everywhere; Windows and Linux may prefer "Settings".
