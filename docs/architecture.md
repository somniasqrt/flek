# Architecture

Read this before editing a layer; then read AGENTS.md, ROADMAP.md and local NOTES.md. This describes the implemented tree, not the remaining roadmap. Platform differences and paths are maintained in [platforms.md](platforms.md); session identity in [ADR 0001](adr/0001-session-and-client-identity.md). CLI wire shapes live in [cli-json.md](cli-json.md), imports in [import-formats.md](import-formats.md).

## Layers and data flow

Dependencies point inward: `ui → app → core`, with `cli → core` alongside app. Core has no Tauri, Solid or webview dependencies. If both CLI and app need it, it belongs in core. Transport parsing belongs in api/rtm; orchestration and writes in sync; SQL in store/read views. App owns OS integration and IPC conversion. CLI owns clap, terminal output and the JSON envelope. UI owns presentation, interaction and reactive state; `api.ts` is its typed Tauri boundary.

```text
Slack HTTP / websocket
       | core::api / core::rtm
       v
   core::sync ----------> core::store / SQLite (WAL, FTS5)
       | commit                   | read connections
       | sync::live::Change       v
       |                 core::reads / views / rail
       |                          |
       +--> app event bridge      +--> cli output
       |                          +--> app Tauri commands
       v                                      |
 UI api.onChange <-----------------------------+
       | re-read through commands             |
       +--> stores / views / components <-----+
 UI actions --> commands --> core sync/API --> commit --> change
```

Network responses are stored before consumers display their authoritative copy. The outbox alone has a provisional local message, matched by `client_msg_id`; command return values are not a second authoritative timeline. Change events are invalidation hints: reconnect/lag requires re-reading stored history. Entry points: CLI `main → dispatch::run`; desktop `main → state::boot`; UI `index.tsx → App → Workspace → useWorkspace` with the same Solid owner. The engine has its own `flek-sync` thread and current-thread Tokio runtime because its follow future is not Send. Store blocking I/O uses spawn_blocking.

`sync::follow` supervises non-auth failures, including presence invalidation: it restarts with capped exponential backoff and ±50% jitter, emits `Change::Connection(Reconnecting)`, and remains cancellable during work/backoff. Auth failures bypass retries and retain the desktop app's generation-checked recovery path. Failed catch-up (including per-conversation failures in pass totals) retries on the same socket while events and heartbeats continue; success restores Connected. A healthy socket with failed catch-up emits `ConnectionState::SyncIncomplete { in_secs }`; the SyncBar shows connected but incomplete sync until catch-up succeeds. `Reconnecting` is reserved for socket/engine reconnection. The app caches and forwards both states through the existing generic connection event bridge. Synthetic preview console hook: `__syncIncomplete(true)` (false restores Connected).

Every successful connection runs a pass. Engine passes and live directory invalidations share `DirectoryRefresh`, which serializes requests and coalesces concurrent successes. A successfully fetched and stored people list is reused for five minutes; failed people refreshes do not advance that marker, so the next catch-up pass can retry immediately after its own backoff. Reconnect passes still reload conversations and client.counts, so cached remote_latest cannot hide messages received while disconnected. Within the people-cache window, live invalidations promptly run conversations-only sync (users.conversations and client.counts, without users.list), with a separate ten-second minimum spacing between live attempts, including failures. A conversation-sync failure after successful people sync can likewise retry without fetching users.list again. Live invalidations coalesce into one future in the session select, so directory network work and its short delay do not suspend websocket heartbeats/events. Unknown-conversation events are deferred and replayed after refresh; events still unresolved are dropped until later sync supplies their directory identity. Explicit/one-shot directory requests remain unthrottled. Both startup and reconnect passes may run the quiet edit/reaction/reply refresh window, sharing the history/replies rate limiter. `refresh_runs` and `refresh_every` throttle successful refreshes; failed refreshes never advance the marker. Zero means every startup/one-shot invocation; subsequent live passes use at least `dead_after` to avoid refresh bursts on quick reconnects. First-pass full/skip-files/custom-emoji behavior remains separate from refresh eligibility.

## Module map

Paths below are relative to each source root. `mod.rs` facades keep existing public function/type paths stable; implementation submodules stay private. Tests next to modules are described with those modules; integration suites are below.

### crates/core/src

