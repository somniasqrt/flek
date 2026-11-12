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
- `store/migrations/` — append-only SQL 0001–0017; schema evolution described below.

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
- `commands/files.rs` — File browsing, fetch/reveal/open, PDF bytes and URL opener IPC.
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
- `components/Viewer.tsx` — Image/PDF overlay and lazy PDF pages.
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

## Database and cache

`store::default_path()` returns `<data_dir>/flek/flek.db`; on macOS this is `~/Library/Application Support/flek/flek.db`. See platforms.md for other OSes. One mutex-protected writer uses WAL, synchronous=NORMAL and a 5-second busy budget by default (startup WAL switching also retries SQLITE_BUSY). Readers are read-only/query_only connections pooled up to four idle handles, each with a 5-second busy timeout. In-memory tests share the writer. Use call/with_conn for writes, read/with_read for reads; do not block the async/UI thread with SQLite. `files::default_cache_dir()` holds files by sanitized id/name and public image hashes in `_links`. Non-secret settings.json is adjacent to the DB; credentials belong in CredentialStore/Keychain, never these files. Native window geometry is separate.

Migrations are compiled by store/migrations.rs from numbered SQL files. Startup inspects schema_version, then rechecks under an IMMEDIATE transaction before applying pending files. Foreign keys are off during rebuilds, checked using pragma_foreign_key_check afterwards, then re-enabled. No checksum is stored; applied text is nevertheless immutable. For future schema work append a numbered file and MIGRATIONS entry, include old-schema/backfill tests, and rebuild any message-derived data from existing raw_json. This pass adds no migration.

Every table (schema through 0017), with its writer and notable semantics:
- `schema_version`: migration runner; version primary key and applied_at, separate from JSON v1.
- `teams`: directory/auth sync; identity, self_user_id, workspace raw_json/icon; self change recounts unread.
- `users`: directory/live/import; names, bot/deleted flags and raw_json profile fields.
- `conversations`: directory/channel/action sync; kind, label, topic/purpose, is_member gates engine/sidebar. last_read feeds unread triggers; remote_latest drives skip; starred/notification/closed_at are local preferences. message_count counts undeleted messages; latest_ts is newest ever stored; unread_count=NULL means unknown.
- `messages`: backfill/live/send/import/search ingestion; identity=(conversation_id, ts), ts TEXT, integer id for FTS. thread_ts/latest_reply/reply_count, edited_ts, deleted, hidden_by_limit, author_name, blocks_json and raw_json preserve evidence.
- `reactions`: message merges/live actions; per message/name/user, cascading FK; nullable event_ts preserves observed activity time.
- `files`: message merges/cache sync; metadata/raw_json, hidden_by_limit, is_external, local_path and preview_path.
- `message_files`: message writer; many-to-many file shares, cascading message FK, not one owner per file.
- `sync_state`: backfill; oldest_ts/newest_ts, complete and updated_at; live messages never advance these cursors.
- `thread_sync`: thread backfill; fetched latest_reply and synced_at per conversation/root.
- `messages_fts`: FTS5 external content over messages.text, stable content_rowid=id, unicode61 tokenizer. INSERT/DELETE/text UPDATE triggers maintain it; migration 0005 rebuilds existing text.
- `link_images`: file/image sync; URL hash, source URL, local_path, failed and fetched_at for cached public images.
- `custom_emoji`: directory emoji sync; team/name to URL or alias_of (including standard emoji aliases).
- `custom_emoji_sync`: emoji sync; synced_at per team, including empty results.
- `refresh_runs`: engine; per-team refreshed_at throttles edit-window refresh separately from history progress.
- `conversation_members`: explicit channel-details sync; conversation/user pairs; missing user profiles remain resolvable by id.
- `pins`: channel-details sync; conversation/ts, pinned_by/pinned_at; messages hydrated separately.
- `conversation_details_sync`: channel-details sync; synced_at distinguishes never fetched from a fetched empty set.
- `presence`: snapshot/live/self actions; independent presence/DND freshness and versions, manual/auto/online/snooze state.
- `channel_directory_sync`: explicit channel-browser sync; fetched_at and truncated per team, bounded directory cache.

Migration 0007 owns counters and uses NULL-safe IS comparisons. Unread means undeleted, newer than last_read, top-level/root/broadcast and from someone else. Read-marker changes recount one conversation. Migration 0016's INSERT trigger clears closed_at only for a newer undeleted message, so replay cannot reopen a DM.

## Invariants and guards

