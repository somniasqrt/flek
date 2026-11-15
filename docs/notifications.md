# Desktop notifications (M6.2.1)

`core::notifications::should_notify` is the pure policy. `collect` reads only
committed SQLite messages, applies that policy, counts eligible messages and
hydrates the newest target. Tauri only supplies native focus and delivers the
result using `tauri-plugin-notification` (minimum 2.8.1 for desktop actions).

- Preferences exposes enable, sound and preview toggles, persisted by core.
  Turning previews off hides both author/conversation names and message text.
- Without an explicit choice, DMs/group DMs notify for every supported message;
  channels notify only for direct or broadcast mentions. An explicit `all`
  sends every supported message, including channels; explicit `mentions`
  requires a mention even in DMs. `muted` always sends nothing. These
  preferences remain Flek-only.
- Own, deleted, hidden, already-read, archived and non-member messages, and
  system subtypes, are suppressed. Conversation and local thread read cursors
  both suppress messages already read. A visible, focused main window suppresses
  the conversation actually shown in its timeline. Search, Files, channel
  browsing and composing a new message do not supply a focused conversation.
- Snooze and scheduled DND use the existing self-presence snapshot and
  `Presence::dnd_until`. Unknown/stale DND is quiet until a fresh snapshot or
  event arrives. Messages suppressed during DND/unknown DND are consumed and
  are not replayed when it ends.
- At process startup a committed integer-ID watermark and exact wall-clock
  cutoff exclude existing history and later historical backfill. Only new
  identities after that cutoff qualify; edits and duplicate RTM/history echoes
  keep the same ID and cannot notify twice. Restart does not replay old alerts.
- A 600 ms delivery window combines bursts into one alert. Reconnect holds
  collection until `Change::CaughtUp`: successful catch-up, completed initial
  presence/DND snapshot attempt and drained directory replay. Retry failures
  keep holding collection. One summary then opens the newest eligible message.
- Dock and macOS tray numbers sum cached top-level unread counts, excluding
  muted, closed, archived and non-member conversations. Unknown counts count as
  zero. DND and the banner enable toggle do not erase unread state.
- Clicking a running-process alert shows/unminimizes/focuses main and drains an
  app-owned pending target through main-only IPC. The UI opens the conversation,
  jumps to the message and opens its thread when applicable. Pending targets
  survive a webview remount. The last 100 delivered target IDs are retained in
  memory; sign-out clears them. Clicking older alerts or alerts after process
  termination/restart is unsupported by this runtime-only integration.

No new Slack endpoint is used. Keyword matching and user-group membership are
not implemented; cached membership cannot establish whether a subteam mention
includes self. Native delivery, macOS permission/Focus behavior and xoxc DND
compatibility have not been verified on a real workspace or by launching Flek.
The OS can block notifications independently of Flek preferences. The plugin's
`show()` queues background delivery and cannot confirm that a banner appeared.

## Local preference API

Migration 0021 adds `conversations.notification_explicit INTEGER NOT NULL
DEFAULT 0 CHECK (notification_explicit IN (0, 1))`, without rebuilding the table.
It marks existing `mentions` and `muted` rows explicit. Legacy `all` rows cannot
be distinguished from the old SQL default, so they inherit the kind default;
users who previously chose all for a channel must select it once again.
Directory upserts preserve both local fields.

Write through the existing
`sync::conversation::preference(store: &Store, bus: &Bus, id: &str,
starred: Option<bool>, notification: Option<String>) -> Result<()>` async API.
`Some("all" | "mentions" | "muted")` atomically stores the value and sets
`notification_explicit = 1`, including a choice equal to the inherited default.
`None` preserves both fields, so star-only changes do not make notifications
explicit. Invalid values are rejected before any write. There is no new CLI
setter or reset-to-inherited command.

