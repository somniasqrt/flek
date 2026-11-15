# Slack API notes

Findings about Slack Web API and websocket behaviour, especially anything
undocumented. Every undocumented call used in `crates/core/src/api/internal.rs`
must be described here first.

How to capture: open Slack in a browser, DevTools → Network. "Fetch/XHR" shows
`api/...` calls, "WS" shows websocket frames. Replace real ids and tokens with
fake ones before writing anything down.

## Template

### `<method or event>`

- **Documented:** yes / no
- **Seen in:** web client action that triggers it
- **Request:** method, params (fake values)
- **Response:** relevant fields
- **Works with `xoxc`:** yes / no / untested
- **Fallback:** what Flek does when it stops working
- **Date checked:** YYYY-MM-DD

---

### `client.counts`

- **Documented:** no
- **Seen in:** the web and desktop clients call it on start-up to draw unread
  badges for every conversation. Shape taken from open-source clients that
  use it (e.g. wee-slack); not captured in DevTools yet, but confirmed by a
  real run (see below).
- **Request:** `POST /api/client.counts`, no parameters besides `token`.
- **Response (relevant fields, fake values):**

  ```json
  {
    "ok": true,
    "channels": [{"id": "C0FAKE", "last_read": "1700000000.000100", "latest": "1700000050.000200",
                  "mention_count": 0, "has_unreads": true}],
    "mpims": [{"id": "G0FAKE", "last_read": "…", "mention_count": 0, "has_unreads": false}],
    "ims": [{"id": "D0FAKE", "last_read": "…", "mention_count": 1, "has_unreads": true}]
  }
  ```

- **Works with `xoxc`:** yes. On 2026-11-08 a real Free workspace returned
  `last_read` for 23 of 28 conversations (the rest have no marker yet).
- **Used for:** `conversations.last_read`, from which `flek channels` counts unread
  messages locally. One call instead of one `conversations.info` per conversation.
  `latest` goes to `conversations.remote_latest`: a sync pass skips a conversation
  whose stored history (`sync_state.newest_ts`) already reaches it, so a start
  with nothing new makes no `conversations.history` request at all. Whether
  `ims`/`mpims` carry `latest` too is not confirmed yet; where it is missing the
  conversation is simply fetched as before.
- **Fallback:** any error except an auth error is logged and ignored; unread counts
  are then shown as unknown (`-`), and every conversation is caught up as before
  (`remote_latest` is cleared). A `latest` that is not a ts string counts as
  unknown. Live `channel_marked` events (M2) update the read marker column.
- **Code:** `crates/core/src/api/internal.rs` → `Client::client_counts`.
- **Date checked:** 2026-11-08.

### `conversations.history` on the Free plan

- **Documented:** yes, but the Free-plan behaviour is not.
- **Observed 2026-11-08:** messages older than 90 days are simply **absent**
  from the response; no content-stripped stubs came back (0 rows flagged
  `hidden_by_limit` out of 281, oldest message ≈ 88 days old, backfill reached
  `has_more: false`). Files may still carry `mode: hidden_by_limit`.
- **Consequence:** older history can come from archives (`flek import`) or
  messages cached before they aged out. Absence from a response is deletion
  evidence only inside the refresh window **and** newer than the shared
  90-day cutoff, enforced in core even for oversized windows and full resyncs.
  The CLI rejects `--refresh-days` above 90. Thread reconciliation applies the
  same cutoff and requires the response to contain the root. Cursor pagination
  reporting `has_more: true` without a usable cursor does not mark deletions or
  advance completion markers; it reports an incomplete sync for retry.
  Non-auth thread request failures are deferred for an hour (persisted across
  restarts), while other threads continue. Explicit thread refreshes can retry
  immediately. Actual `conversations.replies` coverage of old Free-plan threads
  still needs owner verification; these safeguards are covered by mocks.

### System DM `USLACK`

- `users.conversations` lists an `im` whose `user` is `USLACK` (Slack's own
  notifications, distinct from Slackbot `USLACKBOT`). `users.list` does not
  return that id, so Flek labels the DM "Slack".

### Client identity (ADR 0001)

- **Status:** verified 2026-11-08 against Slack desktop 4.51.180 (Electron
  43.1.1, Chrome 150) over the Chromium debugging protocol
  (`Slack --remote-debugging-port=9222`; `SLACK_DEVELOPER_MENU=true` shows a
  Developer menu but no DevTools in this build). Secrets redacted at capture.
- **Source:** `auth/desktop_profile.rs`, `api/profile.rs`, `api/client.rs`.

Slack desktop logs its user agent in `<profile>/logs/default/browser*.log`
(`"userAgent": "… Slack/4.51.180 Chrome/150.0.7871.114 Electron/43.1.1
Safari/537.36 AppleSilicon Sonic Slack_SSB/4.51.180"`); the debugging
endpoint reports the same string. The newest log may not contain it
(rotation). Note the real macOS version (`27_0_1`): Electron does not freeze
it the way Chrome does.

