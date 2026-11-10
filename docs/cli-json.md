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
| `can_open` | boolean, core extension/MIME launch policy; OS opening rechecks the resolved cache path |

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
| `presence` | `active`, `away`, `dnd`, or `unknown`; derived from stored presence/DND only |
| `dnd_until` | integer Unix seconds or null when no current, fresh DND window is known |

Presence adds no live API calls to CLI reads. Unknown/stale state is `unknown`
(grey in the UI). Self manual-away wins; otherwise a current DND/snooze window
wins over active. `dnd_until` can still describe a known pause while manual-away
makes the indicator grey. The app invalidates presence on startup and websocket
disconnect/shutdown. These fields are additive in JSON v1 on all User views,
including `users`, `user`, `whoami.user`, and stored channel members.

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

The additive `presence` change frame has no `data` payload. It signals a stored
presence/DND change or freshness invalidation; it carries no user profile or
message content. Consumers re-read their selected users from SQLite.

## Errors and exit codes

With `--json`, failure writes one object on stderr:

```json
{"schema_version":1,"ok":false,"error":{"code":"ambiguous","message":"ambiguous conversation reference: general (C1, channel), general-help (C2, channel)","candidates":[{"id":"C1","label":"general","kind":"channel"},{"id":"C2","label":"general-help","kind":"channel"}]}}
```

`error.code` and `error.message` are strings. `candidates` is present only for
ambiguous conversation or user references; each candidate has string `id`, `label`, and
`kind` (the same kind vocabulary as Conversation, plus `"user"`). Team-selection ambiguity
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

## Agent reads (M8.2, additive version 1)

All commands below select `--team` or the sole stored team and read SQLite
without Keychain. Only explicit `--fresh` or an uncached `file get` resolves a
session. Fresh failures fail the command. Network responses are stored before
views are read; no raw API object is emitted.

| Command | Payload beside `schema_version: 1` |
|---|---|
| `unread [--messages N] [--include-unknown] [--fresh]` | `conversations`: Conversation[] extended per item with `mentions: integer`, `first_unread_ts: string or null`, `messages: Message[]`; `unknown`: Conversation[] (empty unless requested); `failed`: integer (failed fresh conversation catch-ups, 0 for offline reads) |
| `history <conversation> [--since TIME] [--until TIME] [--around TS_OR_PERMALINK] [--cursor CURSOR]` | Existing `conversation`, `messages`; adds `next_cursor: string or null` (older), `prev_cursor: string or null` (newer). With `--around`, the target Message has `target: true`; other messages omit this field. |
| `thread <permalink>` or `thread <conversation> <ts> [--fresh]` | `conversation`: Conversation; `root_ts: string`; `messages: Message[]` (root and all stored replies, oldest first) |
| `message <permalink>` or `message <conversation> <ts> [--context N] [--fresh]` | `message`: Message; `before`, `after`: Message[] (each oldest first); `conversation`: Conversation |
| `users [--query TEXT] [--include-bots] [--include-deleted] [--limit N]` | `users`: User[]; defaults exclude bots/deleted users, limit 100; query is a case-insensitive substring of handle/display name/real name/email |
| `user <ref>` | `user`: User (profile additions below) |
| `channel info <ref> [--fresh]` | `conversation`: Conversation; `created: integer or null` (Unix seconds), `creator: string or null` (user id), `num_members: integer or null`; `members: User[] or null`; `pins: Message[] or null`; `members_synced_at: RFC3339 UTC string or null` |
| `mentions [--since TIME] [--until TIME] [--include-broadcasts] [--limit N]` | `messages`: Message[], newest first, including the usual `conversation` display label; offline only; defaults since `7d`, limit 50 |
| `files [--channel REF] [--from USER] [--type TYPE] [--since TIME] [--until TIME] [--limit N]` | `files`: File[] with provenance additions below, newest message first; default limit 50; one item per message/file share |
| `file get <id> [--to PATH]` | `file`: File with provenance; `path: string` (destination after `--to`, otherwise cache path); human mode prints this path alone |

Unread conversations use stored counters and the same visibility rules as the
counter triggers: undeleted top-level messages/broadcast replies from others
after `last_read`. Ordering is DMs/group DMs, then conversations with unread
`<@SELF_ID>` mentions, then others; within each group newest stored activity
first, then id. `--messages N` attaches the oldest N unread messages (default 0).
Unknown markers are separate and never guessed. `--fresh` refreshes the directory
and `client.counts`, then uses existing catch-up/thread helpers for conversations
with changed or unknown remote activity (or no stored newest timestamp).
Unread messages alone do not trigger catch-up when the stored history already
reaches `remote_latest`. Authentication errors abort immediately. Other
conversation failures are logged and counted in `failed`; remaining conversations
are still attempted, and stored data remains available. Human output includes
a warning on stderr when `failed > 0`. Directory refresh failures still abort.