- Read authoritative messages from SQLite: CLI output::tests::events_use_stored_messages_and_typed_frames; core tests/send.rs and machine_views.rs. UI rule also requires review.
- Full content survives hidden/empty versions including attachments: tests/backfill.rs::hidden_version_never_overwrites_full_message and tests/qa_hidden.rs.
- Edits are ordered; late live events do not resurrect deletion: tests/qa_ordering.rs::older_edit_does_not_replace_newer_edit and late_edit_does_not_resurrect_a_deleted_message.
- Import fills gaps and retains hidden-root metadata/reactions: tests/import.rs and tests/qa_import.rs; never invent a new message identity.
- Live cannot advance catch-up/thread cursors: tests/backfill.rs::live_message_before_catch_up_does_not_skip_offline_ones and tests/live.rs::thread_reply_moves_no_markers.
- Empty completed history can catch up later without sending EPOCH_TS: tests/backfill.rs::empty_conversation_catches_up_later.
- Read markers only move forward; failures preserve state: tests/conversation_actions.rs::mark_is_forward_only_recounts_and_emits_after_success and concurrent_marks_cannot_send_an_older_cursor_after_a_newer_one.
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
- Downloaded files receive macOS quarantine after atomic rename; saved copies are explicitly marked. OS actions also protect old cache hits and fail closed on marking errors. tests/files.rs::download_retries_after_429 and files::safety::tests::save_copy_adds_quarantine_even_when_source_has_none guard the attributes; see platforms.md for flags and Windows work.
- Direct OS open requires the core extension/MIME allowlist using the resolved cached filename. Other types reveal only, with en/uk warnings. files::safety::tests::exact_extension_and_mime_must_agree and tests/files.rs::open_policy_uses_cached_extension_even_with_safe_metadata_name guard the policy; UI consumes core's can_open flag.
- Every DB path used for open/reveal, scheme or PDF reads must canonicalize to a regular file inside the cache. tests/files.rs::stored_paths_outside_cache_are_refused and cache_containment_resolves_symlinks_and_rejects_directories guard escape rejection. The CLI uses the same core cache lookup.
- Display the real filename with its extension, never replace it with a sender title or truncate its suffix. Strip bidi controls from display and disk names. tests/files.rs::bidi_controls_cannot_disguise_cached_extension, file_views_preserve_real_name_and_strip_bidi_for_display and UI models/file-presentation.test.ts guard this.
- Desktop import stores only the selected/first validated team. Sign-out clears every indexed Flek credential plus _teams and persists desktop_import_on_start=false. auth::security_tests and auth::sign_out_tests guard selection, cleanup and signed-out boot resolution with MemoryBackend.
- Automatic recovery requires desktop_import_on_start=true, a recorded Desktop origin and the same imported team id (including validated identity). Unknown origins stay unknown. Both boot and engine recovery use core's policy. Rejected credentials are deleted before recovery; invalid_auth/token_revoked/account_inactive cannot survive failed recovery. auth::security_tests guard each branch and secret-free errors.
- Secret Debug/Display and errors redact credentials: secret::tests, tests/secret.rs and CLI output::tests::credential_shaped_error_text_is_redacted. Never serialize Secret to logs/files/fixtures.
- Sign-in web content has no IPC: capabilities/main.json grants only main; main.rs registers commands through app manifest/build.rs. Enforced by Tauri configuration, no browser test in this pass.
- Sign-in uses an ephemeral window (`incognito(true)`). Capture script reads localConfig_v2 in that window; cookies_for_url reads the same window's native cookie store before destroy. Completion is cancelled and accepted only from the exact HTTPS app.slack.com origin, without userinfo/non-default ports. Page metadata has byte limits and no control characters, is never used in paths/commands or logged, and credentials must pass auth.test before OS-store save. login.rs tests URL lookalikes and metadata boundaries; native persistence/cookie capture requires owner QA.
- Startup cleanup precedes session boot; sign-in retries a failed migration. The app-owned webview_cleanup_version setting is saved only after removal completes, and cannot be overwritten by UI preferences. Sign-out destroys signin and removes legacy Slack data even if credential deletion reports an error. Capture and sign-out share the maintenance lock. browsing_data.rs tests scope, completion-before-marker, failure and repeat behavior; core settings tests legacy defaults/restart persistence. See platforms.md for exactly what is removed.
- Main never navigates outside tauri://localhost or http(s)://tauri.localhost (no custom port/userinfo); debug alone also permits the configured dev origin, including its exact port. Native popups are always denied; http(s) popup URLs go through open_url. Config disables automatic window creation so navigation.rs attaches both guards before main loads. Pure URL/config tests guard this; gestures require owner QA.
- Message/unfurl/profile anchors have no href, cannot drag or expose native link context menus, and open only via open_url. role=link, tab focus, Enter, focus ring and URL titles preserve accessibility. There was no custom copy-link menu on these anchors. models/external-link.test.ts tests interactions and checks integration.
- flekfile serves allowlisted raster images and a separate narrow video branch; videos must also pass core filename/MIME approval and canonical cache containment. Ranged video responses are bounded to 8 MiB, with protected 206/416/HEAD responses; large un-ranged reads fall back to file cards. All responses, including errors, carry nosniff and CSP `sandbox; default-src 'none'`. SVG/HTML/XHTML/XML/unknown types are refused (403), never rendered as documents. No inline SVG attachment previews in Files, Viewer or FileBrowser; trusted code-owned UI SVG icons remain. files.rs tests active content rejection, headers and PNG/JPEG/PDF response preservation. PDFs still use pdf_data IPC and pdf.js canvas; video/PDF thumbnails are PNGs, not embedded documents.
- UI has no direct third-party image fetch: cached images/proxy slack-imgs.com plus tauri.conf.json CSP. It also enforces object-src/base-uri/form-action/frame-src 'none'; no object/iframe/base or form actions are used (forms handle submit locally). tests/unfurls.rs guards proxy/cache parsing; config and UI source regressions guard directives and the PDF canvas path.

