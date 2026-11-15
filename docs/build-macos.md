# Building for macOS (M6.4.2)

Run from a checkout on macOS with Rust stable, Node 24 LTS with npm, and Apple's
Command Line Tools (`xcode-select --install`). Full Xcode is optional for native
builds when the Command Line Tools provide the SDK and signing tools. Install
the Rust targets before cross-compiling; a universal build needs both
`aarch64-apple-darwin` and `x86_64-apple-darwin` (`rustup target add …`).
No Slack session is needed to build. The script never launches Flek, invokes the
development Cargo runner, imports a certificate, or creates/unlocks a Keychain.
Ad-hoc signing uses no signing certificate. Developer ID signing uses an identity
the owner or CI has already installed and made available to `codesign`.

## Local build

```sh
scripts/build-macos.sh
# Equivalent npm entry point:
npm run app:build:macos
# Optional explicit target:
scripts/build-macos.sh universal-apple-darwin
```

The script runs `npm ci` against the root `package-lock.json`, Tauri's
`beforeBuildCommand` (TypeScript check + Vite production build with
`--configLoader runner`), and release Rust compilation with `Cargo.lock`
enforced by `--locked`. Tauri packages and signs the `.app`, then creates the
DMG. `CI=true` disables Finder AppleScript decoration, so building needs neither
a logged-in desktop nor Finder Automation permission. The DMG still contains
Flek and an Applications symlink; automatic icon positioning is skipped.

In a worktree with dependencies already installed from the same lock file, use
`FLEK_SKIP_NPM_CI=1 scripts/build-macos.sh`. This also supports read-only shared
`node_modules` symlinks. Do not use this shortcut after dependency changes.
`CARGO_TARGET_DIR` is respected; parallel builds of the same target should use
separate target directories to avoid replacing each other's release artifacts.

Artifacts are below Cargo's target directory, using an explicit target even for
a native build:

```text
<target-dir>/<target>/release/bundle/macos/Flek.app
<target-dir>/<target>/release/bundle/dmg/Flek_<version>_<arch>.dmg
```

The DMG architecture suffix is `aarch64`, `x64` or `universal`. Without
`CARGO_TARGET_DIR`/Cargo configuration, `<target-dir>` is the repository's
`target/`. The script verifies the bundle signature and plist, prints sizes and
the DMG SHA-256. It stops on failure; a failed DMG step may still leave a valid
`.app`. Restricted sandboxes can deny the disk-image creation/mount operations
that Tauri's DMG builder requires. Retry the same command in a normal terminal;
no app launch is needed to package it.

This is a repeatable locked-dependency workflow, not a claim of byte-identical
outputs: Rust/Node/SDK versions, paths, signing timestamps and DMG filesystem
metadata affect the bytes. Record those tool versions and the printed checksum
with each release. No updater signatures or release publishing are configured.

## Signing and notarization

By default `APPLE_SIGNING_IDENTITY` is `-` (ad-hoc). No identity discovery or
development certificate fallback occurs. This creates a locally verifiable
signature, but does not establish a trusted publisher or pass Gatekeeper for
Internet distribution. Ad-hoc builds may prompt for Keychain access again after
each rebuild; use a stable Developer ID identity for releases.

To sign with a provisioned certificate, set `APPLE_SIGNING_IDENTITY` to its
complete `Developer ID Application: …` name. This is an opt-in path that accesses
the signing identity through `codesign`; no application credentials are read.
The script deliberately ignores `APPLE_CERTIFICATE` and
`APPLE_CERTIFICATE_PASSWORD`; certificate provisioning belongs outside the build.

Notarization is disabled unless `FLEK_NOTARIZE=1`, even if Apple credentials
already exist in the calling shell. For distribution, enable it with Developer
ID signing and **one** authentication method:

| Method | Environment variables |
| --- | --- |
| App Store Connect API | `APPLE_API_KEY` (key ID), `APPLE_API_ISSUER`, `APPLE_API_KEY_PATH` (readable `.p8` outside the checkout) |
| Apple ID | `APPLE_ID`, `APPLE_PASSWORD` (app-specific password), `APPLE_TEAM_ID` |

Supply values through your secret manager or protected CI environment. Do not
put private keys/passwords in the repository, shell command history or logs, and
do not enable shell tracing. The script does not print authentication values.
API authentication takes precedence if `APPLE_API_KEY` is set; incomplete
authentication or ad-hoc notarization fails before compilation. Tauri submits
the signed app, waits for Apple's result, staples the app ticket, then packages
that app in the DMG and signs the DMG. The script also validates the app ticket.
It does not separately submit/staple the outer DMG. A signed, unnotarized build
is useful for inspection but is not ready for public distribution.

