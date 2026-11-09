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
- **Fallback:** any error except an auth error is logged and ignored; unread counts
  are then shown as unknown (`-`). Live `channel_marked` events (M2) update the
  same column.
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