**API calls** (`client.counts`, `conversations.view`, `users.prefs.get`, …):

```
POST https://<team domain>.slack.com/api/<method>?_x_id=…&_x_version_ts=…&_x_gantry=true&_x_desktop_ia=4&_x_frontend_build_type=current&fp=…&_x_num_retries=0[&_x_csid=…&slack_route=…]
accept: */*
accept-language: en-US
content-type: multipart/form-data; boundary=----WebKitFormBoundary…
cookie: d=…; (and others)
origin: https://app.slack.com
priority: u=0, i
sec-ch-ua: "Not;A=Brand";v="8", "Chromium";v="150"
sec-ch-ua-mobile: ?0
sec-ch-ua-platform: "macOS"
sec-fetch-dest: empty / sec-fetch-mode: cors / sec-fetch-site: same-site
user-agent: <the logged UA>
body fields: token, _x_app_name, _x_sonic, _x_reason, _x_mode, + method params
```

- **No `Authorization` header**: the token travels only in the body.
- Host is the team URL from `auth.test`, not `slack.com`
  (only a GET `slack.com/api/api.test` connectivity check goes there).
- History is not fetched with `conversations.history` in this capture:
  `conversations.view` on open and `ublockworkaround.history` (history under a
  name ad blockers do not match).

**Websocket**: `wss://wss-primary.slack.com/?token=…&sync_desync=1&slack_client=desktop&start_args=…&no_query_on_subscribe=1&flannel=3&lazy_channels=1&gateway_server=<team>-3&batch_presence_aware=1`,
headers `origin: https://app.slack.com`, the UA, `accept-language`,
`cache-control/pragma: no-cache`, permessage-deflate. No client hints, no
`sec-fetch-*`.

**What Flek sends now:** the same UA, client hints, `origin`, `sec-fetch-*`
and `accept`; multipart body with the token, no `Authorization`, on
`https://<team>.slack.com/api` once `auth.test` told the team URL. File
downloads still add `Authorization: Bearer` (cookie-only not verified).
Not reproduced: `_x_*` / `fp` / `slack_route` parameters (internal client
telemetry with values Flek cannot honestly produce), `accept-language`,
`priority`, the flannel websocket (Flek uses documented `rtm.connect`).

### Manual browser session entry (M6.3.3)

- Uses the existing documented `auth.test` path on `https://slack.com/api`:
  multipart body containing `token`, plus `Cookie: d=<normalized value>` and
  the existing client profile headers. No new undocumented endpoint is used.
  Successful responses supply `team_id`, `user_id`, `team`, `user`, and `url`
  through the existing `AuthTest` type. Credentials are saved only after success
  (and after matching `--team`, if supplied by the CLI).
- The main sign-in screen hides masked fields under **Advanced**. In browser
  DevTools, use **Application** (**Storage** in Firefox): **Local Storage →
  https://app.slack.com → localConfig_v2 → teams.<team>.token** for the token,
  and **Cookies → https://app.slack.com → d** for the cookie. Both must come
  from the same signed-in browser session. Treat them as passwords; do not paste
  them into shell commands, logs, issue reports, or chat.
- The parser accepts a raw `xoxd-…` value, `d=…` (including quoted values and
  cookie attributes), or a full `Cookie:` header containing exactly one `d`.
  It trims outer whitespace, decodes percent escapes once without form-decoding
  `+`, and percent-encodes the value for the cookie header without double encoding.
  Duplicate `d` entries, invalid prefixes/characters, missing values and oversized
  input fail before any network or credential-store access.
- Manual origin is recorded explicitly using the existing generic desktop Chrome
  profile; Flek cannot infer the issuer's exact browser identity from pasted
  credentials. Manual sessions never qualify for automatic desktop recovery.
  Signing in manually does not change the desktop auto-import preference.
- Rust values use `Secret` (redacted Debug/Display and zeroizing drop). UI fields
  clear immediately on submission, on completion/error, on collapse and on unmount;
  no UI persistence is used. Validation/storage failures expose fixed descriptions.
  This does not erase the OS clipboard or guarantee erasure of JavaScript/IPC copies.
- **Not verified on a real workspace:** this manual browser-copy flow, browser
  storage layout across current browser/Slack versions, raw-versus-encoded cookie
  normalization with a real session, and acceptance of the generic profile for
  a browser-issued session. TODO (owner): verify these paths and credential-store
  prompts with the real app/CLI; agents must use mocks only.

### `chat.postMessage` (sending as the user)

- **Documented:** yes. Not yet captured from Slack desktop, which may use
  another internal method for its own composer; Flek uses the documented one,
  shaped like every other call (multipart, token in the body, team host).