See the official [Tauri signing guide](https://v2.tauri.app/distribute/sign/macos/)
and [environment reference](https://v2.tauri.app/reference/environment-variables/).

## Bundle metadata and entitlements

- Product `Flek`, stable identifier `app.flek.desktop`, category `Productivity`;
  M6.4.1's existing `icons/icon.icns` is used without regeneration.
- Minimum macOS **13.0** matches the frontend's `safari16` baseline with the
  system WebKit shipped with Ventura. Older macOS with upgraded Safari is not
  a supported baseline. Native behavior on macOS 13 still needs owner QA.
- `Info.plist` adds English/Ukrainian localizations, development region `en`
  and high-resolution display support. Tauri supplies executable/name/version,
  category, icon and minimum-system-version entries.
- Hardened runtime is explicitly on. `Entitlements.plist` is intentionally an
  empty dictionary: the native Rust executable does not use JIT or third-party
  injected libraries. WKWebView uses system-managed web-content processes.
  There is no `get-task-allow`, unsigned executable memory, disabled library
  validation, camera/microphone permission or Apple Events exception.
  Runtime verification is required before adding any exception; follow
  [Apple's hardened-runtime guidance](https://developer.apple.com/documentation/security/hardened-runtime).
- App Sandbox is not enabled. Existing desktop-session import, private local
  archive and saved-file paths retain their current behavior. Traditional
  macOS Keychain Services access uses the existing user's authorization/ACL;
  it needs no usage-description plist key or shared Keychain access group.
- OS notifications are not implemented yet (M6.2.1). Do not add APNs
  entitlements or a fictitious notification usage-description key. The future
  notification implementation must request its authorization at runtime.
- No incoming OS URL scheme exists. `flekfile:` is only a webview protocol;
  outgoing `slack://channel` links belong to the installed Slack client. Neither
  belongs in `CFBundleURLTypes`. The sign-in capture URL is an intercepted
  navigation, not an OS deep link.
- `LSFileQuarantineEnabled` stays absent per M9.1.5. Downloads/copies retain
  explicit per-file quarantine; a global flag could quarantine DB/WAL/settings.
  See [platform notes](platforms.md).

## Verification recipe for the reviewer

No new tests are included in this packaging step. No core functions, models,
HTTP API or SQLite paths change, so wiremock/temp SQLite additions are unnecessary.

For automated coverage, inspect the JSON/plists and exercise the script with
stubbed build tools in a temporary directory: default ad-hoc mode, each accepted
architecture, invalid arguments, missing tools, locked dependency failures,
`FLEK_SKIP_NPM_CI`, rejecting ad-hoc notarization, incomplete Apple authentication,
API precedence, opt-out credential removal, failed build/signature/ticket checks,
and missing artifact handling. Assert no auth values are printed and no app
runner, `security` command or certificate import occurs. Artifact smoke checks
should assert the identifier, version, minimum OS, icon, category, localizations,
runtime signing flag and absence of quarantine/OS URL registration/extra
entitlements. Universal binaries need both architectures in `lipo -info`.

Owner-only checks on a machine/profile intended for runtime QA:

1. Build outside the sandbox, mount the DMG, verify Flek and Applications, copy
   to Applications, inspect the Dock/Finder/menu-bar icon and launch it.
2. Check English/Ukrainian menus, window/tray behavior, opt-in launch at login,
   webview sign-in, authorized desktop-session import/Keychain prompts,
   offline archive, document workers/viewers and external link opening.
3. Confirm explicit downloaded-file quarantine still works and private DB,
   WAL and settings do not acquire a global quarantine policy.
4. On a Developer ID + notarized build, run `codesign --verify --deep --strict`,
   `xcrun stapler validate` on the app and `spctl --assess --type execute -vv`
   on the installed app. Test an actual browser-downloaded DMG on a clean Mac
   (including offline launch) to verify Gatekeeper and the embedded ticket.
5. Verify on the minimum macOS version and each advertised architecture.

Notifications are pending M6.2.1. Developer ID/notarization, Gatekeeper, native
feature behavior and minimum-version compatibility cannot be established by
compilation or signature inspection alone.

## Local build observation (2026-11-15)

On Apple Silicon with Rust 1.99.1, Node 24.10.0, npm 11.6.2, Tauri CLI 2.12.4
and Command Line Tools SDK 27.2, `FLEK_SKIP_NPM_CI=1 scripts/build-macos.sh`
successfully compiled the frontend and release app. Existing shared dependency
symlinks were reused; the fresh `npm ci` path was not run in this sandbox.
`Flek.app` contained 26,411,728 bytes of regular files (25.2 MiB; `du -sh`: 25M).
`codesign --verify --deep --strict` and plist lint passed; inspection confirmed
`adhoc,runtime`, empty entitlements, arm64, Mach-O `minos 13.0`, and the expected
icon, identifier, category and localizations. Flek was never launched.

Tauri's DMG step failed at `hdiutil create` with `Device not configured` in the
restricted build environment. No DMG was produced, so its size/checksum and
mounted contents could not be verified. The tool also warned that
`hdiutil create -volname -format …` is deprecated in favor of `diskutil image`;
that invocation belongs to Tauri CLI's bundled DMG helper. Other warnings were
the existing Vite chunk over 500 kB and the expected skipped notarization without
Apple credentials. Developer ID signing, notarization and stapling were not run.

Offline checks passed: `cargo fmt --all`, `cargo clippy --all-targets -- -D
warnings`, `cargo test -p flek-core` (388 passed, 3 existing ignored benchmarks),
UI TypeScript check, UI tests (111 passed), production frontend build,
`sh -n scripts/build-macos.sh`, plist lint and `git diff --check`. No tests were
added. CLI source was unchanged.
