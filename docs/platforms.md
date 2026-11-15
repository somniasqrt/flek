# Platforms

Flek is built and used on macOS today. Windows support (ROADMAP M7.1.1–M7.1.5)
is implemented with CI builds; native Windows verification is still pending
(M7.1.6 / M7.2). Linux may follow. OS-specific presentation is collected in two
places so the rest of the code stays platform-neutral:

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

Sessions are stored with `keyring` (service `flek`, account = team id; `_teams`
holds the index). Target-specific Cargo features select `apple-native` on macOS,
`windows-native` on Windows and `sync-secret-service` + `crypto-rust` on Linux
(M7.1.3). Linux requires libdbus development packages to build and a running,
unlocked Secret Service on the user's session bus to persist credentials. It does
not silently fall back to an in-memory credential store on these targets. See the
[keyring feature definitions](https://docs.rs/crate/keyring/3.6.3/source/Cargo.toml).

## Slack desktop session import

`flek_core::auth::desktop_import::IMPORT_SUPPORTED` is true on macOS and Windows.
Linux continues directly to webview sign-in: its desktop cookie decryption is not
implemented. `candidate_roots(home)` is deterministic for fixtures;
`current_roots()` uses Windows Known Folders through `dirs` to support redirected
Roaming/Local AppData. Windows candidates, in priority order:

- `%APPDATA%\Slack` (normal desktop build).
- `%LOCALAPPDATA%\Packages\91750D7E.Slack_8she8kybcnzg4\LocalCache\Roaming\Slack`
  (Microsoft Store build).

Both use `Local Storage\leveldb`, `Local State`, and either `Network\Cookies`
or `Cookies`. LevelDB is privately copied and decoded with the existing
Latin-1/UTF-16 handling. The Windows importer decodes the base64
`os_crypt.encrypted_key`, requires its `DPAPI` marker, unwraps the rest with
current-user DPAPI and requires a 32-byte key. Cookie layout is version + 12-byte
nonce + AES-256-GCM ciphertext + 16-byte tag, with empty associated data. `v10`
and `v11` are accepted. A SHA-256 prefix must exactly match the selected host
before removal; unhashed older cookies are also accepted. The resulting session
must start with `xoxd-` and be printable ASCII; URL encoding is preserved.
Keys/plaintext buffers are zeroized on drop and errors never quote profile values.
The DPAPI call uses the safe [windows-dpapi wrapper](https://docs.rs/windows-dpapi/0.2.0/windows_dpapi/fn.decrypt_data.html);
core retains `forbid(unsafe_code)`.

The `v10` format follows [Chromium's Windows os_crypt implementation](https://raw.githubusercontent.com/chromium/chromium/130.0.6723.58/components/os_crypt/sync/os_crypt_win.cc).
TODO: Windows Slack profile paths, `v11`, log-derived client identity and current
cookie schema are **not verified on a real workspace**. Unknown/legacy formats
(including app-bound `v20`), unavailable keys and locked cookie files fail import
and leave webview sign-in available. Flek does not stop Slack, bypass file locks,
request elevation or bypass app-bound encryption. Fully quitting Slack may be
necessary before retrying import. No new Slack endpoint is involved.

## Windows desktop and installers (M7.1.1 / M7.1.4 / M7.1.5)

The existing single-instance plugin is registered first. An ordinary second launch
restores, unminimizes and focuses the main window; a duplicate Windows `--hidden`
login launch leaves the existing instance hidden. Window close respects the existing
keep-in-tray preference. Windows uses the colour application icon in the system
tray: left-click opens the main window, right-click opens the translated menu.
macOS keeps its template icon and menu-bar click behavior.

Launch-at-login remains opt-in through the existing settings/tray actions. The
autostart plugin registers the current executable with `--hidden` in the current
user's `Software\Microsoft\Windows\CurrentVersion\Run` key. It needs no administrator
rights. Windows uses native window decorations; Dock/reopen calls stay macOS-only.
Native behavior still needs M7.1.6 verification.

`.github/workflows/ci.yml` runs formatting, workspace clippy, Rust tests and UI
check/test/build on `macos-latest` and `windows-latest`. A dependent Windows job
runs Tauri's release build and uploads both MSI (WiX) and EXE (NSIS) installers as
`flek-windows-x64-installers` for 14 days. Pushes to main, pull requests and manual
workflow dispatches trigger it. Nothing is published as a release.

`tauri.windows.conf.json` overrides only Windows packaging: MSI/NSIS targets,
current-user NSIS installation and a WebView2 download bootstrapper if needed.
The installer may need network access for that runtime. Artifacts are unsigned;
code signing and native installer/upgrade checks remain release work. Existing
macOS app/DMG configuration is unchanged. To build on Windows with Rust/MSVC,
Visual Studio C++ Build Tools and Node installed: `npm ci`, then
`npm run tauri -- build --target x86_64-pc-windows-msvc --bundles msi,nsis --ci`.

### Tests and checks for M7.1

Automated: `crates/core/tests/desktop_import_windows.rs` covers `unwrap_key` (with an
injected unprotect function, so no DPAPI) and `decrypt_cookie` on any host, using the
independent Node-made vectors below plus synthetic AES-GCM records built in the test:
`v10`/`v11`, host-hash binding, wrong host/key, corrupted nonce/ciphertext/tag,
truncation, unknown versions (`v20`), non-session plaintext, error messages that never
quote inputs, and `read_cookie_with_host` against a temporary Cookies SQLite database.
Windows code is compile-checked from macOS with
`cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`
(needs `rustup target add x86_64-pc-windows-gnu` and `brew install mingw-w64`). This
does not run Windows tests; the CI Windows job does, but its first run is unverified.

Remaining recipe (not automated yet):

- `desktop_import::windows::unwrap_key`: pass JSON with base64 of
  `DPAPI` + synthetic bytes, inject an unprotect closure, assert it receives only
  the bytes after the marker and returns exactly 32 key bytes. Cover malformed
  JSON/types/base64, missing fields, empty/wrong marker, unwrap failure, 16/31/33
  byte keys and errors that never contain the input values. No OS store needed.
- `desktop_import::windows::decrypt_cookie`: key bytes `00..1f`, nonce bytes
  `00..0b`, empty AAD, synthetic value `xoxd-synthetic%2Fcookie`. Independent
  Node `crypto.createCipheriv('aes-256-gcm', key, nonce)` vectors (whole `v10`
  record in hex):

  ```text
  unhashed:
  763130000102030405060708090a0b3f6dae7fe896bb75f929f2ffd88a5d5fc5b5e85b9b123a3a636a525564f1b4119ddf5ace25854f
  SHA-256('.slack.com') + value:
  763130000102030405060708090a0bd61ea55f68b9e855e5b292b4a9274bd39c7f27c00711cc980423b90374aea2ca797fd69882b26bf600cc1a99e1e40d0aa83a0fe231bfc66967fb53fec807a71359cf21ac3fbc41
  ```

  Both must return that exact synthetic value for host `.slack.com`; changing
  the version bytes to `v11` must also work. Cover tag/ciphertext/nonce corruption,
  wrong key/host, truncated records at every boundary, unknown `v20`, empty,
  non-ASCII and non-session plaintext. Errors/Debug must not quote inputs.
- Temporary SQLite/LevelDB only: extend `tests/desktop_import.rs` to cover both
  Windows roots, old/new cookie locations, selected-host propagation, host
  filtering/expiry ordering, WAL copies, plain cookies without Local State,
  incomplete profiles and cleanup on failure. Keep the existing macOS CBC vectors.
  Windows-only synthetic DPAPI roundtrip can wrap the fixed key with user scope;
  never read an installed Slack profile or Credential Manager in automated tests.
- `CredentialStore` remains covered through `MemoryBackend`; use `cargo tree -e
  features --target <triple> -i keyring` to verify actual per-target backend features.
  No new wiremock route is needed; existing mocked `auth.test` validation/recovery
  coverage must continue to pass.
- Owner on real Windows: install each CI artifact; check initial launch, native
  frame/icon and WebView2, close-to-tray/left-click/right-click, minimized-window
  restore, ordinary duplicate launch, `--hidden` duplicate launch, opt-in login
  launch after reboot and disable. Verify session persistence/sign-out in Credential
  Manager, normal and Store import (retry after fully quitting Slack if locked),
  unsupported-key webview fallback, upgrade/uninstall, and the M7.2 webview checklist.
  Automated native installer/runtime verification is not claimed here.

## Left for M7

- Verify Windows session import against normal and Store builds (M7.1.6).
- Linux desktop session import (Secret Service Safe Storage key, Linux cookie format).
- Optional custom window buttons on Windows and Linux instead of the native title bar.
- Tray icon, autostart, single-instance and installer verification on a real Windows
  machine (M7.1.6); Linux tray behaviour varies by desktop environment.
- UI says "Preferences" everywhere; Windows and Linux may prefer "Settings".

## Download quarantine (M9.1)

On `cfg(target_os = "macos")`, core applies `com.apple.quarantine` with the safe
`xattr` crate immediately after a completed download's atomic rename. Saved copies
(`file get --to`) are explicitly marked too, even when copied from an older cache.
Open/reveal also marks old cached attachments before handing them to the OS. Failure
to mark a new download/copy fails the operation and removes the new target; an OS
open/reveal fails instead of proceeding without protection.

The value is `0081;<hex Unix seconds>;Flek;<random UUID v4>`. This is the conventional
browser-style download marker requested for Flek, with first-open approval pending.
The download bit is `0x0001`; the user-approved bit `0x0040` is deliberately **clear**
([Apple's WebKit SPI declarations](https://chromium.googlesource.com/external/github.com/WebKit/webkit/+/283c780414b7c24e4a911c823af665f53a15e8f7/Source/WebCore/PAL/pal/spi/mac/QuarantineSPI.h)).
The raw attribute format and the additional `0x0080` bit are not a documented public
Apple API contract. A generated UUID identifies the mark; Flek does not create a
LaunchServices quarantine database event. No Slack URL, token or cookie goes in it.
Gatekeeper's actual first-open UI still requires owner verification on macOS.

`LSFileQuarantineEnabled` is deliberately not enabled in the bundle. Apple documents
it as a default for **all files created by the app**, with path-pattern exclusions
([Launch Services keys](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/LaunchServicesKeys.html)).
Chromium uses this broad flag and then enriches OS quarantine metadata
([implementation](https://raw.githubusercontent.com/chromium/chromium/main/components/services/quarantine/quarantine_mac.mm)).
For Flek, the broad default could also mark the SQLite DB, WAL and settings. This is
an inference from that documented scope, not a verified runtime side effect; a safe
exclusion policy across configurable paths is more than a one-line bundle change.
M6.4 therefore retains explicit per-download marking and defers a bundle-wide layer.

`cfg(target_os = "windows")` TODO (M7.1): apply Zone.Identifier ADS to downloads and
saved copies. Quarantine is currently a no-op on Windows/Linux; the shared launch
allowlist and canonical cache containment still apply on every OS.

Direct OS opening requires an exact, case-insensitive extension/MIME pair from
`core::files::can_open`, using the resolved cached filename. Images (raster formats),
PDF, plain text/Markdown/CSV, the listed office formats and common audio/video are
allowed. SVG, HTML, scripts, shortcuts, disk images, installers, archives and unknown
or mismatched types only reveal in the platform file manager. This policy does not
inspect file contents or guarantee document safety; quarantine is an independent layer.

## Webview session storage (M9.3 / M9.4)

The bundle identifier is `app.flek.desktop`. Sign-in is incognito on every platform.
Installed Tauri 2.12.5 / Wry 0.57.1 source confirms that WKWebView uses
`nonPersistentDataStore`; cookies_for_url reads that same data_store.httpCookieStore.
WebView2 reads the window's CookieManager; WebKitGTK reads its website_data_manager's
cookie_manager. The capture script executes in the sign-in document's localStorage
before the cancelled completion navigation, and the native cookie read precedes
window destruction. No incognito cookie-read incompatibility was found in source;
runtime capture and persistence are owner checks, not headless test claims.

On macOS, upgrade cleanup and sign-out use the app's default WKWebsiteDataStore,
fetch all available website-data types, and remove only records whose displayName
is `slack.com` or a subdomain (case-insensitive, dot-boundary match). This covers
WebKit-managed cookies, local/session storage, caches, databases and service-worker
records for that domain, including old persistent signin data shared with main.
It does not delete directories or touch Slack desktop or Keychain. Separately, UI
sign-out clears app-origin message drafts and the in-memory outbox, preserving layout
and convenience preferences. WebKit owns the physical WebKit/HTTPStorages backing files; removal
is logical website-data removal, not a forensic secure erase of filesystem blocks.
The completion callback must fire before saving webview_cleanup_version=1 in
settings.json; failure/timeout leaves the marker pending and blocks session boot.

Tauri's portable clear_all_browsing_data API cannot target origins. The provisional
Windows/Linux (M7) fallback clears main's entire browser store on migration/sign-out,
also resetting local UI drafts/layout; SQLite, downloaded files and settings.json
are retained. Native per-origin cleanup on those platforms is future M7 work.

The macOS record-scoped API requires Objective-C calls absent from Tauri's safe
abstraction. `browsing_data::macos` is the sole unsafe-code exception, with main-thread,
object-lifetime and callback safety comments. App uses deny(unsafe_code) elsewhere;
core/CLI keep the workspace forbid. Dependencies were already present transitively.
See [Apple's record-scoped removal API](https://developer.apple.com/documentation/webkit/wkwebsitedatastore/removedata(oftypes:for:completionhandler:))
and [Tauri's window builder API](https://docs.rs/tauri/2.12.5/tauri/struct.WebviewWindowBuilder.html).
Native store cleanup and persistence need the owner checks in ROADMAP.md.


## Private local storage (M9.7)

On Unix (macOS/Linux), core creates app-owned data/cache directories with 0700 and
repairs existing ones. The DB, its `-wal`/`-shm` files, settings and cached downloads
are 0600. The DB is precreated and chmodded before SQLite opens it; sidecars are
chmodded after startup/migrations and writer operations, including checkpoints.
SQLite can recreate sidecars internally between those checks. The containing 0700
directory blocks other users throughout that interval, independent of sidecar mode
or process umask. No process-wide umask changes or unsafe calls are used.
Only Flek-owned paths are repaired; shared system data/cache parents are untouched.
Cache repair skips symlinks, while sensitive storage targets reject symlinks.

Windows has no Unix mode equivalent here: permission changes are cfg(unix).
M7 must apply/verify owner-only ACLs for data, caches, settings and session-copy
directories. Current Windows safety depends on the user's profile/temp ACLs.

Temporary Chromium copies are RAII TempDirs with prefix `flek-chromium-v1-`,
created 0700 on Unix with 0600 copied files. Normal success, errors and unwinding
remove them. LevelDB is explicitly deleted immediately after reading localConfig_v2,
before parsing, cookie decryption/Keychain prompts or network validation. Startup
cleanup only removes same-owner, non-symlink prefixed directories at least five
minutes old. Each new copy holds an OS file lock on `.flek-import.lock`; cleanup
uses a nonblocking lock and skips an active importer even beyond the age threshold.
SIGKILL releases the lock. Five minutes leaves ample grace for local directory/lock
creation while reducing crash-copy exposure from 24 hours; cleanup still requires
a later startup. Older unleased copies are protected only by the age threshold.
CLI cleanup errors are warnings and do not prevent other commands from running.
Old unprefixed `.tmp*` directories cannot be
safely distinguished from other applications' files and are not deleted.
Deletion is logical removal, not secure erasure of disk blocks.

## Public image networking (M9.6)

Public, session-free images (avatars, emoji, workspace icon, link-preview images)
may come from any HTTPS site on port 443: link previews point at the linked site
itself, and Gravatar avatars redirect to Slack's CDN. The boundary is the network,
not the host name: IP-literal hosts must be public, and every name resolves through
a resolver that hands only checked public IP addresses to reqwest's connector,
including IPv6/mapped checks. Redirects are followed up to five hops, each target
re-checked by the same URL policy and resolver. Environment proxies are disabled on
this client so proxy-side DNS cannot bypass the policy (corporate setups that require
an HTTP proxy lose these images; cached/fallback presentation remains). The
conservative IP policy may reject new special-purpose IPv6 allocations until
reviewed. The OS/network still controls routing and TLS trust.
