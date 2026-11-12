<p align="center">
  <img src="assets/flek-icon.svg" width="128" height="128" alt="Flek icon">
</p>

<h1 align="center">Flek</h1>

An **ultrafast, safe, local-first desktop client for Slack**, written in Rust.

It talks to Slack with the same session (`xoxc` token + `d` cookie) that the official web/desktop app uses, so to Slack it's just *you*, not a bot. Every message it sees is stored in a local SQLite database, which means:

- **history never disappears**: on the Free plan Slack hides everything older than 90 days, Flek keeps showing it from the local archive, seamlessly mixed with live messages;
- **search is instant** (SQLite FTS5) and covers messages Slack already hid;
- **reading and sending** work like in the real client: channels, DMs, threads, reactions, files, images.
- **in-app file viewer** opens cached PDFs, Excel (.xlsx) and Word (.docx) files, with an action to open the full file in the default app.

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
| implemented (M3.5, M3.5.6; visual verification pending) | In-app file viewer for images, PDF, Excel (.xlsx) and Word (.docx) from the local cache, including files Slack already hid |
| implemented (M6.7; live verification pending) | Presence on DM/profile/self avatars; You menu for custom status, away/active and notification snooze |
| implemented (M6.8; visual/live verification pending) | Workspace icon, Home/DMs/Activity/Files rail, unread badges, compose picker, notification pause, DM intros; local activity and file browser |
| implemented (M4.6, M6.9; visual/live verification pending) | Focused visible-message read tracking, opening unread divider, conversation context menu, cached channel details, local stars/mute, leave/close |
| implemented (M6.3.5; native verification pending) | Window size and position persistence, with fallback when a monitor is removed |
| implemented (M6.3.1, M6.10; visual/native verification pending) | English/Ukrainian localisation and Preferences: appearance, sidebar, media, sync, storage and account |
| implemented (M6.11; visual/live verification pending) | Browse the workspace's channels (search, filters, preview, join) and create public or private channels |
| planned | Remaining message editing/deletion utilities |
| done (M1, CLI) | Local full-text search including hidden (>90 days) messages |
| done (M1) | Import of existing JSON archives (e.g. slackdump) |
| planned | "Open in Slack" deep link for anything not supported (polls, slash commands, huddles) |

## Preferences and language

Open **Preferences** with **⌘,** (Ctrl+, on Windows/Linux), the app menu, or **You → Preferences**. Changes are applied immediately. Choose System, English or Українська; System uses Ukrainian for a Ukrainian system/browser locale and English otherwise. Theme, message text size, clock, DM sorting, muted groups, presence dots and media previews update without restarting.

Preferences are validated and stored atomically by core in `settings.json` next to `flek.db`. File download limits apply on the next sync pass; edit refresh settings apply when reopening the app (0 days disables refresh). Launch at login is opt-in and reflects the OS login item. Closing can either keep Flek syncing in the menu bar or quit it.

Storage shows the database path/size (including WAL) and cache size. Cache clearing preserves messages, avatars and emoji, invalidates downloaded file paths and resumes live sync; automatic file downloads are skipped for that first resumed pass. Full re-sync re-reads available history in the background. Signing out removes Flek’s stored session, preserves the archive and returns to sign-in; it does not sign out the Slack desktop app or silently re-import it on the next launch. Notifications are deferred to M6.2.1.

For a session-free preview, run `npm run dev --workspace ui` and open `http://localhost:5173/preview.html?lang=uk&preferences` (use `lang=en` for English). All settings, storage and account actions in that page are mocked.

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
| `channels` | List stored conversations with message/unread counts; `--archived` includes archived channels; `--include-non-member` includes conversations discovered by search. | `flek channels --archived` |
| `history <CHANNEL>` | Read recent stored messages; `--limit <LIMIT>` defaults to 30. `--thread <TS>` pages a thread. `--since/--until`, `--around <TS or permalink>`, `--cursor` select windows/pages; JSON returns older/newer cursors. `--fresh` catches up first. | `flek history '#project-demo' --limit 20` |
| `unread` | DMs and mentions first; `--messages N` attaches oldest unread messages; `--include-unknown` lists unknown read markers separately; `--fresh` refreshes directory and catches up activity. | `flek unread --messages 3 --json` |
| `thread <REF> [TS]` | Root and all stored replies; accepts a reply permalink; `--fresh` fetches the thread. | `flek thread C0FAKE 1700000000.123456` |
| `message <REF> [TS]` | One message; `--context N` adds adjacent messages from its timeline; `--fresh` catches up first. | `flek message C0FAKE 1700000000.123456 --context 2` |
| `users` / `user <REF>` | Stored directory/profile; `users --query TEXT --limit N` (default 100), `--include-bots`, `--include-deleted`. User refs accept ids, handles, names and emails. | `flek user @alex --json` |
| `channel info <REF>` | Stored topic/purpose and metadata; `--fresh` stores members and pins for later offline reads. | `flek channel info '#project-demo' --fresh` |
| `mentions` | Local mentions, newest first; `--since` defaults to 7d, `--until`, `--limit` defaults to 50; `--include-broadcasts` adds here/channel/everyone. | `flek mentions --since 12h --json` |
| `files` | Local file shares; `--channel`, `--from`, `--type`, `--since`, `--until`, `--limit` (default 50). Types: image/pdf/video/audio/doc/other or MIME prefix. | `flek files --channel '#project-demo' --type image` |
| `file get <ID>` | Print cache path; download only if missing; `--to PATH` copies it. | `flek file get F0FAKE --to ./notes.pdf` |
| `search <QUERY>...` | Search the local archive, including history hidden in Slack. `--limit` defaults to 20; `--recent` sorts newest first. `--since/--until`, `--context N`, `--offset` or `--cursor` select windows/context/pages. `--remote` stores Slack search candidates before reading merged SQLite results; JSON includes parsed filters, sources and remote status. | `flek search 'release in:#project-demo after:2026-01-01' --recent --limit 10` |
| `send <CHANNEL> <TEXT>` | Send as you, using Slack mrkdwn (`&`, `<`, `>` are escaped). `--thread <TS>` replies to a thread; `--broadcast` requires it and also shows the reply in the channel. Writes require an exact name or id. | `flek send '#project-demo' 'The release notes are ready.' --thread 1700000000.123456 --broadcast` |
| `import <PATH>` | Import a simple JSON file or Slack export / slackdump file or directory. `--channel <CHANNEL>` puts every message of a single-file archive into that conversation. | `flek import ./archives/project-demo.json --channel C0FAKE` |
| `auth import` | Explicitly import desktop sessions into Keychain. | `flek auth import` |
| `auth list` | List teams with stored credentials in Keychain. | `flek auth list` |
| `auth logout` | Remove stored credentials; `--team` selects a team, `--all` removes every stored team. | `flek auth logout --team T0FAKE` |