Tauri keeps `conversation_preference(conversation_id, starred, notification)`;
the UI wrapper remains `api.conversationPreference(id, starred, notification)`.
No caller-supplied explicit flag is accepted: choosing a value sets it in core.
`store::directory::{list_conversations,list_conversations_including}` and
`references::resolve` (Found) return `ConversationRow.notification: String` (raw
stored choice) and `notification_explicit: bool`. These are serialized unchanged
by the existing conversation/preview IPC. UI `notificationPreference(row)`
resolves the menu selection from kind + raw value + explicit flag; mute styling,
Muted grouping and unread logic continue checking the raw `muted` value only.

`notifications::Candidate` remains source-compatible: `preference` is an explicit
all/mentions/muted value, or the in-memory marker `"default"`. SQLite `collect`
projects unchosen preferences to `"default"`, so `should_notify` applies the kind
default. The marker is never stored in the CHECK-constrained preference column.

## Test recipe for Claude

No new tests were added in this implementation. The later Claude-owned
`crates/core/tests/notifications.rs` remains untouched. Its C2 fixture must set
both `notification = 'all'` and `notification_explicit = 1` to assert an explicit
all-posts channel; setting the raw value alone now deliberately inherits mentions.

1. Pure `should_notify`: DM/mpim/channel with all/mentions/muted; exact self
   mentions versus another ID; channel/here/everyone; own messages; deleted,
   hidden, read, archived, non-member and system subtypes; focused same versus
   other conversation; enable off; fresh/no DND, stale/missing DND; scheduled DND
   start/end boundaries and snooze expiry/overlap. Sound/preview do not alter the
   decision.
2. Temporary SQLite `watermark`/`collect`: stored content is authoritative;
   startup/backfill cutoff; two conversations with the same timestamp; duplicate
   upserts and edits preserve IDs; unread own/hidden/deleted rows do not notify; conversation and thread
   cursors both suppress read replies;
   a suppressed batch still advances the cursor; repeat collection is empty;
   mixed eligible/suppressed rows aggregate to the correct count/newest thread
   target; resolved references and 240-character Unicode preview; team isolation;
   rows committed above the returned watermark are collected on the next pass.
3. Temporary SQLite `badge_count`: cached counts, unknown counts and excluded
   muted/closed/archived/non-member rows; mark-read updates and zero clearing.
4. Wiremock plus local websocket follow harness: CaughtUp follows successful
   catch-up and initial snapshot completion; partial catch-up failure/retry and
   delayed directory replay do not release it early; repeated subscription
   snapshots do not produce another CaughtUp. No real session or credentials.
5. Settings old JSON defaults, save/load roundtrip for the three booleans, and
   EN/UK parity. Check old-schema migration (all inherits, mentions/muted explicit),
   star-only flag preservation, explicit selection of the existing raw default,
   directory refresh preservation and UI inherited/explicit selections. App command manifest/main-only capability entries for both IPC
   commands; sign-in webview receives no notification permissions.

## Owner checks

Use `http://localhost:5173/preview.html?preferences&lang=uk` (and `lang=en`).
Check Notifications navigation, keyboard access, disabled dependent controls,
translations and saved toggles. The mock command wrappers are implemented.
In the preview console, `__notificationClick()` opens the synthetic root;
`__notificationClick("C1", "<reply ts>", "<root ts>")` exercises a thread target.
`__notificationContext` exposes the last visible conversation sent to Rust.
Preview does not create OS notifications.

In an installed macOS build, the owner should check: system notification
permission and Focus settings; hidden/minimized/unfocused versus visible focused
conversation; own versus other DM, channel mention and all/mentions/muted;
snooze/scheduled DND and expiry; sound and private previews; duplicate echo/edit;
offline reconnect with many new messages yields one summary; historical startup
backfill is quiet; click from banner and Notification Center opens/highlights the
right conversation/thread; Dock/tray counts clear after reading/muting/sign-out.
Repeat with a hidden window and Preferences/search/Files open. Check behavior
when DND cannot refresh (quiet) and after process restart (old alerts have no
navigation target). Native/visual QA is owner-only.
