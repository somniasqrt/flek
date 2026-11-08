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
  use it (e.g. wee-slack); **not yet verified in DevTools for this workspace**.
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

- **Works with `xoxc`:** expected (it is what the web client itself uses); untested.
- **Used for:** `conversations.last_read`, from which `flek channels` counts unread
  messages locally. One call instead of one `conversations.info` per conversation.
- **Fallback:** any error except an auth error is logged and ignored; unread counts
  are then shown as unknown (`-`). Live `channel_marked` events (M2) update the
  same column.
- **Code:** `crates/core/src/api/internal.rs` → `Client::client_counts`.
- **Date checked:** 2026-11-08 (from public sources only).