History remains oldest-first within a page; `--limit` defaults to 30 and now
also limits `--thread` pages. Bounds are inclusive. Cursors are opaque and bound
to the conversation, thread, and normalized time bounds; reuse those options
when following a cursor. Relative bounds are evaluated anew, so use absolute
bounds for stable pagination. Garbage/mismatched cursors produce `usage` (2).
Cursors are null at the ends, including a zero-limit page. `--around` conflicts
with `--cursor`; it reserves half the limit for older messages and fills the
remainder with the target/newer messages, filling unused space at the edges.
An unstored/filtered target yields surrounding stored messages without a target
flag. A permalink must belong to the selected conversation.

Thread references use the stored `thread_ts` to resolve a reply to its root.
`thread --fresh` fetches that root's replies directly. `message --fresh` first
catches up the conversation and stale threads, then fetches the specific thread
when the message is a reply. Message context uses the thread timeline for
replies and the channel timeline for top-level messages. Missing messages or
threads fail with `not_found` (3), including the reference.

User references accept `U…`/`W…` ids, `@handle`, handle, display name, real name
or stored email. Ranking and prefix policy match conversation resolution;
ties return `ambiguous` (4), with candidates having `kind: "user"`.
Deleted users/bots remain resolvable even when excluded from `users` listings.

The shared User view gains these nullable fields on every command returning it:

| Field | Type / source |
|---|---|
| `title`, `status_text`, `status_emoji`, `email` | string or null, stored `raw_json.profile` |
| `status_expiration` | integer or null, Unix seconds from profile (0 means no expiration) |
| `tz_label` | string or null, stored `raw_json.tz_label` |
| `is_admin`, `is_owner` | boolean or null when absent from stored raw user |
| `dm_conversation_id` | string or null, stored personal DM in the user's team |

`channel info --fresh` fetches all pages of `conversations.members` and
`pins.list`; replaces the stored sets; preserves pinned message payloads using
the usual non-lossy message merge. Messages, attachments/reactions, both sets,
and the details sync marker are committed in one transaction. Only message pins
are represented. A stored member missing from the directory uses its id for `id`/`name`, false for
`is_bot`/`deleted`, and null for unknown profile fields. Before the first
successful details fetch, `members`, `pins`, and `members_synced_at` are null.
After an empty successful fetch they are `[]`, `[]`, and the fetch time.
`created`/`creator`/`num_members` reflect existing stored conversation metadata;
the details fetch does not imply a conversation metadata or history refresh.

Mentions exclude deleted messages and the signed-in user's own messages, but
include replies. Exact `<@SELF_ID>` matches are required; broadcasts optionally
add `<!here>`, `<!channel>`, `<!everyone>`. FTS finds candidates and the exact
mrkdwn is checked before limiting. No local self id means `not_found`.

File provenance additions (on `files` and `file get`): `conversation_id`, `ts`,
`user_id`, `permalink` are nullable strings; `permalink` points to the sharing
message. `hidden_by_limit` is boolean. Existing File fields remain unchanged;
attachments in Message retain their existing compact File shape. Files can
appear once per stored share; `file get` chooses the newest share in the team.
`local_path` is null when no cache path is recorded. `file get` checks the file
still exists, downloads through core into the standard cache only when needed,
and stores the cache path. `--to` copies to the destination; `file.local_path`
continues to identify the cache. DB paths outside the canonical cache are refused;
on macOS new downloads and saved copies receive quarantine protection. Names have
bidi formatting controls removed for display. Hidden files fail with `not_found` and an
explanation; files without a downloadable stored URL fail before session access.
Type groups are `image`, `pdf`, `video`, `audio`, `doc` (text/office documents),
`other`; MIME prefixes such as `image/` or `application/vnd.` also work.

Every TIME argument shares a core parser: Slack seconds with six fractional
digits, a date `YYYY-MM-DD` at local midnight, RFC3339 (with timezone), or a
non-negative integer duration ending in `m`, `h`, `d`, relative to now.
Invalid/out-of-range times or reversed bounds fail as `usage` (2). Date midnights
that are ambiguous/nonexistent in the local timezone also fail as `usage`.

## Agent search (M8.3, additive version 1)

`search <query> [--since TIME] [--until TIME] [--context N] [--limit N]
[--offset N | --cursor CURSOR] [--recent] [--remote]` adds these envelope fields:

