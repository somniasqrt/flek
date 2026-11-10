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
│   ├── cli/         # `flek` binary: auth, whoami, db, sync, channels, history, search, send, import
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
cargo run -p flek-cli -- --help
cargo build --release -p flek-cli
```

See the [CLI guide](#cli) for installation, commands and scripting.

<a id="keychain-development"></a>

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

## CLI

### Install and run

From the repository, use `cargo run -p flek-cli -- <cmd>` (for example,
`cargo run -p flek-cli -- channels`). For a standalone release binary:

```bash
cargo build --release -p flek-cli
./target/release/flek --help
```

Optionally, `cargo install --path crates/cli` installs `flek` into Cargo's bin
directory. The examples below assume `flek` is on your PATH.

The first session-requiring command, such as `flek whoami`, imports your
session from the logged-in Slack desktop app if needed. Allow the macOS
Keychain prompt once; credentials stay in Keychain. See the existing
[Keychain and development-signing notes](#keychain-development) for rebuilds.

### Global flags

These options can appear before or after the subcommand. Run `flek --help`
or `flek <cmd> --help` for usage.

| Flag | Meaning |
|---|---|
| `--json` | Machine-readable JSON output. |
| `-v`, `-vv`, `-vvv` / `--verbose` | More logging: info, debug, trace. `RUST_LOG` overrides. |
| `--team <TEAM_ID>` | Team id (offline: the only team in the database; online: stored session). |
| `--db <PATH>` | Database file; default: `~/Library/Application Support/flek/flek.db`. |

### Commands

Examples use fictional destinations and timestamps; replace them with your own.

| Command | Purpose and options | Example |
|---|---|---|
| `whoami` | Show the current team and user; import a session if needed. | `flek whoami` |
| `db` | Open, create or migrate the local DB and show its path and schema version. | `flek db --db /tmp/flek-demo.db` |
| `sync` | Fetch users, conversations, history, threads and files. `--channel <CHANNEL>` selects one exact conversation; `--follow` stays connected until Ctrl+C; `--full` re-reads all history and threads. `--refresh-days <DAYS>` defaults to 7 (0 disables refresh); `--no-files` skips downloads; `--max-file-size <MB>` defaults to 100 (0 means no limit). | `flek sync --channel '#project-demo' --follow --max-file-size 25` |
| `channels` | List stored conversations with message/unread counts; `--archived` includes archived channels. | `flek channels --archived` |
| `history <CHANNEL>` | Read recent stored messages; `--limit <LIMIT>` defaults to 30. `--thread <TS>` shows a thread using its root timestamp. `--fresh` catches up this conversation before reading. | `flek history '#project-demo' --limit 20` |
| `search <QUERY>...` | Search the local archive, including history hidden in Slack. `--limit <LIMIT>` defaults to 20; `--recent` sorts newest first instead of by relevance. | `flek search 'release in:#project-demo after:2026-01-01' --recent --limit 10` |
| `send <CHANNEL> <TEXT>` | Send as you, using Slack mrkdwn (`&`, `<`, `>` are escaped). `--thread <TS>` replies to a thread; `--broadcast` requires it and also shows the reply in the channel. Writes require an exact name or id. | `flek send '#project-demo' 'The release notes are ready.' --thread 1700000000.123456 --broadcast` |
| `import <PATH>` | Import a simple JSON file or Slack export / slackdump file or directory. `--channel <CHANNEL>` puts every message of a single-file archive into that conversation. | `flek import ./archives/project-demo.json --channel C0FAKE` |
| `auth import` | Explicitly import desktop sessions into Keychain. | `flek auth import` |
| `auth list` | List teams with stored credentials in Keychain. | `flek auth list` |
| `auth logout` | Remove stored credentials; `--team` selects a team, `--all` removes every stored team. | `flek auth logout --team T0FAKE` |

`sync` is resumable; Ctrl+C is safe. Every `flek sync` run re-reads the
refresh window for edits, reactions, deletions and thread replies. The app
re-reads that window at most once every 3 hours. `--full` merges without
losing stored history, including messages Slack no longer returns.

Search supports `from:<person>`, `in:<channel>`, `has:link`, `has:file`,
`before:YYYY-MM-DD`, `after:YYYY-MM-DD`, trailing prefix `*` (`release*`),
`"exact phrase"` and `-excluded`. Quote the whole query for your shell:

```bash
flek search '"release notes" from:alex in:#project-demo has:link -draft'
```

### Referring to conversations

Use an id (`C0FAKE`), `#name`, a plain channel name, or `@person`, handle,
display name, real name or email for a stored DM. Quote names containing
spaces. `#` restricts matching to channels; `@` to personal DMs. HTTPS Slack
permalinks such as
`https://fake.slack.com/archives/C0FAKE/p1700000000123456` also work;
conversation commands use the conversation component. To read that thread,
pass its root timestamp explicitly: `flek history C0FAKE --thread 1700000000.123456`.

