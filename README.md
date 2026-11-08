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

**Desktop app**:

```bash
cargo install tauri-cli --version "^2"
cd ui && npm install && cd ..
cargo tauri dev
```

On first launch Flek reads your session from the Slack desktop app. macOS asks once for access to the "Slack Safe Storage" Keychain item, allow it.

## Safety

- Tokens and cookies live only in macOS Keychain, never in files, logs or the database.
- Behaves like a normal client: websocket for live data, no aggressive polling, no mass actions.
- Uses a session token outside the official app. This is unofficial use and Slack (or your workspace admin) may revoke the session. Use it on your own account only.

## License

TBD
