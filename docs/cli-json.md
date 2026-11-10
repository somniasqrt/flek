# CLI JSON contract (schema version 1)

Pass `--json` anywhere among the command options. Successful finite commands
write exactly one compact JSON object followed by a newline to stdout. Every
object has `"schema_version": 1`. Empty lists remain arrays. Human output is
unchanged when the flag is absent. `--help` and `--version` retain clap's text
output. JSON mode suppresses tracing/progress text, including `RUST_LOG` output,
so stderr is reserved for a machine-readable failure.

Consumers must ignore unknown fields and stream event types. Adding fields or
new event types keeps the schema version. Removing or renaming a field, changing
its type, or changing its meaning requires a version bump. This version is
independent of SQLite's migration version (`db_schema_version`).

## Command envelopes

The table lists payload fields alongside the common `schema_version: integer`.

| Command | Payload |
|---|---|
| `whoami` | `url`, `team`, `team_id`, `user_id`, `user_name`: strings; `user`: User or null from the database; `enterprise_id`: optional string; `client`: object with `issuer`, `user_agent` strings and `client_hints` string-to-string map |
| `db` | `path`: string; `db_schema_version`: integer |
| `channels` | `conversations`: Conversation[]; archived conversations excluded unless `--archived` |
| `history <conversation>` | `conversation`: Conversation; `messages`: Message[] (oldest first) |
| `search <query>` | `results`: Message[] with an additional `snippet: string` on each result (`**` marks matches); ordering follows relevance or `--recent` |
| `send <conversation> <text>` | `message`: Message, re-read from the database after sending |
| `import <path>` | `summary`: `{format: string, read: integer, written: integer, unknown_channels: string[]}` |
| `sync` | `summary`: `{conversations: integer, messages: integer, deleted: integer, failed: integer, files_downloaded: integer}` |
| `auth list` | `teams`: string[] of stored team ids |
| `auth import` | `teams`: objects with `team_id`, `name` strings and `ok` boolean; successful rows have `user`, `user_id` strings (authentication identity labels); failed rows have `error: string` |
| `auth logout` | `removed`: string[] of removed team ids |

`whoami`'s authentication label `user_name` and the per-team `auth import` label
are identity metadata; all stored user entities use the User shape below.
No token, cookie, or raw session object is included.

Examples:

```json
{"schema_version":1,"conversations":[]}
```

```json
{"schema_version":1,"conversation":{"id":"C1","name":"general","label":"general","kind":"channel","is_archived":false,"is_member":true,"topic":null,"purpose":null,"unread_count":null,"last_read":null,"latest_ts":null,"synced_at":null,"history_complete":false},"messages":[]}
```

## Message

The same core view is used in history, search, send and message stream events.
Slack timestamps are strings; do not convert them to floating point.

| Field | Type / meaning |
|---|---|
| `conversation_id` | string |
| `conversation` | string, stored display label |
| `ts` | string, Slack seconds.microseconds identifier |
| `datetime` | string, RFC 3339 UTC (`Z`), including fractional seconds when present |
| `thread_ts` | string or null; null for a top-level message, including a root whose stored `thread_ts` equals its own `ts` |
| `reply_count` | integer |
| `user_id` | string or null |
| `user` | string or null, display name (fallback: handle/imported author) |
| `text` | string, raw stored mrkdwn |
| `text_resolved` | string; user/channel references resolved from the DB; labeled URLs become `label (url)`; HTML entities decoded after reference parsing |
| `edited` | boolean |
| `deleted` | boolean |
| `hidden_by_limit` | boolean |
| `files` | File[]; empty if no attachments |
| `reactions` | Reaction[]; empty if none, ordered by name |
| `permalink` | string or null if the team's URL is unknown |

Unknown mentions use the id or the supplied channel label. Entity decoding is
one pass (`&amp;lt;` becomes `&lt;`), and decoded angle brackets are not reparsed as
mentions. Supported entities are `amp`, `lt`, `gt`, `quot`, `apos`, `nbsp` and
numeric decimal/hex entities. Unknown entities remain literal.

Permalinks use the stored team URL, without API calls:
`https://fake.slack.com/archives/C1/p1700000000123456`. Replies append
`?thread_ts=1700000000.123456&cid=C1`. History/search visibility rules continue to
exclude deleted messages, and history excludes ordinary replies unless
`--thread` is supplied. The shared Message view can represent deleted records
for integrations that request them explicitly.

| File field | Type |
|---|---|
| `id` | string |
| `name` | string or null |
| `mimetype` | string or null |
| `size` | integer or null, bytes |
| `local_path` | string or null, cached file path |

| Reaction field | Type |
|---|---|
| `name` | string, emoji name without colons |
| `count` | integer, number of stored reacting users |
| `users` | string[], user ids sorted ascending |

## Conversation

| Field | Type / meaning |
|---|---|
| `id` | string |
| `name` | string or null (DMs may have no name) |
| `label` | string, display label |
| `kind` | `"channel"`, `"private"`, `"im"`, or `"mpim"` |
| `is_archived` | boolean |
| `is_member` | boolean |
| `topic` | string or null |
| `purpose` | string or null |
| `unread_count` | integer or null when the read marker is unknown |
| `last_read` | string or null, stored Slack timestamp |
| `latest_ts` | string or null, newest stored message timestamp |
| `synced_at` | RFC 3339 UTC string or null if never synced; from `sync_state.updated_at` |
| `history_complete` | boolean, whether backfill exhausted available history |

`history_complete` does not imply access to history hidden by the Free plan.
`synced_at` records conversation history sync; it is not a per-thread freshness
guarantee. A live event alone does not advance catch-up markers.

## User

