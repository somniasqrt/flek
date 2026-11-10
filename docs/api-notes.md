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
- **Consequence:** history older than the window can only come from archives
  (`flek import`). What is already in the local DB is never removed by sync:
  a message missing from the window only gets `deleted = 1` when it falls
  inside the refresh window, which is always recent.

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
by the viewer. Unread uses the conversation's `last_read`; this approximates
thread unread state because per-thread read markers are not stored. Unknown
read markers do not manufacture unread badges.

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
