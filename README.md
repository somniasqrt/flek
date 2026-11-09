# Flek

An **ultrafast, safe, local-first desktop client for Slack**, written in Rust.

It talks to Slack with the same session (`xoxc` token + `d` cookie) that the official web/desktop app uses, so to Slack it's just *you*, not a bot. Every message it sees is stored in a local SQLite database, which means:

- **history never disappears**: on the Free plan Slack hides everything older than 90 days, Flek keeps showing it from the local archive, seamlessly mixed with live messages;
- **search is instant** (SQLite FTS5) and covers messages Slack already hid;
- **reading and sending** work like in the real client: channels, DMs, threads, reactions, files, images.

## Why

On Slack's Free plan, messages, files and links older than 90 days get hidden. Regular members can't export a workspace, and a workspace that has reached the app integration limit can't add an official OAuth app either.

## Features

| Status | Feature |
|---|---|
| done (M0) | Zero-click login: import session from the installed Slack desktop app (macOS Keychain prompt once) |
| done (M0) | Credentials stored only in macOS Keychain |
| done (M1) | Backfill of all available history into SQLite, resumable, with threads, reactions and files |
| done (M2, CLI) | Live updates over websocket, survives network drops and sleep |
| planned | Channels, DMs, threads, reactions, images, files |
| planned | Sending messages, replies, reactions, mark as read |
| done (M1, CLI) | Local full-text search including hidden (>90 days) messages |
| done (M1) | Import of existing JSON archives (e.g. slackdump) |
| planned | "Open in Slack" deep link for anything not supported (polls, slash commands, huddles) |

## Stack

- **Rust** core: `tokio`, `reqwest` (rustls), `tokio-tungstenite`, `serde`, `rusqlite` (bundled, FTS5), `keyring`
- **Tauri 2** desktop shell, lightweight web UI (SolidJS + TypeScript)
- macOS first (Apple Silicon)

## Layout

```
flek/
├── crates/
│   ├── core/        # flek-core: auth, Slack API, websocket, store, sync (no UI deps)
│   ├── cli/         # `flek` binary: whoami, sync, search, import
│   └── app/         # Tauri app, commands bridging core <-> UI
├── ui/              # SolidJS frontend
├── docs/            # API notes discovered via DevTools, ADRs
├── AGENTS.md        # rules and context for coding agents
└── README.md
```

## Getting started

Prerequisites: Rust stable, Node 20+, Slack desktop app logged in.

**CLI** (core works without any UI):

```bash
cargo run -p flek-cli -- whoami                 # import session, show team/user
cargo run -p flek-cli -- sync                   # users, channels, history, threads, files → local DB
cargo run -p flek-cli -- channels               # conversations with message/unread counts
cargo run -p flek-cli -- history general        # read a channel offline (--thread <ts> for a thread)
cargo run -p flek-cli -- search "grades in:#general after:2024-01-01"   # offline full-text search
cargo run -p flek-cli -- import <path>          # simple JSON archive or Slack export / slackdump
cargo run -p flek-cli -- auth list              # teams stored in Keychain
cargo run -p flek-cli -- auth logout            # remove stored session
```

`flek sync --follow` stays connected and prints new messages live.
`flek sync` is resumable (Ctrl+C is safe) and re-reads the last 7 days for
edits, reactions and deletions (`--refresh-days`); `--full` walks everything
again, `--no-files` skips downloads.

**Keychain prompts during development (macOS).** Keychain remembers "Always
Allow" per code signature, and every cargo build gets a new ad-hoc one, so
macOS asks again after each rebuild. Create a self-signed code signing
certificate once and `cargo run` signs `flek` with it
(`scripts/macos-runner.sh`):

1. Keychain Access → menu *Keychain Access → Certificate Assistant → Create a Certificate…*
2. Name `Flek Dev`, Identity Type *Self Signed Root*, Certificate Type *Code Signing* → Create.
3. Run any `flek` command and click *Always Allow* once per prompt; rebuilds keep the permission.

`security find-identity -p codesigning` should list `"Flek Dev"`; being
marked `CSSMERR_TP_NOT_TRUSTED` is fine (no need to trust it). Another name
works via `FLEK_CODESIGN_IDENTITY`.

**Desktop app**:

```bash
cargo install tauri-cli --version "^2"
cd ui && npm install && cd ..
cargo tauri dev
```

On first launch Flek reads your session from the Slack desktop app. macOS asks once for access to the "Slack Safe Storage" Keychain item, allow it.

## Message menu and reactions

Right-click a message for **Add reaction** (R), **Reply in thread** (T),
**Copy link** (L) or **Copy message** (Command/Ctrl+C). The copied link is a
Slack deep link; copying it does not open Slack. Shortcuts act while the menu is open.

The emoji picker has category tabs, search by name/description, frequently
used emoji and a skin-tone preference. Usage and tone are stored locally.
Arrow keys move between emoji; Enter chooses the first search result.

## Composer shortcuts

Use **Command** on macOS and **Ctrl** on Windows/Linux. Shortcuts follow
physical keys, so they also work with Ukrainian keyboard layouts. Button
tooltips show the platform's shortcut.

| Action | Shortcut (Mod = Command/Ctrl) |
|---|---|
| Bold / italic / underline | Mod+B / Mod+I / Mod+U |
| Strikethrough | Mod+Shift+X |
| Insert link | Mod+K |
| Numbered / bulleted list | Mod+Shift+7 / Mod+Shift+8 |
| Quote | Mod+Shift+9 |
| Inline code / code block | Mod+Shift+C / Mod+Shift+J |
| Clear formatting | Mod+\ |
| Toggle formatting toolbar | Mod+Shift+F |
| Emoji picker | Mod+Shift+E |
| Undo / redo | Mod+Z / Mod+Shift+Z (also Ctrl+Y on Windows/Linux) |
| Send / new line | Enter / Shift+Enter |

The arrow beside Send switches Enter to a new line and Mod+Enter to send.
Formatting, emoji and mention pickers work in channels and threads; drafts
keep their formatting. File upload, video/audio clips and canvases are not
available from the composer yet.

## Safety

- Tokens and cookies live only in macOS Keychain, never in files, logs or the database.
- Behaves like a normal client: websocket for live data, no aggressive polling, no mass actions.
- Uses a session token outside the official app. This is unofficial use and Slack (or your workspace admin) may revoke the session. Use it on your own account only.

## License

TBD