`sync` is resumable; Ctrl+C is safe. Every `flek sync` run re-reads the
refresh window for edits, reactions, deletions and thread replies. The app
re-reads that window at most once every 3 hours. `--full` merges without
losing stored history, including messages Slack no longer returns.

Search is offline without `--remote`, including JSON, context and paging. Remote
search can discover public non-member channels and unsynced messages, while local
archives retain older history. Server search with this web-session type is
unverified; non-auth remote failures keep local results and report a warning
(`remote.error` in JSON). Search preserves fuller stored messages and doesn't
start syncing discovered channels. See [the JSON contract](docs/cli-json.md#agent-search-m83-additive-version-1)
for bounds, cursors and unsupported remote filters.

Search supports `from:<person>`, `in:<channel>`, `has:link`, `has:file`,
`before:YYYY-MM-DD`, `after:YYYY-MM-DD`, trailing prefix `*` (`release*`),
`"exact phrase"` and `-excluded`. Quote the whole query for your shell:

```bash
flek search '"release notes" from:alex in:#project-demo has:link -draft'
```

Time bounds accept Slack ts, local dates (`YYYY-MM-DD`), RFC3339, or durations
(`30m`, `12h`, `7d`). Bounds are inclusive; use absolute bounds when following
history cursors.

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

`channels`, `history`, `unread`, `thread`, `message`, `users`, `user`,
`channel info`, `mentions`, `files`, `search`, `import` and `db` work locally without
reading Keychain. Reads reflect what is already stored; import writes to
the local archive. If several teams are in the DB, pass `--team` for
team-scoped commands. `db` can create an empty DB, but does not populate it.

`whoami`, `sync`, `send`, explicit `--fresh` reads and uncached `file get` require a session and network
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
flek unread --messages 3 --json | jq '.conversations[] | {label, unread_count, mentions, messages}'

# Last 20 stored messages, oldest first, with resolved names and links.
flek history '#project-demo' --limit 20 --json | jq -r '.messages[].text_resolved'

# Mention text and the permalink an agent can pass to thread/message.
flek mentions --since 7d --json | jq -r '.messages[] | [.conversation, .text_resolved, .permalink] | @tsv'

# Copy the older-page cursor verbatim; keep absolute window bounds unchanged.
flek history '#project-demo' --limit 20 --json | jq '.next_cursor'

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
semantics. Agent reads are available; editing, reactions and channel creation
remain on the roadmap.

## Navigation shortcuts

They follow the Slack desktop app (Mod = Command/Ctrl) and work while typing.

| Action | Shortcut |
|---|---|
| Quick switcher | Mod+K |
| Previous / next conversation | Alt+↑ / Alt+↓ (add Shift for unread only) |
| Back / forward | Mod+[ / Mod+] |
| Home / DMs / Activity | Ctrl+1 / Mod+Shift+K / Mod+Shift+M |
| Browse channels | Mod+Shift+L |
| Conversation details | Mod+Shift+I |
| New message | Mod+N |
| Search / search this conversation | Mod+G / Mod+F |
| Close the side panel | Mod+. or Esc |
| Mark the conversation read | Esc (when no panel is open) |
| Zoom in / out / actual size | Mod+= / Mod+- / Mod+0 (View menu, remembered) |
| Full screen, minimize, close window | Ctrl+Cmd+F, Mod+M, Mod+W (macOS menus) |

## File viewer

Click a downloaded image, PDF, Excel (.xlsx) or Word (.docx) file to open it in
the viewer; Esc or a click outside closes it, and the arrow button opens the file
in its default app. Everything is read from the local cache, so it also works
offline and for files Slack hides on the Free plan. Videos play inline in the
timeline.

- **Excel**: one tab per sheet (←/→ switch tabs), cell values as saved in the
  file. Formulas are not recalculated and macros never run. Up to 1000 rows ×
  100 columns per sheet are shown; larger sheets say they are truncated.
- **Word**: text, headings, lists, tables and embedded images on a white page.
  Links are shown as text, and page layout and fonts are not reproduced.
- Documents are parsed in a separate worker with size and complexity limits.
  Files over 50 MB, documents that are too large or complex, and damaged or
  encrypted files each show their own message with the "open in default app"
  action. Legacy .xls/.doc, PowerPoint and other formats open in their default
  app directly.

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
| Insert link | Mod+Shift+U |
| Numbered / bulleted list | Mod+Shift+7 / Mod+Shift+8 |
| Quote | Mod+Shift+9 |
| Inline code / code block | Mod+Shift+C / Cmd+Option+Shift+C (macOS), Mod+Shift+J |
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
