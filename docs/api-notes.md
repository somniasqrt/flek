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

- **Status:** UA verified from Slack desktop's own logs; client hints and fetch
  metadata derived, **not yet compared with a DevTools capture** (M2.6.8).
- **Source:** `auth/desktop_profile.rs`, `api/profile.rs`.

Slack desktop logs its user agent in `<profile>/logs/default/browser*.log`
(`"userAgent": "… Slack/4.51.180 Chrome/150.0.7871.114 Electron/43.1.1
Safari/537.36 AppleSilicon Sonic Slack_SSB/4.51.180"`). The newest log may not
contain it (rotation), older ones do. Note the real macOS version (`27_0_1`):
Electron does not freeze it the way Chrome does.

What Flek sends, all derived from the issuer's user agent:

| Header | API calls | file downloads | websocket |
|---|---|---|---|
| `user-agent` | issuer's | issuer's | issuer's |
| `sec-ch-ua`, `-mobile`, `-platform` | Chromium issuers only | same | not sent |
| `origin: https://app.slack.com` | yes | no | yes |
| `sec-fetch-site: same-site`, `-mode: cors`, `-dest: empty` | yes | no | no |

`sec-ch-ua` follows Chromium's GREASE algorithm; Electron has no product
brand, so Chrome 150 inside Slack gives `"Not;A=Brand";v="8", "Chromium";v="150"`.
To check in M2.6.8: these values, whether the desktop calls
`<team>.slack.com/api/…` rather than `slack.com/api/…` (then `sec-fetch-site`
stays `same-site`), extra query parameters (`_x_id`, `slack_route`, …), and
which headers the websocket handshake carries.
