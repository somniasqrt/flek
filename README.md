# Flek

> Name: **Flek**. Don't use "Slack" in the product name or icon: it's a trademark.

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
| planned | Zero-click login: import session from the installed Slack desktop app (macOS Keychain prompt once) |
| planned | Credentials stored only in macOS Keychain |
| planned | Backfill of all available history into SQLite |
| planned | Live updates over websocket |
| planned | Channels, DMs, threads, reactions, images, files |
| planned | Sending messages, replies, reactions, mark as read |
| planned | Local full-text search including hidden (>90 days) messages |
| planned | Import of existing JSON archives (e.g. slackdump) |
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
cargo run -p flek-cli -- sync                   # backfill + catch up into local DB
cargo run -p flek-cli -- search "grades sheet"  # local full-text search
cargo run -p flek-cli -- import <path>          # import an existing JSON archive
```

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