| Field | Meaning |
|---|---|
| `query` | Parsed interpretation described below |
| `next_cursor` | Opaque cursor for the next stored result page, or null |
| `remote` | `{searched: boolean, total: integer, pages: integer, error: null or {code: string, message: string}}` |
| `remote_unsupported_filters` | Strings identifying syntax that cannot be represented exactly in the remote query; also available as a translation preview on offline reads |

`results` retains every Message field and `snippet` (`**` match markers).
Each result additionally has `before: Message[]`, `after: Message[]`, and
`source: "local" | "remote" | "both"`. Context defaults to zero; arrays are
oldest first and contain up to N neighbours from the same timeline. Replies
use their thread, including the root; top-level messages use the conversation
without ordinary replies. Deleted neighbours are excluded. Context does not
inherit the search text/date/author filters.

`query` has `fts: string` (the actual SQLite MATCH expression), `terms: string[]`
(original free-text tokens including quotes, `prefix*` and `-excluded`),
`from: {id, label}[]`, `in_conversations: {id, label}[]`, `has_link: boolean`,
`has_file: boolean`, `before`, `after`, `since`, `until` (original strings or
null), and `since_ts`, `until_ts` (effective normalized bounds or null).
The original parser's `before:` day starts at UTC midnight (exclusive upper
bound); `after:` starts at UTC midnight the following day (inclusive lower
bound). Invalid calendar dates fail as usage. `--since`/`--until` use the shared
TIME parser, including local-midnight dates, Slack timestamps, RFC3339 and
relative durations. For search these bounds are lower-inclusive and
upper-exclusive, just like the parser's normalized bounds. Flags refine query
bounds: the later lower bound and earlier upper bound win. Reversed bounds
are usage errors. Repeated from/in filters retain the parser's OR semantics.
Unresolved references fail with `not_found`, and tied prefixes with `ambiguous`,
using the M8.1 resolvers before any credentials are accessed.

Limit defaults to 20, offset to 0. Cursor and offset conflict. Cursors are bound
to team, normalized query and sort; garbage/mismatched cursors fail as usage.
Absolute times are needed for stable cursor reuse; relative times are evaluated
anew. Ties are broken by timestamp descending, then conversation id. A zero
limit yields no results and a null cursor. Paging reflects the current database,
not a retained snapshot; new writes or a repeated remote search can change ranks.
A null cursor means no more **stored** matches, not that Slack search was exhausted.

Without `--remote`, search never accesses Keychain or the network. With it,
`search.messages` candidates are stored first, then SQLite runs the same query
and hydrates all views. Deduplication uses `(conversation_id, ts)`. Source is
per invocation: a returned remote key already stored before ingestion is `both`,
a new remote key is `remote`, and other stored matches are `local`. Remote-only
messages that do not satisfy the local query after ingestion are filtered out.
All merged matches use local BM25 relevance (default) or timestamp (`--recent`),
so Slack scores never compete with local BM25 scores.

Remote fetching requests at most `offset + limit + 5` candidates (rounded up
by at most two for fixed-size numbered pages), capped at 300 and three pages.
Page size is at most 100; continuation requests wait at least three seconds,
and the shared client's Retry-After handling remains active. `remote.total` is
Slack's reported total, `pages` counts successfully ingested pages, and
`searched` means a remote request was attempted (false for limit zero).
Non-auth remote failures return stored results, `remote.error`, and exit 0;
human output warns on stderr. Auth failures retain exit 5. JSON warnings are
carried only in `remote.error`. A failure after an earlier successful page
retains that page's stored evidence. Database failures still fail the command.

Remote translation retains phrases, exclusions, from/in ids (`from:<@UFAKE>`,
`in:<#CFAKE>`), has filters and query dates. Explicit prefixes are omitted and
listed verbatim in `remote_unsupported_filters`; bare words are sent but their
local stemming differences are listed as `local_stemming:<word>`. Multiple
from/in filters are omitted remotely and listed as `from:multiple`/`in:multiple`
to avoid changing the local OR into remote AND. Exact TIME flag bounds are
omitted and listed as `--since`/`--until`; SQLite still enforces them precisely.
These omissions can reduce recall within the bounded remote candidate budget.

Search payloads can omit full history fields. Ingestion therefore uses the
existing gap-filling message merge: complete local messages, attached files,
reactions and metadata are retained; new rows and content-stripped gaps are
filled. Normal history sync remains responsible for updating complete messages.
Search never advances history/thread sync markers. Unknown users are stored
when a profile/username is present; otherwise views retain their ids.
Unknown conversations are stored with `is_member = 0`; existing membership is
preserved. They remain searchable/resolvable but are excluded from the app
sidebar, default `channels`, unread listings and the engine's conversation walks.
`channels --include-non-member` explicitly includes them (combine with
`--archived` to also include archived records).