| Field | Type |
|---|---|
| `id` | string |
| `name` | string, handle |
| `display_name` | string or null, stored profile display name |
| `real_name` | string or null |
| `is_bot` | boolean |
| `deleted` | boolean |
| `tz` | string or null, IANA timezone when stored |

## References and offline reads

Conversation references accept an id (`C…`, `G…`, `D…`), `#name`, an unprefixed
name, `@person`, or a person's handle/display name/real name/email for a stored
DM. `#` restricts matching to channels; `@` to personal DMs. Matching ranks are:
exact id, exact alias, case-insensitive exact match, case-insensitive prefix.
Only the best rank participates; ties fail with candidates instead of choosing
one. Reads such as `history` use core `Match::AllowPrefix`. `send` and
`sync --channel` use `Match::Exact`: only ranks 0–2 can resolve to a destination.
A prefix-only match returns `ambiguous` (exit 4) with suggestions in `candidates`,
even when there is just one suggestion. Retry with the complete name or id;
no send or filtered sync proceeds from a prefix. Future write commands must
also select `Match::Exact`. Deleted/archived identities are not silently
filtered from resolution. All conversation/DM aliases are loaded with one
joined database query.

Slack permalinks must use HTTPS, a `*.slack.com` host, and the path
`/archives/<id>/p<16 digits>`. An optional `thread_ts` must be a Slack timestamp.
The core message-reference parser returns `conversation_id`, `ts`, `thread_ts`
(or resolves a conversation plus a separate timestamp); existing conversation
commands use the conversation component of a permalink.

`history`, `channels`, `search`, and archive import select the team from
`--team` or the sole team in the DB. They never read the Keychain. If several
teams are stored, pass `--team`; a missing team defaults to `not_found`.

`history <conversation> --fresh [--thread <ts>]` requires a session and catches
up only that conversation before reading SQLite. It reuses the existing
catch-up and stale-thread sync. With `--thread` it explicitly fetches all pages
of that thread, even if the root is not stored. It does not download files or
backfill the whole conversation. A first catch-up on a never-synced conversation
uses the existing fast-pass behavior (one newest page); use `sync` for deep
history. Freshness failures fail the command, rather than presenting stale data
as freshly fetched.

## NDJSON (`sync --follow --json`)

Every stdout line is a complete compact JSON object with `schema_version: 1`
and `type: string`. No pretty-printed multiline records or human log lines are
mixed into the stream. The terminal summary is emitted on graceful shutdown.

| `type` | Other fields |
|---|---|
| `message` | `message`: Message, loaded from SQLite |
| `progress` | `data`: `{stage: "directory"}`; `{stage: "conversations", done: integer, total: integer, current: string, deep: boolean}`; `{stage: "files", done: integer, total: integer}`; or `{stage: "idle"}` |
| `connection` | `data`: `{state: "connected"}` or `{state: "reconnecting", in_secs: integer}` |
| `summary` | `summary`: same object as finite `sync` |
| `message_deleted`, `reactions` | `data`: `{conversation_id: string, ts: string}`; notification to re-read the DB |
| `read_marker`, `history` | `data`: `{conversation_id: string}` |
| `typing` | `data`: `{conversation_id: string, user_id: string}` |
| `directory` | no other fields |

The notification stream is best effort (the existing broadcast bus is bounded);
use SQLite/history as the source of truth after a disconnect. A failure may
terminate a partially emitted stream and is reported on stderr.

## Errors and exit codes

With `--json`, failure writes one object on stderr:

```json
{"schema_version":1,"ok":false,"error":{"code":"ambiguous","message":"ambiguous conversation reference: general (C1, channel), general-help (C2, channel)","candidates":[{"id":"C1","label":"general","kind":"channel"},{"id":"C2","label":"general-help","kind":"channel"}]}}
```

`error.code` and `error.message` are strings. `candidates` is present only for
ambiguous conversation references; each candidate has string `id`, `label`, and
`kind` (the same kind vocabulary as Conversation). Team-selection ambiguity
requires an explicit `--team` and may omit candidates. Credential-shaped text
is redacted from messages, and transport errors use a safe generic description.
Failed finite commands emit no stdout success object (an auth import with some
successful teams may include per-team failures in its one result object).
Unresolved conversation errors include the quoted query and a `flek channels`
hint in both human output and JSON `message`. An empty database without
`--team` reports ``no stored team; run `flek auth import` and `flek sync` ``.
Clap usage errors use the same envelope with code `usage`; `message` preserves
clap's first diagnostic line without ANSI styling (for example,
`error: unexpected argument '--foo' found`). Use `--help` for full usage details.

| Exit | `error.code` | Meaning |
|---|---|---|
| 0 | — | success |
| 1 | `internal` | internal, DB, I/O or other failure |
| 2 | `usage` | invalid arguments or query syntax |
| 3 | `not_found` | unresolved conversation/message or no stored team |
| 4 | `ambiguous` | tied references, prefix-only suggestions in exact mode, or team selection requires disambiguation |
| 5 | `auth` | no session, Keychain/import failure, `invalid_auth`, `token_revoked`, `not_authed` |
| 6 | `rate_limited` | exhausted rate limiting (`429`, `ratelimited`, `rate_limited`); existing API retry behavior still applies |
| 7 | `network` | transport error, HTTP failure, websocket or other Slack API error |
| 8 | `forbidden` | reserved for the M8.7 write guard; no command uses it yet |

## Concurrent database opens

Opening a fully migrated database checks its version without a write lock,
so CLI reads can start while the app holds a WAL write transaction. Only
pending migrations acquire `BEGIN IMMEDIATE`; the version is checked again
inside that transaction before applying SQL, since another process may have
migrated while this process waited. Startup lock waits and pending migrations
still obey the SQLite busy timeout.