- `api/auth.rs` — `auth.test`.
- `api/channel_browser.rs` — Documented workspace directory and explicit channel writes.
- `api/channel_details.rs` — Documented conversation membership and pins endpoints.
- `api/chat.rs` — `chat.postMessage`: sending as the user.
- `api/client.rs` — HTTP plumbing: auth headers, `ok`/`error` envelope, rate limits.
- `api/conversations.rs` — `users.conversations`.
- `api/emoji.rs` — `emoji.list`.
- `api/history.rs` — `conversations.history`.
- `api/internal.rs` — Undocumented endpoints the Slack web client uses. Each one is described in
- `api/mod.rs` — Public facade for api; grouped implementations are re-exported here.
- `api/pagination.rs` — Cursor pagination shared by list methods.
- `api/presence.rs` — Documented presence, custom status and Do Not Disturb methods.
- `api/profile.rs` — The client identity Flek presents to Slack (HTTP and websocket alike).
- `api/reactions.rs` — Documented message reaction methods.
- `api/rtm.rs` — `rtm.connect`: a one-time websocket URL for live events.
- `api/search.rs` — Documented server-side search. Search payloads are partial history messages.
- `api/team.rs` — Documented workspace metadata, fetched by directory sync.
- `api/users.rs` — `users.list`.
- `auth/chromium.rs` — Reading Chromium/Electron profile data: Local Storage (LevelDB) and the
- `auth/desktop_import.rs` — Import the session from the installed Slack desktop app (macOS only so far).
- `auth/desktop_profile.rs` — The identity of the installed Slack desktop app (ADR 0001): the user agent
- `auth/keychain.rs` — Credentials in the OS credential store via `keyring` (service `flek`,
- `auth/mod.rs` — Session resolution/import/recovery and credential-store facade.
- `auth/origin.rs` — Who issued a session (ADR 0001). Stored next to the credentials, so every
- `auth/webview.rs` — The system webview Flek signs in with (ADR 0001, webview login).
- `error.rs` — Error type shared by the whole core.
- `net_policy.rs` — Private URL trust boundaries and public-image DNS resolver: session URLs only to HTTPS Slack hosts (or the exact mock origin); public images from any HTTPS site but only public IPs, redirects re-checked.
- `private_fs.rs` — Private app-owned directories/files and SQLite sidecar mode repair (Unix).
- `files/mod.rs` — Authenticated stored-file cache, sanitization, public proxy images and previews. `files/safety.rs` owns the shared open allowlist, canonical containment, bidi stripping, quarantine and protected saved copies.
- `files/storage.rs` — Offline storage inspection and cache maintenance.
- `lib.rs` — Public library module surface; no app dependencies.
- `rail.rs` — Local rail views. No network or UI dependencies.
- `reads/channel.rs` — Cached metadata, members and pins, including never-fetched versus empty.
- `reads/files.rs` — File filters, share context, downloadable metadata and cached file lookup.
- `reads/history.rs` — Bounded timeline/thread windows, opaque cursors and message context.
- `reads/mentions.rs` — FTS mention candidate source shared with rail activity.
- `reads/mod.rs` — Stable offline read facade (reads::users/history/etc. remain functions).
- `reads/unread.rs` — Known unread conversations and separate unknown read-marker results.
- `reads/users.rs` — Offline directory filtering and hydrated user views.
- `references.rs` — Unambiguous references shared by CLI, app and future integrations.
- `rtm/connection.rs` — One websocket session: handshake, `hello`, frames in and out.
- `rtm/events.rs` — RTM events we act on. Parsing is manual (not a serde tag) because
- `rtm/mod.rs` — Websocket connection/event exports.
- `search.rs` — Agent search: resolve locally, optionally ingest remote evidence, read SQLite.
- `secret.rs` — A string that never shows up in logs.
- `settings.rs` — Versioned, non-secret preferences shared by desktop and sync.
- `store/channel_browser.rs` — Cached channel directory. Metadata stays in conversations, including raw JSON.
- `store/directory/conversations.rs` — Conversation merges, labels, member-only sidebar listing and resolution.
- `store/directory/mod.rs` — Public facade for store/directory; grouped implementations are re-exported here.
- `store/directory/profiles.rs` — Cached avatars, profile photos, redacted profiles and custom status reads.
- `store/directory/progress.rs` — Forward read markers, remote latest timestamps and engine skip queries.
- `store/directory/teams.rs` — Stored identity and workspace icon metadata.
- `store/directory/users.rs` — Directory user upserts, partial live merges and display-name lookup.
- `store/emoji.rs` — Workspace (custom) emoji from `emoji.list` and their cached images.
- `store/files.rs` — File metadata and the local cache path.
- `store/messages/cache.rs` — Cached file and preview paths by file id.
- `store/messages/display.rs` — Timeline hydration with files, unfurls and own reactions.
- `store/messages/mod.rs` — Public facade for store/messages; grouped implementations are re-exported here.
- `store/messages/progress.rs` — Conversation/thread catch-up cursors, refresh markers and stale roots.
- `store/messages/reactions.rs` — Idempotent reaction changes and grouped counts.
- `store/messages/write.rs` — Transactional hidden/full/edit merge rules, deletion and live reply counting.
- `store/migrations.rs` — Numbered SQL migrations, applied in order at startup.
- `store/mod.rs` — Store handle, WAL startup/migrations, writer mutex, reader pool and blocking dispatch.
- `store/presence.rs` — Ephemeral state persisted for DB-only readers. Freshness is per source:
- `store/search.rs` — Local full-text search over messages (FTS5) with Slack-like filters:
- `store/unfurls.rs` — Link previews (Slack "unfurls", the `attachments` of a message) and the
- `sync/backfill.rs` — History backfill: catch up on new messages, then walk back to the start.
- `sync/channel_browser.rs` — User-triggered directory, preview, join and create. Never called by background sync.
- `sync/channel_details.rs` — Persist a complete membership/pins snapshot before any view is produced.
- `sync/conversation.rs` — Explicit sidebar actions; persist only after Slack confirms remote writes.
- `sync/directory.rs` — Users and conversations into the database.
- `sync/engine.rs` — The background sync engine the app (and `flek sync`) runs; nobody has to
- `sync/files.rs` — Fill the local file cache.
- `sync/follow.rs` — Keep a live connection: connect, catch up, apply events, survive drops.
- `sync/import.rs` — Import existing archives so no history is lost. Formats are described in
- `sync/live.rs` — Live events → database, and the change bus the UI and CLI listen to.
- `sync/mod.rs` — SyncOptions, timestamp helpers and network-to-store orchestration facade.
- `sync/presence.rs` — Presence snapshots and self writes. Network responses commit before readers
- `sync/reactions.rs` — Confirm with Slack, then apply the same idempotent change as the RTM echo.
- `sync/read.rs` — Conversation cursors only. Slack's separate, undocumented thread read API is out of scope.
- `sync/send.rs` — Sending a message: Slack first, then the database, through the same
- `sync/threads.rs` — Thread replies via `conversations.replies`.
- `time.rs` — Shared CLI time arguments, normalized without floating point.
- `views/conversations.rs` — Versioned conversation output and cached workspace permalinks.
- `views/messages.rs` — Batch hydration, resolved labels, file/reaction output and search results.
- `views/mod.rs` — Public facade for views; grouped implementations are re-exported here.
- `views/text.rs` — Timestamp rendering and Slack reference/entity resolution.
- `views/users.rs` — Hydrated user output including stored presence and DND.
- `views/version.rs` — Machine-view schema version (independent of SQLite version).
- `store/migrations/` — append-only SQL 0001–0019; schema evolution described below.

### crates/cli/src

- `account_commands.rs` — Credential import/list/logout and whoami output.
- `archive_commands.rs` — DB metadata, stored channels and archive import output.
- `args.rs` — Global flags and non-read clap command definitions.
- `common.rs` — Database opening, offline team selection and terminal timestamp format.
- `dispatch.rs` — Dispatch unchanged command variants and configure logging.
- `main.rs` — Parse arguments and report typed failures at the process boundary.
- `output.rs` — CLI framing and safe failure reporting.
- `read_commands/args.rs` — Read clap definitions, including history window options.
- `read_commands/dispatch.rs` — Read-command routing and per-command human/JSON output.
- `read_commands/history.rs` — Reference/time selection, history/context reads and terminal rendering.
- `read_commands/mod.rs` — Read adapter facade and shared imports.
- `search_commands.rs` — Offline/explicit-remote search adapter and warning output.
- `send_commands.rs` — Exact destination resolution and explicit send output.
- `sync_commands.rs` — Engine setup, terminal progress and live NDJSON emission.

### crates/app/src

- `browsing_data.rs` — Versioned legacy Slack website-data cleanup; macOS record-scoped WebKit bridge and completion/marker sequencing.
- `navigation.rs` — Guarded main-window construction, own-origin navigation and denied native popups.
- `autostart.rs` — Launch at login (see `platform::autostart_launcher`), started with
- `commands/account.rs` — Boot status, onboarding, account origin and sign-out IPC.
- `commands/channels.rs` — Stored channel details, explicit directory/preview/join/create IPC.
- `commands/conversations.rs` — Sidebar/rail reads, read marks, preferences and dismiss IPC.
- `commands/files.rs` — File browsing, fetch/reveal/open, bounded document bytes and URL opener IPC.
- `commands/messages.rs` — Timeline/thread/search reads and explicit send/reaction IPC.
- `commands/mod.rs` — Stable command re-exports, error text conversion and shared page cap.
- `commands/presence.rs` — Directory, avatars, profile/status reads and bounded presence actions.
- `commands/settings.rs` — Preferences, serialized maintenance, cache/storage and resync IPC.
- `files.rs` — `flekfile://localhost/<file id>[/preview]` and `…/link/<hash>`: cached
- `i18n.rs` — Native strings. Fixed-size pairs enforce completeness for both languages.
- `login.rs` — "Sign in to Slack": an embedded webview on app.slack.com (ADR 0001).
- `main.rs` — Flek desktop: a thin Tauri shell over `flek-core`. Business logic lives in
- `platform.rs` — Everything that behaves differently per OS, in one place. macOS keeps its
- `state.rs` — What the app holds: the database, the credential store, the change bus,
- `tray.rs` — Menu bar icon (system tray on Windows and Linux): sync status, open
- `window_size.rs` — Normal main-window size and outer position, in logical pixels.

### ui/src

- `App.tsx` — Status routing, locale/preferences integration and workspace mounting.
- `api.ts` — IPC types, invoke wrappers, change subscriptions and cached asset URLs.
- `components/Avatar.tsx` — Cached avatar/fallback and optional presence indicator.
- `components/ChannelsMenu.tsx` — Browse/create channel menu.
- `components/ComposeIcon.tsx` — Composer action SVG paths.
- `components/Composer.tsx` — Editor DOM, toolbar, popovers and send controls.
- `components/ConversationMenu.tsx` — Conversation actions, menu targets and leave confirmation.
- `components/CustomStatus.tsx` — Expiry-aware status text/emoji presentation.
- `components/DmIntro.tsx` — Self, partner and group DM introduction.
- `components/EmojiCategoryIcon.tsx` — Category SVG paths.
- `components/EmojiPicker.tsx` — Category/search/frequency/tone selection.
- `components/EmojiView.tsx` — Custom/standard emoji rendering and cached custom catalog.
- `components/SvgPreview.tsx` — Lazy SVG image-context previews shared by attachments and FileBrowser.
- `components/Files.tsx` — Message attachments and cached previews. `components/FileSafety.tsx` presents core launch decisions, warnings and platform file-manager labels.
- `components/MessageMenu.tsx` — Message actions and reaction picker.
- `components/MessageRow.tsx` — Author grouping, text, reactions, files and thread summary.
- `components/PresenceIndicator.tsx` — Visual active/away/DND/unknown indicator.
- `components/ResizeHandle.tsx` — Pointer/keyboard panel resize interaction.
- `components/Rich.tsx` — Parsed mrkdwn rendering and link actions.
- `components/RichBlocks.tsx` — Read-only rich_text and Block Kit rendering; unsupported types retain plain-text/native-client fallback.
- `components/StatusForm.tsx` — Status form rendering; YouMenu retains draft and focus ownership.
- `components/SyncBar.tsx` — Sync progress and connection status presentation.
- `components/Unfurls.tsx` — Cached website preview cards.
- `components/Viewer.tsx` — Image/PDF/Office overlay and lazy PDF pages; OfficeDocument.tsx owns sheet tabs, windowed rows and sanitized Word pages.
- `components/YouMenu.tsx` — Self menu, DND/status actions and dialog/focus state.
- `components/composer/createComposer.tsx` — Draft/selection/undo commands and effects, invoked under the component owner.
- `components/icons.tsx` — Shared navigation/action SVG paths.
- `i18n/T.tsx` — Rich translated messages with named render tags.
- `i18n/en.ts` — English key/type source; whole messages.
- `i18n/index.ts` — Typed translation, locale, interpolation, plurals and safe error mapping.
- `i18n/rich-message.ts` — Parse allowed tag pairs before inserting literal parameters.
- `i18n/uk.ts` — Ukrainian dictionary checked against English keys.
- `index.tsx` — Initialize platform, then mount App.
- `models/channel-browser-model.ts` — Channel name rules and local directory filters; shared Rust name fixtures.
- `models/compose.ts` — Draft keys/storage, outgoing text, client ids and optimistic timestamps.
- `models/conversation-model.ts` — Unread divider selection and conversation-action eligibility.
- `models/emoji-catalog.json` — Generated emojibase catalog; regenerate with scripts/generate-emoji-catalog.mjs.
- `models/emoji-picker-data.ts` — Catalog/category/search/frequency/tone helpers; convenience preferences use localStorage.
- `models/emoji.ts` — Unicode emoji name and skin-tone lookup.
- `models/file-presentation.ts` — Real filenames without bidi controls and fail-closed presentation of core launch eligibility.
- `models/external-link.ts` — Href-free, keyboard-accessible external link interactions through open_url.
- `models/links.ts` — Link kinds and short display labels.
- `models/mrkdwn.ts` — Pure Slack text parsing into safe render nodes.
- `models/presence.ts` — Presence/DND/status expiry and calendar arithmetic.
- `models/rail-model.ts` — Rail selection persistence and message/file summaries.
- `models/rich-compose.ts` — Rich blocks/plain fallback conversion and sanitized draft restoration.
- `models/settings-model.ts` — Preference defaults/types, DM ordering and message text sizing.
- `models/shortcuts.ts` — Physical-key editing shortcuts; preserve IME and AltGr.
- `models/snippet.ts` — FTS control-marker splitting and readable result summaries.
- `models/time.ts` — Localized clocks, relative time and day/list labels.
- `models/virtualizer.ts` — Keyed Fenwick height sums, visible ranges and anchor restoration; local calendar-day bounds.
- `models/switcher.ts` / `sidebar-navigation.ts` — Local fuzzy ranking, sidebar ordering and keyboard targets.
- `models/block-kit.ts` / `slack-link.ts` — Defensive read-only block parsing, fallback detection and encoded deep links.
- `models/unfurl-text.ts` — Website entity decoding into literal render nodes.
- `platform.ts` — Runtime OS detection and platform key labels; see platforms.md.
- `preview/assets.ts` — Self-contained image/emoji/PDF bytes and cache URL overrides.
- `preview/directory.ts` — Synthetic people/profiles, presence and rail/file views.
- `preview/messages.ts` — Synthetic conversations/messages, replies, blocks and files.
- `preview/mock-ipc.ts` — Mock command handlers/events and synthetic send/live-reply helpers.
- `preview.tsx` — Mount App using synthetic IPC/cache fixtures only.
- `stores/outbox.ts` — Optimistic pending/failed messages and authoritative echo matching.
- `stores/panel-sizes.ts` — Desired panel fractions, pixel bounds and legacy size migration; colocated tests.
- `stores/people.ts` — Shared user-name/avatar signals and directory reload.
- `stores/presence-store.ts` — Bounded subscriptions, presence/status signals and expiry clock.
- `stores/profile.ts` — Selected profile signal shared across panels.
- `stores/settings.ts` — Reactive saved settings, theme application and Preferences visibility.
- `styles/base.css` — Unmodified base rules, loaded in original cascade order.
- `styles/channel-browser.css` — Unmodified channel/browser rules, loaded in original cascade order.
- `styles/chrome-search.css` — Unmodified chrome/search rules, loaded in original cascade order.
- `styles/composer.css` — Unmodified composer rules, loaded in original cascade order.
- `styles/file-browser.css` — Unmodified file/browser rules, loaded in original cascade order.
- `styles/files-unfurls.css` — Attachments, filename wrapping, safety warnings and unfurls, loaded in original cascade order.
- `styles/layout.css` — Unmodified layout rules, loaded in original cascade order.
- `styles/menus.css` — Unmodified menus rules, loaded in original cascade order.
- `styles/messages.css` — Unmodified messages rules, loaded in original cascade order.
- `styles/onboarding.css` — Unmodified onboarding rules, loaded in original cascade order.
- `styles/outbox.css` — Unmodified outbox rules, loaded in original cascade order.
- `styles/preferences.css` — Unmodified preferences rules, loaded in original cascade order.
- `styles/presence.css` — Unmodified presence rules, loaded in original cascade order.
- `styles/profile.css` — Unmodified profile rules, loaded in original cascade order.
- `styles/rail-sidebar.css` — Unmodified rail/sidebar rules, loaded in original cascade order.
- `styles/rail-views.css` — Unmodified rail/views rules, loaded in original cascade order.
- `styles/reactions-thread.css` — Unmodified reactions/thread rules, loaded in original cascade order.
- `styles/timeline.css` — Unmodified timeline rules, loaded in original cascade order.
- `styles/viewer.css` — Viewer overlay and full filenames, loaded in original cascade order.
- `styles.css` — Ordered CSS import entry; retains the layout rule comment.
- `views/ActivityView.tsx` — Local activity tabs, paging, unread filters and message jumps.
- `views/ChannelBrowser.tsx` — Cached channel directory, filtering, previews and explicit joins.
- `views/ChannelPanel.tsx` — Cached topic/purpose, members and pins with explicit refresh.
- `views/CreateChannel.tsx` — Explicit channel creation dialog and partial-success notices.
- `views/DirectMessages.tsx` — Stored DM list and group avatars.
- `views/FileBrowser.tsx` — File navigation, filters, paging and viewer/open actions.
- `views/NewMessage.tsx` — Choose an existing conversation for composing.
- `views/Onboarding.tsx` — Sign-in/import presentation without business logic.
- `views/Preferences.tsx` — Preferences shell, state loading, sections and confirmation routing.
- `views/ProfilePanel.tsx` — Cached profile, local clock, presence and DM navigation.
- `views/Rail.tsx` — Workspace navigation and self account-menu anchor.
- `views/Search.tsx` — SearchBar, search results and message jump conversion.
- `views/Sidebar.tsx` — Member conversation groups, muted/starred sections and context-menu targets.
- `views/Thread.tsx` — Thread reads, change reconciliation and reply composer.
- `views/Timeline.tsx` — One timeline lifecycle: measured variable-height virtualization, bidirectional local windows, date/reveal navigation, focused read tracking and scrolling.
- `views/Workspace.tsx` — Workspace DOM composition; state/effects stay in workspace-state.
- `views/preferences/AccountSection.tsx` — Account origin, storage actions and sign-out preferences.
- `views/preferences/SyncSection.tsx` — History/file download limits and resync preferences.
- `views/preferences/context.ts` — Typed preference section dependencies and confirmable actions.
- `views/preferences/controls.tsx` — Reusable preference inputs, cards, switches and theme controls.
- `views/preferences/sections.tsx` — General, appearance, sidebar and media preference sections.
- `views/timeline-parts.tsx` — Day labels and non-member channel join footer.
- `views/workspace-state.ts` — Conversation loading/selection, rail routing, event handling, dialogs and panel sizing.

Folders express ownership: views are screens/panels; components are reusable UI; models hold transformations (some draft helpers also touch DOM/localStorage); stores own shared reactive state. `views/preferences/` stays one feature folder. `components/composer/` is private editor machinery; preview/ never enters production. Layout widths use proportional clamp()/min()/max() with pixel floors; icons, fonts, padding and row heights stay in px. Desired panel sizes persist only on user commits; ResizeObserver measures actual size without overwriting preference.

## File viewer

`document_data` replaces `pdf_data` in the command manifest, UI API and preview. It
requires a stored file, a downloaded canonical regular file inside the cache,
and the core's exact resolved filename/MIME agreement for PDF, XLSX, DOCX or SVG.
Metadata and the read stream both enforce `MAX_DOCUMENT_BYTES` (50 MiB). The UI
shows a localized failure with the existing OS-open action for oversized or
unsupported documents. Other Office formats retain their OS action.

Core supplies `viewer_kind` independently of `can_open`: raster images, SVG,
the existing narrow video set, PDF, XLSX and DOCX may use the viewer. SVG remains
ineligible for OS launch; its OS action reveals it in the file manager. Missing
viewer decisions fail closed in the UI. The safety notice appears only when neither
OS launch nor the viewer is supported; viewer-header OS labels still follow
`can_open`. Timeline, threads and FileBrowser use one download/open action: `fetch_file` reconciles cache hits or uses the existing
manual download path, then returns a fresh SQLite file view. It preserves manual
caps, quarantine and retry/failure semantics. Busy/error state is keyed by file id
so DB-driven thumbnail-to-image replacement cannot discard it. Hidden attachments
remain disabled. Download, OS-action and rendering failures have localized copy.

SVG viewer bytes use the same 50 MiB metadata and streaming cap as other IPC documents.
Timeline, threads and FileBrowser honor `image_previews` and prefer a cached Slack
raster thumbnail. The existing preview queue also selects SVG `thumb_*` URLs from
stored `files.raw_json`, gated by core `viewer_kind`; no new derived state or resync
is needed. Downloads retain the existing size limit, pacing, host policy, failure
memory and pause flag. The preview slot checks the body signature (PNG or JPEG)
before recording a path and before serving it; other bodies are skipped permanently.
When no usable thumbnail exists and the SVG is cached, `document_data` with
`inline_preview=true` delegates to core's SVG-only read, with metadata and streaming
limits of 1 MiB. It never downloads. `SvgPreview` waits until the reserved tile is
within 400 px of the viewport, and falls back to an ordinary card/icon on IPC or
decode failure. Successful loads preserve the tile's reserved dimensions.

Both viewer and inline SVG bytes become a temporary `image/svg+xml` Blob URL used
only by `<img>`: no inline attachment SVG DOM, document navigation, iframe/object/embed or external loads.
Image-context SVG disables scripts and external resources. The URL is revoked on
cleanup (also on inline decode failure), and late IPC completion cannot create a
URL after cleanup. CSP remains unchanged, with `blob:` allowed only in `img-src`; navigation guards,
`frame-src`, `object-src`, media/connect sources, command names and main-only IPC permissions stay unchanged. `flekfile`
continues to reject SVG, including MIME case/parameter variants.

Inline video retains controls, with a viewer expand button and clickable filename.
Opening the viewer pauses inline players; the viewer uses `<video controls>` over
the existing ranged `flekfile` branch (8 MiB cap, HEAD/206/416). Closing by Esc,
backdrop, stage or close button pauses the video, removes its source and calls
`load()` to release it. Codec/protocol errors show the PDF-style failure block
with the OS action. Native codec support remains platform-dependent.

PDF rendering remains lazy pdf.js canvas. OfficeDocument is a separate lazy UI
chunk. It starts a disposable worker, with a 15-second deadline and termination on
close, that dynamically imports read-excel-file (XLSX) or Mammoth (DOCX). No Office
parser runs in Rust or on the UI thread. A streaming fflate preflight bounds actual
inflated bytes to 32 MiB total / 8 MiB per entry, 512 entries, 200,000 XML elements
and nesting depth 128; worksheet grids also have a shared 1,000,000-cell budget; DTD/entity declarations and traversal/duplicate names are
rejected. Validated entries are repacked without compression so subsequent readers
cannot allocate from attacker-controlled ZIP length declarations or spawn blob
workers. SAX filtering removes worksheet rows/columns beyond 1000 × 100 before
the spreadsheet reader allocates dense arrays, including sparse hostile addresses.
The read-excel-file parser uses saved values only: it does not evaluate formulas,
run macros, follow hyperlinks or fetch external references. Intl formats dates
(calendar dates in UTC), numbers and localized booleans.

`models/spreadsheet-layout.ts` reads presentation from those same validated XML
entries inside the worker, without a second ZIP expansion. Column widths use the
Excel character-width approximation (7 px per character plus 5 px padding;
sub-character widths use 12 px per character), with a 64 px fallback. Point sizes
use 96/72 CSS px per point. Sheet defaults, explicit row heights, hidden dimensions
and merged ranges are preserved at 100%. Cell XFs select validated fonts, solid
fills, four borders, alignment and wrapping; RGB, indexed and theme colours resolve
to six-digit hex, including HSL luminance tint. Font families are ignored in favour
of the viewer's fixed system font. Unsupported colours/styles use defaults. No
file CSS, font files, hyperlinks or external resources reach the DOM.

Optional presentation has separate caps: 2,048 entries per style collection,
20,000 XML nodes for styles/theme, 1,024 column ranges and 1,024 merges per sheet.
Overlapping merges also discard the sheet's presentation. Excessive styles fall
back to default styles; excessive worksheet metadata falls back to plain layout,
while saved values remain available. Trailing styled empty cells and merge extents
can extend the value grid only within the existing per-sheet 1000 × 100 and shared
1,000,000-cell budgets. Merges crossing the preview boundary are clipped and mark
the sheet truncated. Only bounded arrays, sparse style indexes, numeric pixel
sizes, hex colours and enumerated style keywords cross the worker boundary.

Sheets retain keyboard tabs, a focusable scroll region and explicit ARIA row and
column indexes. A positioned ARIA table uses row-height prefix sums and binary
search for its visible rows, with overscan; intersecting merged anchors stay
mounted when their first row scrolls away. Hidden dimensions occupy zero space.
Column letters and row numbers stay sticky. Text nodes spill across following
empty cells when left-aligned and unwrapped, stopping at the next occupied cell
or merge; wrapped/merged/aligned cells clip within their own rectangle. Unstyled
cells use a white sheet and light grid lines in both app themes. Custom number
formats, conditional formatting, rich text runs, automatic row-height measurement,
images/charts and exact Excel font metrics are outside this presentation subset.

Mammoth disables external file access and embedded style maps. Only raster embedded
images become data URIs. Its output (at most 2 MiB) passes DOMPurify with a narrow
tag/attribute allowlist before insertion, capped at 20,000 nodes / depth 64. Links
are inert text; no href, CSS, event handlers, ids, remote/SVG images or active embeds
survive. The paper stays white in both themes. Office rendering adds no CSP sources. Cached
`preview_path` thumbnails are reused wherever already present, without new network
requests. No thumbnail support is inferred from a filename.

Dependency choices (exact versions in ui/package.json, locked transitives):

| Library | License | Reason |
| --- | --- | --- |
| read-excel-file 9.3.15 | MIT | A read-only XLSX parser; selected over ExcelJS's broader read/write workbook API and larger distribution. Its universal entry runs inside our worker without creating its own workers. npm xlsx 0.18.x is not used. |
| Mammoth 1.13.0 | BSD-2-Clause | Semantic HTML and embedded images, easy to sanitize; selected over docx-preview's direct DOM/CSS page rendering. Complex Word pagination/fonts are intentionally not reproduced. |
| DOMPurify 3.4.17 | Apache-2.0 OR MPL-2.0 | Maintained sanitizer instead of a custom HTML parser/security policy. All generated HTML still has an explicit minimal allowlist. |
| fflate 0.8.3 / saxen 11.2.0 | MIT / MIT | Bounded streaming ZIP preflight and SAX XML validation/filtering before library allocations. Both are also spreadsheet parser dependencies. |
| jsdom 29.1.1 (dev only) | MIT | Node DOM environment for sanitizer regression tests, compatible with the installed Node 24.10.0. Never bundled. |

Mammoth explicitly warns that its output is not sanitized; Flek never inserts it
without DOMPurify. Main bundle impact is about +5 kB; Office code ships only in
lazy chunks loaded when an Office file is opened.

Regression coverage: app commands/files.rs::document_gate_checks_cache_type_and_size;
UI models/document.test.ts covers detection, real synthetic archives, sanitization,
truncation, formatting and decompression/sparse-address limits. Synthetic viewer
states live in preview/documents.ts, messages.ts and mock-ipc.ts.

## Database and cache

`store::default_path()` returns `<data_dir>/flek/flek.db`; on macOS this is `~/Library/Application Support/flek/flek.db`. See platforms.md for other OSes. One mutex-protected writer uses WAL, synchronous=NORMAL and a 5-second busy budget by default (startup WAL switching also retries SQLITE_BUSY). Readers are read-only/query_only connections pooled up to four idle handles, each with a 5-second busy timeout. In-memory tests share the writer. Use call/with_conn for writes, read/with_read for reads; do not block the async/UI thread with SQLite. `files::default_cache_dir()` holds files by sanitized id/name and public image hashes in `_links`. Non-secret settings.json is adjacent to the DB; credentials belong in CredentialStore/Keychain, never these files. Native window geometry is separate.

Read/parse/validation errors disable desktop auto-import and preserve the broken file. The app and engine options share a last-good settings snapshot (safe defaults before the first valid load), so an invalid file cannot abort a pass. Warnings are logged once per failure episode. `Change::SettingsError { error: Some("settings_unavailable") }` and the runtime-only Settings.load_error field expose the problem through IPC; successful reload emits None. The app forwards this through flek://settings, including diagnostics for late-mounted consumers. Saves refuse to replace an invalid/unreadable existing file; repair or deliberately remove it before saving again. Startup website cleanup still completes, retaining its marker only in memory when preferences are broken, and retries on the next launch.

Preferences displays a localized settings-read warning, including how to repair the file.
The synthetic preview supports `?settingsError&preferences` and live failure/recovery events.

Migrations are compiled by store/migrations.rs from numbered SQL files. Startup inspects schema_version, then rechecks under an IMMEDIATE transaction before applying pending files. Foreign keys are off during rebuilds, checked using pragma_foreign_key_check afterwards, then re-enabled. No checksum is stored; applied text is nevertheless immutable. For future schema work append a numbered file and MIGRATIONS entry, include old-schema/backfill tests, and rebuild any message-derived data from existing raw_json. Migration 0018 adds conversation activity_seen and local thread_reads cursors. Migration 0019 adds a nullable thread_sync.retry_after deadline; existing successful progress is unchanged.

Every table (schema through 0020), with its writer and notable semantics:
- `schema_version`: migration runner; version primary key and applied_at, separate from JSON v1.
- `teams`: directory/auth sync; identity, self_user_id, workspace raw_json/icon; self change recounts unread.
- `users`: directory/live/import; names, bot/deleted flags and raw_json profile fields.
- `conversations`: directory/channel/action sync; kind, label, topic/purpose, is_member gates engine/sidebar. last_read feeds unread triggers; remote_latest drives skip; starred/notification/closed_at are local preferences. message_count counts undeleted messages; latest_ts is newest ever stored; unread_count=NULL means unknown.
- `messages`: backfill/live/send/import/search ingestion; identity=(conversation_id, ts), ts TEXT, integer id for FTS. thread_ts/latest_reply/reply_count, edited_ts, deleted, hidden_by_limit, author_name, blocks_json and raw_json preserve evidence.
- `reactions`: message merges/live actions; per message/name/user, cascading FK; nullable event_ts preserves observed activity time.
- `files`: message merges/cache sync; metadata/raw_json, hidden_by_limit, is_external, local_path, preview_path and auto_download_paused.
- `file_download_failures`: cache sync/manual fetch; separate attachment/preview failure_count, last_failure_at, permanent and retry_after per file.
- `message_files`: message writer; many-to-many file shares, cascading message FK, not one owner per file.
- `sync_state`: backfill; oldest_ts/newest_ts, complete and updated_at; live messages never advance these cursors.
- `thread_sync`: thread backfill; fetched latest_reply and synced_at per conversation/root. Non-auth request failures set retry_after for an hour without advancing fetched progress; background walks, including full resync, defer that root until then. Explicit thread refresh bypasses the delay, and a successful fetch clears it.
- `thread_reads`: local thread panel/live thread_marked cursors; per-conversation/root last_read for Activity. conversations.activity_seen tracks the separate Activity visit cursor (0018).
- `messages_fts`: FTS5 external content over messages.text, stable content_rowid=id, unicode61 tokenizer. INSERT/DELETE/text UPDATE triggers maintain it; migration 0005 rebuilds existing text.
- `link_images`: file/image sync; URL hash, source URL, local_path, fetched_at, failure_count, last_failure_at and retry_after. The failed flag now means a definitive refusal.
- `media_message_changes`: insert/content-update triggers; one increasing position per changed message, initially seeded from existing messages.
- `media_images`, `media_emoji`: incremental media scan; image URLs and emoji names per message, with cascading deletion. Names are independent of the emoji catalog, so catalog changes can resolve previously unknown names without rereading message text.
- `custom_emoji`: directory emoji sync; team/name to URL or alias_of (including standard emoji aliases).
- `custom_emoji_sync`: emoji sync; synced_at per team, including empty results.
- `refresh_runs`: engine; per-team refreshed_at throttles edit-window refresh separately from history progress.
- `conversation_members`: explicit channel-details sync; conversation/user pairs; missing user profiles remain resolvable by id.
- `pins`: channel-details sync; conversation/ts, pinned_by/pinned_at; messages hydrated separately.
- `conversation_details_sync`: channel-details sync; synced_at distinguishes never fetched from a fetched empty set.
- `presence`: snapshot/live/self actions; independent presence/DND freshness and versions, manual/auto/online/snooze state.
- `channel_directory_sync`: explicit channel-browser sync; fetched_at and truncated per team, bounded directory cache.

Automatic file/preview failures survive reconnects, restarts and cache eviction. Transient errors (including transport/timeouts, 429 and 5xx) wait 60 seconds, doubling up to 24 hours. HTTP 403/404/410 stop automatic retries; hidden-by-limit files are excluded before requests. Manual stored fetches bypass these gates, and success clears attachment failures. Preview success clears only preview failures. Cache maintenance preserves failure rows and pauses automatic attachment/preview downloads for cleared files; manual success resumes that file. This prevents a clear from immediately repopulating the cache or retrying unavailable archives. External disk eviction does not discard retry state.

Public-image failures use the same 60-second-to-24-hour backoff. Only 404/410, stopped redirects, URL/DNS policy refusals, non-images and size-cap refusals are permanent; other HTTP/transport/I/O errors remain retryable. Sync checks all recorded public paths once per store/cache at process start. Display/scheme reads check the requested file and queue exact missing-path hints for the next sync pass; normal pass selection uses DB state without statting all images. A missing file clears only its recorded path, preserving newer writes and retry state. Thus a moved database or deleted `_links` directory recovers through paced downloads. On-demand profile-photo requests remain explicit retries.

Media discovery reads through Store::read, at most 1000 dirty messages per batch, parsing text/raw_json outside the writer. Each refresh drains all queue positions up to the maximum observed at its start, yielding between batches so live writes can interleave. A short writer transaction installs image URLs/emoji names and removes only the exact scanned positions; concurrent edits keep their newer positions for the next refresh, bounding work even if a message keeps changing. Thus a migrated archive is indexed in one refresh rather than needing repeated reconnects. Candidate reads combine the persisted index with a bounded read-only overlay of the newest unprocessed messages. Link-image candidates from both sources are ordered by message timestamp descending and deduplicated in that order before applying the per-pass download cap. Reconnects/restarts retain scan progress. Reaction-name reads remain separate. `_links` and the `_id_` encoding namespace are reserved for directories (case-insensitive); attachment names matching `_flek_preview.png` are renamed, and only preview_path can select the reserved preview slot.

Migration 0007 owns counters and uses NULL-safe IS comparisons. Unread means undeleted, newer than last_read, top-level/root/broadcast and from someone else. Read-marker changes recount one conversation. Migration 0016's INSERT trigger clears closed_at only for a newer undeleted message, so replay cannot reopen a DM.

## Invariants and guards

- A successful remote send remains successful if the local write/read fails: sync::send::Sent carries ts and not_stored_locally, app forwards it, and CLI JSON retains message (null if unavailable) plus ts/not_stored_locally. The outbox records remote acceptance, shows a local-sync warning when needed and permits Retry only for failed sends. Only SQLite can replace the provisional message. Client message ids correlate optimistic entries/echoes; remote deduplication of repeated sends is not assumed. send.rs currently contains no edit/delete/react write paths.
- Shared-channel users and archive imports may create stub teams for foreign keys; offline default_team considers only teams with a nonempty self_user_id, while explicit --team and multi-workspace errors remain unchanged. Unknown user_change without a usable team id requests a directory refresh instead of writing an empty team. User/import writes reject empty team ids; existing rows are retained. Simple archive imports resolve destinations with Match::Exact and return candidate errors for prefix-only matches.
- Store::call/with_conn preserve the operation result after permission repair: post-write chmod failures warn once per process and never replace committed success (or the original operation error). Opening a store still requires private permissions.
- Read authoritative messages from SQLite: CLI output::tests::events_use_stored_messages_and_typed_frames; core tests/send.rs and machine_views.rs. UI rule also requires review.
- API history/users treat null text/files/reactions/profile as defaults. users.list skips other malformed items with a count-only warning. History/replies decode per item: malformed payloads retain their readable ts in HistoryPage.skipped_ts, so they count as present during deletion reconciliation and advance sync bounds without overwriting stored content. If any skipped item has no usable ts, HistoryPage.incomplete disables deletion reconciliation for the entire catch-up/thread window; normal pagination and sync completion markers still advance. Backfill follows a supplied cursor even across malformed pages with no readable identity, and terminates when neither an identity nor cursor can advance it. Warnings contain only skipped counts and readable timestamps, never payloads. tests/malformed_page_guard.rs covers stored-content preservation. Null content stubs remain hidden, so the SQL full-content guard and its reaction/file links still apply; api::history::tests::null_stubs_preserve_content_and_malformed_items_keep_identity guards this merge.
- Full content survives hidden/empty versions including attachments: tests/backfill.rs::hidden_version_never_overwrites_full_message and tests/qa_hidden.rs.
- Missing history/replies can imply deletion only newer than sync::FREE_HISTORY_DAYS (90); thread responses must include the root. Incomplete cursor pagination reports IncompleteSync without reconciliation or completion markers. Pins use import-style gap filling, and file tombstones cannot overwrite stored metadata. tests/archive_sync.rs covers these guards, thread retry persistence/recovery/auth propagation and the 0018→0019 migration.
- Edits are ordered; late live events do not resurrect deletion: tests/qa_ordering.rs::older_edit_does_not_replace_newer_edit and late_edit_does_not_resurrect_a_deleted_message.
- Import fills gaps and retains hidden-root metadata/reactions: tests/import.rs and tests/qa_import.rs; never invent a new message identity.
- Live cannot advance catch-up/thread cursors: tests/backfill.rs::live_message_before_catch_up_does_not_skip_offline_ones and tests/live.rs::thread_reply_moves_no_markers.
- Empty completed history can catch up later without sending EPOCH_TS: tests/backfill.rs::empty_conversation_catches_up_later.
- Read markers only move forward; failures preserve state: tests/conversation_actions.rs::mark_is_forward_only_recounts_and_emits_after_success and concurrent_marks_cannot_send_an_older_cursor_after_a_newer_one.
- A complete, validated users.conversations listing atomically reconciles membership for its team; absent conversations become non-members with zero unread counters. Partial/failed/malformed listings cannot clear membership. Read-count refreshes do not recount non-members. Local closed_at, stars and notifications survive reconciliation; archived returned conversations retain membership. tests/directory.rs::only_complete_membership_listing_clears_missing_conversations and malformed_listing_never_clears_membership guard this.
- Non-members stay outside sidebar/unread/engine walks: tests/channel_browser.rs::directory_pages_store_non_members_outside_the_sidebar and tests/agent_search.rs::remote_ingests_merges_without_degrading_and_never_grants_membership.
- Local stars/mute/close survive directory refresh; only newer messages reopen: tests/conversation_actions.rs::leave_and_close_update_db_after_confirmation_and_new_dm_reopens.
- Counters, FTS and cascading relations survive migrations: tests/counters.rs, tests/store.rs and tests/files.rs::files_of_messages_stored_before_m1_6_are_recovered.
- Independent presence/DND versions reject stale snapshots: tests/presence.rs::slow_snapshot_does_not_undo_live_updates_and_self_survives_denied_batch; UI models/presence.test.ts guards presentation expiry.
- Panel preference is not measured size: stores/panel-sizes.test.ts guards bounds/migration; ResizeObserver persistence separation remains a review invariant.
- Opening unread divider is a visit snapshot, separate from live marker: models/conversation-model.test.ts guards firstUnread; Timeline lifecycle requires review.
- Real Slack, owner DB and Keychain are never test fixtures: tests use temp SQLite, wiremock, fake ids and MemoryBackend. No automated guard can replace this operational rule.
- Undocumented calls live only in api/internal.rs, with non-auth fallback; see api-notes.md and tests/directory.rs / engine.rs. New internal calls need an explicit feature/capability check; no unchecked prefs writes.
- Normal transport is paced HTTP plus websocket, respecting Retry-After: tests/api.rs::retries_after_http_429, tests/backfill.rs::limiter_spaces_requests and tests/rtm.rs.

M9 invariants to preserve (M9.3/M9.4 native owner checks remain pending):
- Text/names/unfurls are text nodes; rich drafts are sanitized and paste is plain text. models/unfurl-text.test.ts, models/rich-compose.test.ts and i18n.test.ts cover pure pieces; DOM/paste sanitization still needs review.
- javascript:/file: links never reach the opener: commands/files.rs::open_url permits http/https/mailto and narrowly validated slack://channel deep links; models/rich-compose.test.ts tests editor protocols. commands/files.rs tests the native opener scheme decision without launching anything.
- SQL values use bound parameters; dynamic fragments contain internal expressions only. Agent read/search fixture suites cover quotes/filters; parameter discipline also requires review.
- Downloaded content receives macOS quarantine on its private temporary file before atomic rename, so the final path never appears unmarked. Saved copies and OS actions protect old cache hits too; existing quarantine values (including Gatekeeper approval) are preserved. Attribute read/write failures fail closed, and RAII removes the unfinished temp file. tests/files.rs::download_retries_after_429 and files::safety::tests::save_copy_adds_quarantine_even_when_source_has_none guard the attributes; see platforms.md for flags and Windows work.
- Direct OS open requires the core extension/MIME allowlist using the resolved cached filename. Other types reveal only, with en/uk warnings. files::safety::tests::exact_extension_and_mime_must_agree and tests/files.rs::open_policy_uses_cached_extension_even_with_safe_metadata_name guard the policy; UI consumes core's can_open flag.
- Every DB path used for open/reveal, scheme or document reads must canonicalize to a regular file inside the cache. tests/files.rs::stored_paths_outside_cache_are_refused and cache_containment_resolves_symlinks_and_rejects_directories guard escape rejection. The CLI uses the same core cache lookup.
- Display the real filename with its extension, never replace it with a sender title or truncate its suffix. Strip bidi controls from display and disk names. tests/files.rs::bidi_controls_cannot_disguise_cached_extension, file_views_preserve_real_name_and_strip_bidi_for_display and UI models/file-presentation.test.ts guard this.
- Desktop import and webview sign-in store only the selected/first validated team. Webview validation stops after that first success. Credential saves write the _teams index before the secret entry. Sign-out attempts to persist desktop_import_on_start=false and purge credentials independently, reporting the first error after both attempts. Purging works without an active session and also deletes DB-known team ids; a malformed/unreadable index does not stop those deletions or index removal. Failed deletions stay indexed for retry when possible. auth::security_tests, auth::keychain::security_tests, auth::sign_out_tests and app account tests use MemoryBackend.
- Automatic recovery requires desktop_import_on_start=true, a recorded Desktop origin and the same imported team id (including validated identity). Unknown origins stay unknown. Both boot and engine recovery use core's policy. Rejected credentials are deleted before recovery; invalid_auth/token_revoked/account_inactive cannot survive failed recovery. auth::security_tests guard each branch and secret-free errors.
- A successful user-initiated desktop import in either app or CLI re-enables desktop_import_on_start through core's import_explicit_and_save. Automatic boot imports do not change the preference. Unreadable/invalid settings remain untouched with auto-import disabled, even after a valid explicit import; repair/remove settings.json to restore preference writes. With no stored session and auto-import disabled, CLI errors explain `flek auth import` and settings repair. Sign-in also offers a confirmed clear-saved-session action without requiring a usable session. UI sign-out removes all flek.draft.* keys and the in-memory outbox, including after partial native failure; layout and convenience preferences remain.
- Stored-session validation recovers/signs in only for genuine Slack auth errors. Every other validation error (including proxy HTTP errors, non-JSON captive portals and exhausted rate limits) resumes the stored identity from SQLite when available; credentials are retained and the engine retries connectivity. App boot and CLI sync share this policy. Without a cached identity the validation error is returned. Desktop startup constructs both engine/write clients before publishing Ready; client construction failure clears runtime session state and publishes SignIn with a reason.
- Secret Debug/Display and errors redact credentials: secret::tests, tests/secret.rs and CLI output::tests::credential_shaped_error_text_is_redacted. Flek persists credentials only in the OS credential store, never its logs, settings or archive. Desktop import temporarily copies Slack’s token-bearing LevelDB and cookie database; private modes and cleanup are described below. CLI/app share logging::filter: verbosity enables debug/trace only for Flek targets, and a separate metadata gate caps every dependency at info regardless of RUST_LOG target/span directives. tests/secret.rs covers the gate and a real local websocket handshake through the log bridge.
- Sign-in web content has no IPC: capabilities/main.json grants only main; main.rs registers commands through app manifest/build.rs. The main capability explicitly grants event listen/unlisten and window start-dragging/internal-toggle-maximize (the drag region’s double-click action), plus individual application commands including platform; no core:default or frontend plugin permissions. App permission tests guard this list; native event delivery and dragging require owner QA.
- Sign-in uses an ephemeral window (`incognito(true)`). Capture script reads localConfig_v2 in that window; cookies_for_url reads the same window's native cookie store before destroy. Completion is cancelled and accepted only from the exact HTTPS app.slack.com origin, without userinfo/non-default ports. Page metadata has byte limits and no control characters, is never used in paths/commands or logged, and credentials must pass auth.test before OS-store save. login.rs tests URL lookalikes and metadata boundaries; native persistence/cookie capture requires owner QA.
- Startup cleanup precedes session boot; sign-in retries a failed migration. The app-owned webview_cleanup_version setting is saved only after removal completes, and cannot be overwritten by UI preferences. Sign-out destroys signin and removes legacy Slack data even if credential deletion reports an error. Capture and sign-out share the maintenance lock. browsing_data.rs tests scope, completion-before-marker, failure and repeat behavior; core settings tests legacy defaults/restart persistence. See platforms.md for exactly what is removed.
- Main never navigates outside tauri://localhost or http(s)://tauri.localhost (no custom port/userinfo); debug alone also permits the configured dev origin, including its exact port. Native popups are always denied; http(s) popup URLs go through open_url. Config disables automatic window creation so navigation.rs attaches both guards before main loads. Pure URL/config tests guard this; gestures require owner QA.
- Message/unfurl/profile anchors have no href, cannot drag or expose native link context menus, and open only via open_url. role=link, tab focus, Enter, focus ring and URL titles preserve accessibility. There was no custom copy-link menu on these anchors. models/external-link.test.ts tests interactions and checks integration.
- flekfile serves allowlisted raster images and a separate narrow video branch; videos must also pass core filename/MIME approval and canonical cache containment. Ranged video responses are bounded to 8 MiB, with protected 206/416/HEAD responses; large un-ranged reads fall back to file cards. All responses, including errors, carry nosniff and CSP `sandbox; default-src 'none'`. SVG/HTML/XHTML/XML/unknown types are refused (403), never rendered as documents. No SVG attachment DOM; SVG previews and the viewer use bounded IPC bytes in image-context Blob URLs only, with cached PNG thumbnails preferred for inline previews. Trusted code-owned UI SVG icons remain. files.rs tests active content rejection, headers and PNG/JPEG/PDF response preservation. PDFs use document_data IPC and pdf.js canvas; XLSX/DOCX/SVG use the same bounded IPC and never flekfile; video/PDF thumbnails are PNGs, not embedded documents.
- UI has no direct third-party image fetch: cached images/proxy slack-imgs.com plus tauri.conf.json CSP. It also enforces object-src/base-uri/form-action/frame-src 'none'; no object/iframe/base or form actions are used (forms handle submit locally). tests/unfurls.rs guards proxy/cache parsing; config and UI source regressions guard directives and the PDF canvas path.

- File session headers go only to HTTPS on port 443 at `slack.com`/`*.slack.com`, `slack-edge.com`/subdomains and `slack-files.com`/subdomains. Imported URLs are untrusted; userinfo is refused. The explicitly configured API base's exact origin is the only HTTP mock exception (host, scheme and effective port). RTM requires WSS `*.slack.com`; its mock exception maps WS/WSS to HTTP/HTTPS and still compares the exact origin. net_policy tests plus tests/network_security.rs and tests/rtm.rs guard rejection before requests and valid local handshakes.
- API calls follow redirects only to the API base’s exact origin (scheme, host and effective port), at most five hops, so a multipart token cannot be forwarded to another Slack/CDN origin. File downloads use a separate HTTP client and retain their trusted Slack file-host policy or exact API-origin exception, also bounded to five hops; the Bearer header remains enabled for downloads. Cross-origin file redirects strip Cookie and Authorization in reqwest. Pure API policy and local two-origin network regressions cover refusal and same-origin behavior.
- Public avatars, emoji, workspace icons, profile photos and link images use `files::ImageClient`, which cannot carry a session. Any public HTTPS host on port 443 is accepted, without userinfo; there is no CDN hostname allowlist. Link-preview sources are encoded through IMAGE_PROXY by callers. Image redirects are allowed with a five-hop limit; each destination must pass the same URL and public-IP checks. The reqwest resolver rejects an entire DNS answer containing any non-public IP (IPv4, IPv6, mapped IPv4); the connector uses that checked answer without a second lookup. Environment proxies are disabled. net_policy pure tests guard URL/IP boundaries; unfurls/emoji integration fixtures assert zero HTTP/private requests, with separate synthetic cache hydration checks.
- Image bodies are bounded to 5 MiB before allocation/write using Content-Length and a stream counter. Automatic attachments and PDF/video/SVG raster previews use the smaller of max_file_bytes and declared size + 64 KiB. Manual fetch_stored bypasses the automatic size setting and retry gates: known sizes use declared size + 64 KiB, unknown/negative sizes use the existing default cap. A stored URL can be tried manually even for a hidden file. Convenience download/fetch APIs use the default cap; explicit automatic max_file_bytes=None with unknown size retains the owner's unlimited setting. Unique private TempPaths remove partial data on body, limit, I/O, quarantine or cancellation failures; completed files are quarantined before atomic rename. files::security_tests and tests/network_security.rs guard known/missing/lying lengths, limits and cleanup.
- Unix app-owned data/cache directories are 0700 and DB/WAL/SHM/settings/cache files are 0600, repaired on startup/use. Store precreates the DB privately, chmods sidecars after open/migrations and after writer calls/checkpoints; 0700 parent directories protect sidecars even during SQLite recreation. Shared OS parents are never chmodded. Cache permission repair walks each cache tree once per process; later passes validate/create only directories they use and write private files. Repair skips child symlinks and refuses a symlink root. The app logs that the cache is unavailable and still opens the local archive; cache fetch/read/open/maintenance gates refuse that root instead of following it. tests/network_security.rs checks creation, legacy mode repair and checkpoints. Windows ACL work remains in platforms.md.
- Token-bearing Chromium LevelDB and cookie-database copies use RAII private copies created 0700 with the `flek-chromium-v1-` prefix and copied files 0600. The LevelDB copy is explicitly closed immediately after reading localConfig_v2, before decoding/parsing, cookie decryption/Keychain prompts or network validation. CLI/app startup removes only same-owner, non-symlink prefix directories at least five minutes old, skipping held .flek-import.lock leases. Five minutes gives local copy/lease setup ample grace; the process-held OS lock protects even a slower active importer and is released on SIGKILL. Legacy copies without a lease remain age-gated. Cleanup failure warns without aborting CLI startup. Normal success/error paths remove copies; crashes leave copies until a later scoped cleanup. Removal is not secure erasure. Generic historical `.tmp*` directories cannot safely be attributed to Flek and are never swept. See platforms.md for cleanup limits.

## Conventions and extension checklist

Core returns thiserror `Error` / `Result`: Usage for invalid arguments, NotFound for absent stored entities, Slack { code, needed } for API errors. Preserve typed ErrorKind exit categories. anyhow/context belongs at boundaries; IPC keeps its existing textual errors. Represent partial success explicitly: ChannelAction.warning can report purpose_failed/catch_up_failed after successful creation/join. Keep transformations in tested core functions or UI models; components and commands wire those operations. Do not hide business rules in JSX or IPC adapters. Use t() with whole sentences and params, never concatenate translated fragments. Use `<T>` for rich sentences; parse allowed tags before inserting untrusted params. English keys define the dictionary type; Ukrainian must retain parity and glossary.

SQL is a multiline Rust literal with uppercase keywords, clauses on separate lines, spaces after commas and around operators; wrap long column lists. Bind values, never interpolate user input. Keep SQL literals, GLOB patterns and JSON paths exact. Applied migration SQL is exempt from reformatting. Example:
```sql
SELECT id, label
FROM conversations
WHERE team_id = ?1
AND is_member = 1
ORDER BY label
```

Pure core tests live beside code; cross-module SQLite/HTTP/websocket tests in crates/core/tests. CLI flags, stderr, exits and JSON are guarded by cli/tests/contract.rs. UI uses node --test via npm test (root/models/stores globs), no real IPC or browser; i18n.test.ts recursively scans JSX, excluding preview.tsx and preview/ fixtures. Preview mocks and synthetic assets live in preview/; keep each new IPC shape mocked. Large cohesive files retained: core/search.rs (ordered local/remote ingestion pipeline), store/search.rs (one parser plus unit tests), sync/import.rs (archive decoding plus tests), app/state.rs (engine/session lifecycle), window_size.rs (geometry plus tests), Timeline.tsx (one scrolling/read lifecycle), and i18n dictionaries (one language each).

For a feature (schema changes belong to feature work, not architecture-only passes):
1. Add a core operation and meaningful unit/integration test; reuse typed errors.
2. Add store writes/reads and an append-only migration/backfill only when needed.
3. Add thin Tauri command, register in main.rs/build.rs, grant only main capability.
4. Add api.ts wrapper/types, shared state if needed, then view/component interaction.
5. Add whole-message en/uk translations, pure model tests and preview IPC/fixtures.
6. Update relevant docs and Ukrainian ROADMAP (step status plus Journal); run checks.

Offline checks: cargo fmt; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; in ui/: npm run check; npm test; npm run build. Do not launch app/session commands, use real Slack, run Playwright/WebKit or retain QA scripts for this pass.