- File session headers go only to HTTPS `slack.com`/`*.slack.com`, `slack-edge.com`/subdomains and `slack-files.com`/subdomains. Imported URLs are untrusted; userinfo is refused. The explicitly configured API base's exact origin is the only HTTP mock exception (host, scheme and effective port). RTM requires WSS `*.slack.com`; its mock exception maps WS/WSS to HTTP/HTTPS and still compares the exact origin. net_policy tests plus tests/network_security.rs and tests/rtm.rs guard rejection before requests and valid local handshakes.
- Session HTTP redirects follow only those trusted file URLs or the exact API base origin, at most five hops. Cross-origin redirects strip Cookie and Authorization in reqwest; the two-host regression additionally requires no request to an untrusted redirect destination. API and file traffic share this stricter policy.
- Public avatars, emoji, workspace icons, profile photos and link images use `files::ImageClient`, which cannot carry a session. Only HTTPS on port 443, dot-boundary `slack-edge.com`/subdomains, `secure.gravatar.com` and `slack-imgs.com` are accepted. Link-preview sources remain query values sent through IMAGE_PROXY, never directly fetched. The reqwest resolver rejects an entire DNS answer containing any non-public IP (IPv4, IPv6, mapped IPv4); the connector uses that checked answer, without a second lookup. No environment proxies or image redirects are allowed, so neither can bypass resolution checks. net_policy pure tests guard URL/IP boundaries; unfurls/emoji integration fixtures assert zero HTTP/private requests, with separate synthetic cache hydration checks.
- Image bodies are bounded to 5 MiB before allocation/write using Content-Length and a stream counter. Attachment and PDF/video-preview bodies use the smaller of the configured max_file_bytes and declared size + 64 KiB; on-demand stored fetches load the same settings beside the DB. Convenience download/fetch APIs use the default cap; explicit max_file_bytes=None with unknown size retains the owner's unlimited setting. Unique private TempPaths remove partial data on body, limit, I/O or cancellation failures; completed files are atomically renamed before quarantine. files::security_tests and tests/network_security.rs guard known/missing/lying lengths, limits and cleanup.
- Unix app-owned data/cache directories are 0700 and DB/WAL/SHM/settings/cache files are 0600, repaired on startup/use. Store precreates the DB privately, chmods sidecars after open/migrations and after writer calls/checkpoints; 0700 parent directories protect sidecars even during SQLite recreation. Shared OS parents are never chmodded; cache repair skips symlinks. tests/network_security.rs checks creation, legacy mode repair and checkpoints. Windows ACL work remains in platforms.md.
- Chromium session copies use RAII TempDirs created 0700 with the `flek-chromium-v1-` prefix and copied files 0600. CLI/app startup removes only same-owner, non-symlink prefix directories at least 24h old; fresh copies are left to avoid racing an importer. Generic historical `.tmp*` directories cannot safely be attributed to Flek and are never swept. auth::chromium tests cover success/error cleanup, modes and scoped crash cleanup. See platforms.md for cleanup limits.

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