Reads allow case-insensitive prefix matching after exact matches. Ambiguous
references return candidates, never a guess. `send` and `sync --channel`
require a complete name or id (case-insensitive exact names are accepted);
a prefix produces suggestions even if there is only one candidate.

### Offline and online

`channels`, `history`, `search`, `import` and `db` work locally without
reading Keychain. Reads reflect what is already stored; import writes to
the local archive. If several teams are in the DB, pass `--team` for
team-scoped commands. `db` can create an empty DB, but does not populate it.

`whoami`, `sync`, `send` and `history --fresh` require a session and network
access. `auth` manages Keychain sessions; `auth import` also validates them
online. `history --fresh` catches up only the selected conversation; with
`--thread` it fetches that thread's pages. It does not download files or do
a full backfill; use `sync` for deep history.

The app and CLI can run at the same time on the same SQLite DB.

### For scripts and AI agents

With `--json`, finite commands emit one JSON envelope on stdout, including
`"schema_version": 1` and command-specific fields such as `conversations`,
`messages` or `results`. Errors emit a JSON object on stderr with
`schema_version`, `ok: false`, and `error.code` / `error.message` (plus
`error.candidates` for ambiguous conversation references). JSON mode
suppresses human logs and progress; help remains text.

```bash
# Stored unread conversations (unknown read markers are excluded).
flek channels --json | jq '.conversations[] | select((.unread_count // 0) > 0)'

# Last 20 stored messages, oldest first, with resolved names and links.
flek history '#project-demo' --limit 20 --json | jq -r '.messages[].text_resolved'

# Live NDJSON: each line has schema_version and a type discriminator.
flek sync --follow --json | jq --unbuffered 'select(.type == "message") | .message'
```

`sync --follow --json` streams NDJSON events and emits a terminal summary on
graceful shutdown. Notifications are best effort; re-read history after a
disconnect. Keep Slack timestamps as strings.

| Exit | Error code | Meaning |
|---|---|---|
| 0 | — | Success. |
| 1 | `internal` | Internal, DB, I/O or other failure. |
| 2 | `usage` | Invalid arguments or query syntax. |
| 3 | `not_found` | Unresolved conversation/message or no stored team. |
| 4 | `ambiguous` | Tied references, prefix-only suggestions in exact mode, or ambiguous team selection. |
| 5 | `auth` | Missing/invalid/revoked session, Keychain or import failure. |
| 6 | `rate_limited` | Rate-limit retries exhausted. |
| 7 | `network` | Transport, HTTP, websocket or other Slack API failure. |
| 8 | `forbidden` | Reserved for the planned write guard; currently unused. |

See [the CLI JSON contract](docs/cli-json.md) for full schemas and stream
semantics. A full agent-facing CLI (reading unread messages, threads and
mentions, editing, reactions, creating channels) is in progress.

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