- **Request (fake values):** `channel=C0FAKE`, `text=…`,
  `client_msg_id=<random UUID v4>`, and for a thread reply `thread_ts=<root>`
  plus `reply_broadcast=true` for "Also send to #channel". `text` is what
  the user typed (mrkdwn) with `&`, `<`, `>` escaped as `&amp;`, `&lt;`,
  `&gt;` (Slack's escaping rules), so `<@U…>` typed by hand is sent literally.
- **Response:** `{ok, channel, ts, message}`; `message` is the stored message
  (with `client_msg_id`). Flek fills `thread_ts`, `subtype: thread_broadcast`,
  `user` and `client_msg_id` from the request when they are missing.
- **Storage:** the returned message goes through the live-event path
  (`sync::live::apply`), i.e. an upsert by `(conversation_id, ts)`. The RTM
  echo of the same message, before or after the response, rewrites the same
  row; the root's reply count is bumped idempotently (`note_reply`).
- **Errors:** `channel_not_found`, `not_in_channel`, `is_archived`,
  `msg_too_long`, `restricted_action*`, auth errors → `Error::Slack { code }`,
  worded for people by `api::chat::describe_send_error`. `ratelimited` / HTTP
  429 are retried (`Retry-After`, at most 3 times) by `Client::call`.
- **Works with `xoxc`:** untested by Flek's tests (wiremock only). Owner's
  first real test: the self-DM.
- **Date written:** 2026-11-09.

### `reactions.add` / `reactions.remove`

- **Documented:** [add](https://docs.slack.dev/reference/methods/reactions.add/),
  [remove](https://docs.slack.dev/reference/methods/reactions.remove/).
- **Request:** `channel=C0FAKE`, `timestamp=1700000000.000100`, `name=heart`.
  Both use the existing authenticated client and its retry handling.
- **Storage:** only after success, apply the signed-in user's change through
  the live-event path. The RTM echo is idempotent and other users' reactions
  are preserved. `already_reacted` (add) and `no_reaction` (remove) reconcile
  the local copy to the requested state.
- **UI:** message context menu, hover actions, searchable emoji picker;
  clicking a reaction toggles only the signed-in user's participation.
- **Works with `xoxc`:** not live-tested; verified against wiremock only.
  No real Slack actions are run by the agent.
- **Date written:** 2026-11-09.

### Rich-text composer payloads

- The app sends documented `rich_text` blocks alongside a plain-text fallback
  using `chat.postMessage`'s `blocks` parameter. The CLI still sends mrkdwn text.
- Supported elements: sections, ordered/bulleted lists, quotes, preformatted
  blocks; text with bold/italic/underline/strike/code styles, links and user mentions.
- Typed or pasted HTML is never forwarded. Paste is plain text; editor DOM is
  converted to structured elements, and restored drafts are sanitized.
- The fallback remains searchable in SQLite. `blocks_json` is exposed to the
  UI for rich-text display; unknown block types keep the existing mrkdwn fallback.
- References: [rich text](https://docs.slack.dev/reference/block-kit/blocks/rich-text-block/),
  [text styles](https://docs.slack.dev/reference/block-kit/block-elements/text-element/),
  [chat.postMessage](https://docs.slack.dev/reference/methods/chat.postMessage/).
- Verified with synthetic UI data and mock HTTP only; no live Slack calls.
- Date written: 2026-11-09.

## Agent channel details (M8.2)

`conversations.members` uses documented cursor pagination (`channel`, `limit`,
`cursor`) and `pins.list` returns a single `items` array. Message pin payloads
are merged into the message store before the membership/pin snapshot is
replaced. Non-message pins are omitted. Both endpoints use the existing client,
authentication and rate-limit handling. Behaviour with `xoxc` has **not** been
verified on the real workspace; tests use wiremock and synthetic sessions only.

### `search.messages` (M8.3 agent search)

- **Documented:** [yes](https://docs.slack.dev/reference/methods/search.messages/).
  Slack currently labels this a legacy method; Flek uses it as requested for
  optional server-side discovery, with an offline fallback.
- **Seen in:** not captured from the real web client; official response shapes
  and synthetic wiremock payloads only.
- **Request:** `POST /api/search.messages` through the existing authenticated
  client; `query=release from:<@UFAKE> in:<#CFAKE>`, `page=1`, `count=25`,
  `sort=score` (or `timestamp`), `sort_dir=desc`, `highlight=false`.
  Numbered pagination is used, not the method's optional cursor mode.
- **Response:** `messages.matches[]` carries a message plus `channel` metadata;
  `messages.total`, `messages.paging.pages` or `messages.pagination.page_count`
  describe remote coverage. Profiles/usernames are saved when supplied; unknown
  user ids otherwise stay ids. Search results can omit blocks, files, reactions
  and thread metadata, so they are treated as incomplete evidence.
- **Storage:** every candidate uses the existing history-to-row conversion and
  gap-filling merge. Complete local versions are preserved, including when a
  candidate is empty or has a `hidden_by_limit` file. Unknown channels are stored
  as non-members, with no automatic history sync or sidebar/default CLI listing.
  History/thread sync markers do not advance. Output is always re-read from SQLite
  and locally filtered/ranked; `(conversation_id, ts)` deduplicates matches.
- **Limits:** documented Tier 2, at most 100 matches per page; Flek waits at
  least three seconds before continuation pages, honours shared Retry-After
  retries, and caps each command to three pages / 300 candidates, further
  bounded by its requested result window plus a small margin.
- **Coverage:** remote discovery may include public channels outside membership
  and unsynced content. Free-plan coverage is limited to the recent 90-day
  window; stored archives remain searchable locally. Real workspace behaviour
  for this method and its date/timezone semantics remain unverified. Slack's
  documentation also warns that UI search preferences affect user-token results
  and nearby matches can be collapsed; remote totals are not local row counts.
- **Translation:** phrases, negatives, from/in ids, has filters and query dates
  are translated. Prefixes, repeated OR filters and exact timestamp/relative
  flag bounds cannot be represented identically and are reported in
  `remote_unsupported_filters`; all original filters still apply in SQLite.
  See `docs/cli-json.md` for the precise output and candidate-budget semantics.
- **Works with `xoxc` + `d` cookie:** **unverified**. No real API call made.
- **Fallback:** network/non-auth Slack errors (including `missing_scope` and
  `not_allowed_token_type`) return stored results plus `remote.error`, exit 0;
  human mode warns on stderr. Authentication errors fail normally (exit 5).
  No additional undocumented endpoint is attempted.
- **Date checked:** 2026-11-10 (documentation and mock tests only).

### Presence, custom status and DND (M6.7, 2026-11-10)

**Verification boundary:** official documentation, synthetic wiremock/WebSocket
servers and in-memory SQLite only. All seven new HTTP methods below, presence
subscriptions/queries, batched/single presence and manual/DND/status live events
remain **unverified with `xoxc` + `d` on the real workspace**. The captured URL
above shows `batch_presence_aware=1` for Slack desktop; that does not prove Flek's
RTM session supports any particular presence or DND operation. Flek now explicitly
passes `batch_presence_aware=1` to its documented `rtm.connect` request.

Documented methods, sent through the existing authenticated multipart client:

| Method | Fields / response used |
|---|---|
| [`users.getPresence`](https://docs.slack.dev/reference/methods/users.getPresence/) | `user` = self; `presence`, `manual_away`, `auto_away`, `online`, `connection_count` |
| [`users.setPresence`](https://docs.slack.dev/reference/methods/users.setPresence/) | `presence=away` or `auto`; `auto` restores automatic detection, cannot force an offline user active; refresh self details after success |
| [`users.profile.set`](https://docs.slack.dev/reference/methods/users.profile.set/) | `profile` is JSON with `status_text` (≤100 Unicode code points), `status_emoji` (`:name:`), `status_expiration` (integer Unix seconds); merge returned profile into `users.raw_json.profile` |
| [`dnd.teamInfo`](https://docs.slack.dev/reference/methods/dnd.teamInfo/) | Explicit comma-separated `users`, at most 50 per request; never omit users (which would request the team); empty set makes no call |
| [`dnd.info`](https://docs.slack.dev/reference/methods/dnd.info/) | No user argument: self; schedule and snooze fields |
| [`dnd.setSnooze`](https://docs.slack.dev/reference/methods/dnd.setSnooze/) | Positive `num_minutes`; response can contain only snooze fields, so preserve the stored schedule |
| [`dnd.endSnooze`](https://docs.slack.dev/reference/methods/dnd.endSnooze/) | No arguments; response may be only `ok`; clear snooze, preserve scheduled DND (Resume does not disable a notification schedule) |

[`presence_sub` and `presence_query`](https://docs.slack.dev/apis/web-api/user-presence-and-status/)
are documented legacy RTM messages, implemented in core's connection send path.
Every subscription replaces the entire id set. Each connection and changed set
sends both messages, plus one DND batch snapshot and self `dnd.info` /
`users.getPresence`; there is no presence polling. CLI follow makes no presence
HTTP requests unless given a subscription receiver. The UI requests non-archived
DM partners, self and the open profile; no channel members, search authors or
message authors. The set is capped at 500 (self and the open profile first in the
UI); extra people stay unknown/grey. Newly added ids are invalidated until fresh
state arrives. Reconnection repeats subscriptions and snapshots. Snapshot source
versions prevent slow responses from overwriting newer live updates.

Live payloads used:

- [`presence_change`](https://docs.slack.dev/reference/events/presence_change/):
  `{type, user, presence}` or `{type, users: [...], presence}`. Update each named
  person's presence, preserving a known self manual-away override.
- [`manual_presence_change`](https://docs.slack.dev/reference/events/manual_presence_change/):
  `{type, presence}` (self implied; explicit `user` also accepted). `away` sets
  the manual override; returning to automatic detection clears it. The following
  presence event determines active versus away, without manufacturing green.
- [`dnd_updated`](https://docs.slack.dev/reference/events/dnd_updated/): self,
  `{type, user?, dnd_status: {dnd_enabled, next_dnd_start_ts,
  next_dnd_end_ts, snooze_enabled?, snooze_endtime?}}`.
- [`dnd_updated_user`](https://docs.slack.dev/reference/events/dnd_updated_user/):
  another named user, same nested schedule shape. Snooze details may not be
  available for other users; Flek cannot infer AFK or paused notifications when
  Slack does not disclose them.
- [`user_change`](https://docs.slack.dev/reference/events/user_change/): merge the
  stored raw user/profile, including custom status, preserve the existing team
  when `team_id` is omitted, then emit Directory so open profiles and DM labels /
  statuses re-read SQLite. Unknown event types keep the existing fallback.

**Indicator mapping** (core and pure UI helpers): stale/unknown → hollow grey;
self `manual_away=true` → hollow grey; otherwise fresh `snooze_enabled` with
`snooze_endtime > now` or `dnd_enabled` inside `[next_dnd_start_ts,
next_dnd_end_ts)` → moon; otherwise `presence=active` → green, except self
`auto_away`, `online=false` or `connection_count=0` → grey. `presence=away` →
grey. DND wins over active/automatic away, but the explicit self manual-away
override wins over DND. A moon means disclosed DND, not an inferred third Slack
presence value. Separate presence/DND freshness flags reset at app startup,
disconnect and shutdown; a DND update alone cannot revive yesterday's green.

Status expiration `0` means never. The UI hides expired text and emoji locally
without a network request. Today = local 23:59:59; This week = local Sunday
23:59:59; duration options add seconds; calendar arithmetic respects local DST.
Until tomorrow snoozes to local 09:00, rounded up to whole minutes. Theme and
clock are local UI concerns; writes persist only after Slack succeeds. Failed
writes leave the DB unchanged and surface an inline error in the menu/dialog.
If automatic presence succeeds but the detail read fails, persist the confirmed
override and show grey until live state arrives. No new undocumented endpoint
or workspace-wide fallback is used: denied snapshots remain grey, and existing
message/history sync continues. Owners can use the official client if a setter
is unsupported by their session.


## Workspace icon and local rail views (M6.8, 2026-11-10)

[`team.info`](https://docs.slack.dev/reference/methods/team.info/) is documented
(`team:read`). Directory sync requests the current workspace, stores only the
returned `team` object in `teams.raw_json`, and selects `icon.image_132`, falling
back to `icon.image_88`. `icon.image_default=true` is ignored. The image uses the
same public, session-free CDN downloader and `link_images` cache as avatars.
The UI reads only a cached hash; missing metadata, failed downloads or an
unsupported method leave the initials fallback. No render triggers team.info.
**Verified only with synthetic payloads/wiremock; xoxc+d support remains
unverified. No real Slack or Keychain was used.**

Activity is entirely local (`core::rail::activity`), paginated and reusable by a
future CLI command. Mentions share the indexed candidate query with
`reads::mentions`; only direct `<@ME>` mentions are included. User-group
membership is not stored, so group/broadcast mentions are not inferred. DMs
and threads each contribute one latest message from someone else per
conversation/thread. Thread participation includes roots and stored replies
by the viewer. Unknown read markers do not manufacture unread badges.

Activity read state (migration 0018) has three local cursors; an item is read
up to the latest that applies:

- `conversations.last_read`, as before, for every kind.
- `thread_reads.last_read` for thread replies (threads and mentions in
  threads). Set by the thread panel when it is open in a focused window, and by
  the `thread_marked` RTM event from other clients. Slack's thread cursor write
  (`subscriptions.thread.mark` in the web client) is undocumented and not
  called, so Slack itself still sees threads read in Flek as unread.
- `conversations.activity_seen` for reactions and top-level mentions: when a
  client last looked at the conversation. Set to the receive time of any
  `channel_marked`/`im_marked`/… event (from any client) and to the item time
  when an Activity item is opened. A conversation marker never reads threads.

Slack has no documented read state for reactions; reading a conversation on
another client without moving its marker (no new messages) does not clear them.

`thread_marked` is undocumented. Expected shape, parsed defensively (anything
else is ignored): `{"type":"thread_marked","subscription":{"type":"thread",
"channel":"C…","thread_ts":"…","last_read":"…","unread_count":0,…}}`.
**Not yet confirmed on the real workspace** (DevTools → WS frames, read a
thread in the browser).

[`reaction_added`](https://docs.slack.dev/reference/events/reaction_added/)
already identifies the reacting `user`; migration 0015 adds nullable
`reactions.event_ts` and rail query indexes. The first observed live event time
survives duplicate events and history refreshes for a retained reaction.
Backfilled/old reactions have no event time: display them at the message time,
always read. Removed reactions disappear; the viewer's own actions are excluded.
The Activity badge counts unread items across kinds (an item may be both a
mention and a thread reply).

Files extends the existing `reads::files` reader with literal name/title search,
newest/name/size sort, viewer filtering and offset pagination. The UI's Other
category includes audio/video; existing CLI type semantics remain unchanged.
The additive `title` field preserves the original `name` in the CLI contract.
Downloads reuse the stored-file cache path and existing authenticated fetch;
paths are persisted and change events published before UI re-reads. Hidden
files may open/download from an existing cache, but cannot trigger a fetch.
The compose picker selects an existing local conversation; creating a DM with
a person outside the stored conversation list remains a separate feature.


## Conversation actions and notification preferences (2026-11-10)

Documented methods used by the sidebar/read tracking:

- `conversations.mark`: POST `channel`, `ts`; success `{ "ok": true }`.
  Core serializes marks, skips cursors at/before SQLite's `last_read`, waits for
  success, then advances SQLite and emits `ReadMarker`. Errors keep local state.
  UI debounces fully visible messages while focused/visible; thread subscription
  read state uses a separate undocumented API and is deliberately out of scope.
- `conversations.leave`: POST `channel`; success `{ "ok": true }`, then
  `is_member = 0`. Known `is_general` channels are disabled; Slack's
  `cant_leave_general` error is surfaced without changing membership.
- `conversations.close`: POST `channel`; success `{ "ok": true }` (including
  `no_op` / `already_closed`), then local `closed_at = latest_ts`. A newer stored
  message clears it; replaying older history and directory refresh do not.

References: [mark](https://docs.slack.dev/reference/methods/conversations.mark/),
[leave](https://docs.slack.dev/reference/methods/conversations.leave/),
[close](https://docs.slack.dev/reference/methods/conversations.close/).
All three are tested only with wiremock; **xoxc+d support remains unverified**.
No real workspace or Keychain was accessed.

### Undocumented prefs — unverified, NOT implemented

Candidate methods are `users.prefs.get` / `users.prefs.set`. The proposed read
shape is `{ "ok": true, "prefs": { "all_notifications_prefs": "<JSON string>",
"muted_channels": "C1,C2" } }`. A proposed write is form fields `name` and
`value` (for example `name=muted_channels`, `value=C1,C2`), with response
`{ "ok": true }`. The exact per-channel keys/values inside
`all_notifications_prefs`, merge behavior, and accepted write shape are **not
verified**. This is a research note, not a captured request/response contract.
Blindly replacing workspace preference blobs risks losing unrelated settings.

The permitted fallback is implemented instead: migration 0016 stores Flek-local
`notification = all | mentions | muted` and `starred`. Migration 0021 adds
`notification_explicit`: absent an explicit choice, im/mpim defaults to all and
channels to mentions. Choosing any notification value sets the flag in core;
star-only writes preserve it. Existing mentions/muted migrate as explicit;
legacy all is ambiguous and inherits the kind default. The raw preference
remains unchanged for mute/unread consumers. The UI labels notification
choices “In Flek only”; stars also do not sync to Slack (sections/stars APIs are
undocumented). No internal prefs calls, parsing, directory prefs fetch, or writes
are enabled. Mute suppresses unread emphasis/badges and Activity unread, and
moves Home items into the collapsed Muted group. All/mentions are saved choices;
OS notification delivery is still the separate planned M6.2.1 feature. A future
prefs sync must live in `api/internal.rs`, be capability-gated, merge confirmed
current prefs, and leave SQLite unchanged on any unsupported/error response.

## Channel directory, join and create (M6.11)

Documented methods, **unverified with an `xoxc` session + `d` cookie**:

- `conversations.list` with `types=public_channel,private_channel`, `exclude_archived=false`, `limit=200`, cursor paging. Private channels come back only when the user is a member. Flek fetches it only when the channel browser opens (cache older than 1 hour) or on an explicit refresh, never in background sync, and stops after 20 pages (`truncated` is stored and shown). Results are stored as conversations; unknown ones get `is_member = 0`, so the sidebar and the sync engine ignore them.
- `conversations.history` on a public channel the user hasn't joined: one recent page for the read-only preview. Slack documents this as allowed for public channels; if it fails, the preview shows what is stored.
- `conversations.join` returns the channel; Flek sets `is_member = 1` and catches the conversation up (a failed catch-up is a warning, not a failed join).
- `conversations.create` (`name`, `is_private`) then optional `conversations.setPurpose`. Names are lowercased and whitespace becomes `-` before sending; letters of any script, ASCII digits, `-` and `_` are accepted, 80 characters at most (`crates/core/tests/fixtures/channel-names.json` is shared by the Rust and UI tests). Slack's own errors (`name_taken`, `invalid_name_*`, `restricted_action`) are shown translated.


## CLI channel management and own status (M8.5)

Only documented methods are used. All request/response shapes below are based
on Slack's reference; compatibility with `xoxc` + `d` is **not verified on a
real workspace**. No Slack session, Keychain or real app was used to verify this
implementation.

| Method | Request fields (authentication omitted) | Success data used |
|---|---|---|
| [`conversations.create`](https://docs.slack.dev/reference/methods/conversations.create/) | `name`, `is_private` | `channel`; existing M6.11 create/store path |
| [`conversations.join`](https://docs.slack.dev/reference/methods/conversations.join/) | `channel` | `channel`; existing join/store/catch-up path |
| [`conversations.invite`](https://docs.slack.dev/reference/methods/conversations.invite/) | `channel`, `users` (one resolved user id) | `ok`; the returned channel is not treated as a complete member list |
| [`conversations.kick`](https://docs.slack.dev/reference/methods/conversations.kick/) | `channel`, `user` | `ok` |
| [`conversations.leave`](https://docs.slack.dev/reference/methods/conversations.leave/) | `channel` | `ok` |
| [`conversations.setTopic`](https://docs.slack.dev/reference/methods/conversations.setTopic/) | `channel`, `topic` | `channel.topic.value`; a top-level `topic` string or the confirmed requested value is a compatibility fallback |
| [`conversations.setPurpose`](https://docs.slack.dev/reference/methods/conversations.setPurpose/) | `channel`, `purpose` | top-level `purpose` string, otherwise confirmed requested value (reference also permits `ok` only) |
| [`conversations.rename`](https://docs.slack.dev/reference/methods/conversations.rename/) | `channel`, `name` | `channel.name` (server normalization), otherwise confirmed submitted normalized name |
| [`conversations.archive`](https://docs.slack.dev/reference/methods/conversations.archive/) | `channel` | `ok` |
| [`users.profile.set`](https://docs.slack.dev/reference/methods/users.profile.set/) | `profile`: JSON containing `status_text`, `status_emoji`, `status_expiration`; no `user` field | returned `profile`, merged through M6.7 core status code |

The prepared request (dry-run report) omits credentials and shows `profile` as
the JSON string that is sent in the API form field. Status
clear submits `{"status_text":"","status_emoji":"","status_expiration":0}`.
No undocumented membership/status endpoint or fallback write is enabled.
Membership deltas are idempotent and raw metadata is updated alongside columns.
The existing RTM conversation-change path still schedules directory refresh;
profile echoes merge through `user_change`. Neither path adds member counts,
so local confirmed writes do not double-count their echoes. Complete membership
snapshots remain explicit `channel info --fresh` work.

TODO (owner verification): confirm all above methods with the owner's session,
including private-channel restrictions, canonical rename/topic/purpose responses,
emoji/expiration acceptance, live echoes and join catch-up. The response-value
fallbacks for topic/rename are not verified on a real workspace. Do not add an
undocumented alternative until its shape is captured and documented.

### Stored file thumbnails (`thumb_*`)

- **Source:** optional fields of Slack file objects already stored in `files.raw_json`;
  no additional endpoint or request parameters. Recorded file-download fixtures
  cover `thumb_pdf`, `thumb_pdf_w` and `thumb_pdf_h`; the SVG cases are synthetic,
  and availability/format of thumbnails on real SVG uploads remains unverified.
- **Fields used:** PDF/video previews retain `thumb_pdf` / `thumb_video` and their
  `_w` / `_h` dimensions. For files core recognizes as SVG, the existing preview
  queue instead chooses the first nonempty URL in this order: `thumb_480`,
  `thumb_360`, `thumb_720`, `thumb_960`, `thumb_1024`, `thumb_160`, `thumb_80`,
  `thumb_64`. Size-specific thumbnail fields on other file types are not added
  to the queue. Tile dimensions use `original_w` / `original_h`, or fixed defaults.
- **Format:** the existing `_flek_preview.png` cache slot and
  `flekfile://localhost/<id>/preview` hold a PNG or JPEG, recognized by the
  body's signature and served with that type. Any other body is removed and
  recorded as a permanent preview failure, and the scheme refuses such bodies
  already in the cache. Image decoding can still reject a damaged image, in
  which case the UI tries the cached SVG fallback.
- **Policy:** same authenticated file downloader, trusted HTTPS Slack hosts,
  redirect checks, byte cap, request pacing, per-preview retry memory and
  `auto_download_paused` gate as PDF/video thumbnails. Existing rows are selected
  directly from stored `raw_json` on the next eligible pass; no full resync or
  derived-table migration is required.
- **Fallback:** no usable cached PNG plus a cached SVG uses the existing
  `document_data` command with `inline_preview=true`. Core checks the resolved
  extension/MIME and caps both metadata and streamed bytes at 1 MiB. UI requests
  it lazily by file id and uses only `<img src=blob:>`; oversized/broken SVGs keep
  their ordinary card/icon and can still open in the existing 50 MiB viewer.
- **Date checked:** 2026-11-14, source review and synthetic fixtures only; no live
  Slack traffic.

## M6.2.1 notification inputs

No additional endpoint or undocumented preference blob is used. Notification
policy reads committed `messages.text`, `user_id`, `subtype`, `thread_ts`,
conversation-local all/mentions/muted preferences, `last_read`, and the existing
M6.7 self DND snapshot/events. Direct `<@USER_ID>` and broadcast `<!channel>`,
`<!here>`, `<!everyone>` text tokens are recognized. User-group membership and
keyword preferences are not fetched. New native notifications and the existing
DND methods with xoxc+d are **not verified on a real workspace**.

TODO (owner verification): confirm self DND snapshots/events and mention text
fallbacks for the target workspace. Keep subteam/keyword support disabled until
there is a verified membership/preference source. See [notification behavior and
test recipe](notifications.md); missing/stale DND currently suppresses banners.

### Guarded CLI writes (M8.4 / M8.7)

All methods below are documented Web API calls sent through the shared client
(`crates/core/src/api/write.rs` allow-lists exactly these and refuses any
`token`/`cookie`/`authorization` parameter). None is verified on a real
workspace with an `xoxc` session; fixtures are synthetic.

| Method | Fields | Notes |
|---|---|---|
| `chat.postMessage` | `channel`, `text`, optional `blocks` (JSON), `thread_ts`, `reply_broadcast`, `client_msg_id` | `ts` and `channel` come from the response; the permalink is built locally from the cached workspace URL, falling back to `chat.getPermalink` |
| `chat.scheduleMessage` | `channel`, `text`, optional `blocks`, `thread_ts`, `reply_broadcast`, `post_at` | Returns `scheduled_message_id`; nothing is stored locally (a scheduled message is not in history yet). Slack limits `post_at` to 120 days |
| `conversations.open` | `users` (comma separated, 1 to 8), `return_im=true` | Response `channel` object is upserted so the conversation is resolvable offline |
| `conversations.mark` | `channel`, `ts` | Reuses the monotonic mark logic; skipped when the local read cursor is already at or beyond `ts` |
| `reactions.add` / `reactions.remove` | `channel`, `timestamp`, `name` | `already_reacted` / `no_reaction` are treated as success |
| `pins.add` / `pins.remove` | `channel`, `timestamp` | `already_pinned` / `no_pin` are treated as success; local `pins` table updated after confirmation |
| `stars.add` | `channel`, `timestamp` | Legacy stars. The modern "Later" list has no documented API, so `flek save` sets only the legacy star and warns (`legacy_star_only_later_api_unavailable`). **Unverified:** whether a legacy star appears in the modern "Later" view of the target workspace. No undocumented endpoint is used |

- **Fallback:** none needed beyond the Open in Slack deep link; errors surface
  as typed CLI errors (`auth`, `rate_limited`, `network`).
- **Date checked:** 2026-11-15, source review and wiremock fixtures only.

## Windows desktop import (M7.1.2)

No new endpoint is used. Session validation retains the existing `auth.test` path.
Local-profile assumptions and Windows fallback behavior are in
[platforms.md](platforms.md#slack-desktop-session-import).

TODO: normal/Store profile paths, `Local State` key availability, `v11` AES-GCM
cookies, cookie host hashes and the availability of a complete desktop user agent
in Windows logs are **not verified on a real workspace**. `v10` follows Chromium
source; unsupported encryption (including app-bound `v20`) and locked stores fail
with webview sign-in available. Do not add an undocumented endpoint or invent a
desktop user agent to work around an unavailable local profile.

## Local search materials (M5.1.4)

`reads::materials` / the main-window-only `search_materials` command read SQLite
only. No Slack endpoint or additional credential access is used. Migration 0023
creates a material catalogue and its FTS5 index, then atomically backfills all
stored messages in bounded batches via `store::materials`. Message writes refresh
it after hidden/edited/import merge guards, inside the same transaction. File
metadata updates refresh previous shares; message deletion removes catalogue rows.

Links come from HTTP(S) text/mrkdwn, documented rich-text `link` elements and Block
Kit text, and the already-used attachment `original_url` / `from_url` / `title_link`
and title fields (see the existing unfurl reader). Attachment text/pretext and
blocks are included. Image URLs, service icons and `link_images` cache entries are
not material shares. No arbitrary JSON URL recursion or third-party fetch occurs.
One URL is deduplicated within a message; separate shares retain their own message
and thread context. File rows reuse `message_files` and `files`, and display real
filenames with extensions. Files have no new download/open path here.

An empty query browses all materials. Existing `in:`, `from:`, `after:` and
`before:` parsing and UTC bounds apply; free text also matches material names,
URLs and the surrounding message text. `has:link` and `has:file` select material
kinds; using both includes both kinds. Sorts are newest, oldest and name (SQLite
case folding, deterministic row-id tie breaks). The core clamps pages to 1–200,
uses indexed keyset cursors bound to the team/query/sort, and hydrates only the
selected page. It does not compute a full result count. UI catalogue invalidation
resets pagination when the workspace revision changes; cursors are not database
snapshots during concurrent edits.

No new live Slack compatibility assumptions are introduced. Visual/native QA is
not verified on a real workspace; use the synthetic preview recipe in
`ui/src/preview/README.md` first.
