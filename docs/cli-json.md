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
| `send <conversation> [text]` and the other write commands | Guarded writes; see "Agent writes" below for the receipt (`action`, `channel`, `ts`, `permalink`, `message`, ...) and the `--dry-run` report |
| `upload <file> --channel <conversation> [--thread <root-ts>] [--comment <text>]` | `file_id`, `conversation_id`: strings; `message`: Message or null from SQLite; `ts`: string or null; `not_stored_locally`, `completion_unknown`: booleans; guarded, see "Agent writes" |
| `import <path>` | `summary`: `{format: string, read: integer, written: integer, unknown_channels: string[]}` |
| `export --obsidian <vault>` | `summary`: `{directory: string, conversations: integer, messages: integer, links: integer, files: integer, notes_written: integer, notes_unchanged: integer, attachments_copied: integer, attachments_unchanged: integer, attachments_unavailable: integer}`; see [offline Obsidian export](obsidian-export.md) |
| `sync` | `summary`: `{conversations: integer, messages: integer, deleted: integer, failed: integer, files_downloaded: integer}` |
| `auth list` | `teams`: string[] of stored team ids |
| `auth import` | `teams`: objects with `team_id`, `name` strings and `ok` boolean; successful rows have `user`, `user_id` strings (authentication identity labels); failed rows have `error: string` |
| `auth logout` | `removed`: string[] of removed team ids |
| `login --manual` | `saved`: true; no credential or session object |

`whoami`'s authentication label `user_name` and the per-team `auth import` label
are identity metadata; all stored user entities use the User shape below.
No token, cookie, or raw session object is included.

`flek login --manual` is the last-resort browser session entry. With terminal
stdin it prompts twice without echo (token, then `d` cookie); stderr must also
be a terminal. With redirected/piped stdin it reads exactly two lines in that
order, accepting a final line without a newline. Never put credentials in argv,
shell history, environment variables, or files. `--json` requires non-terminal
stdin so prompts cannot mix with the JSON failure envelope. Success returns only
`saved: true`; `--team` checks the validated identity before saving. Input failures
use the existing usage category, rejected credentials the auth category, validation
failures the network category, and credential-store failures the auth category.
See [API notes](api-notes.md#manual-browser-session-entry-m633) for browser DevTools
locations, accepted cookie forms, origin behavior and unverified assumptions.

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
| `viewer_kind` | `image`, `svg`, `video`, `pdf`, `xlsx`, `docx` or null; core in-app viewer policy, separate from OS launch eligibility |

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

A successful `send` always reports Slack's accepted `ts`. If local persistence or the
post-send read fails, `message` is null and `not_stored_locally` is true; stderr
warns that sync will recover the local copy. The command still exits successfully:
retrying an accepted message can duplicate it. Schema version remains 1.

## NDJSON (`sync --follow --json`)

Every stdout line is a complete compact JSON object with `schema_version: 1`
and `type: string`. No pretty-printed multiline records or human log lines are
mixed into the stream. The terminal summary is emitted on graceful shutdown.

| `type` | Other fields |
|---|---|
| `message` | `message`: Message, loaded from SQLite |
| `progress` | `data`: `{stage: "directory"}`; `{stage: "conversations", done: integer, total: integer, current: string, deep: boolean}`; `{stage: "files", done: integer, total: integer}`; or `{stage: "idle"}` |
| `connection` | `data`: `{state: "connected"}`, `{state: "sync_incomplete", in_secs: integer}` (socket alive, catch-up retry pending), or `{state: "reconnecting", in_secs: integer}` |
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
| 8 | `forbidden` | the write guard denied the action: no `--yes` and no matching allow rule in `agent.toml` (or the session belongs to another team). Nothing was sent or logged |

| 8 | `forbidden` | reserved for the M8.7 write guard; no command uses it yet |
| 9 | `timeout` | `flek wait --timeout` elapsed with no reply |

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

## Agent writes (M8.4, M8.7, additive version 1)

Flek writes as the account owner, so every write goes through one guard. Reads
are always allowed; the commands below are the only writers in the CLI.

| Command | Slack method | Action name (policy/log) |
|---|---|---|
| `send <channel> [text]` | `chat.postMessage` | `send` |
| `schedule <channel> [text] --at TIME` | `chat.scheduleMessage` | `schedule` |
| `react` / `unreact <ref> --emoji NAME` | `reactions.add` / `reactions.remove` | `react` / `unreact` |
| `mark-read <ref>` | `conversations.mark` | `mark-read` |
| `pin` / `unpin <ref>` | `pins.add` / `pins.remove` | `pin` / `unpin` |
| `save <ref>` | `stars.add` | `save` |
| `dm <person...>` (1 to 8 people) | `conversations.open` | `dm` |
| `channel create <name> [--private]` | `conversations.create` | `channel-create` |
| `channel invite` / `kick <channel> <person>` (one person) | `conversations.invite` / `conversations.kick` | `channel-invite` / `channel-kick` |
| `channel join` / `leave <channel>` | `conversations.join` / `conversations.leave` | `channel-join` / `channel-leave` |
| `channel topic` / `purpose` / `rename <channel> <text>` | `conversations.setTopic` / `setPurpose` / `rename` | `channel-topic` / `channel-purpose` / `channel-rename` |
| `channel archive <channel>` | `conversations.archive` | `channel-archive` |
| `status set <text>` / `status clear` | `users.profile.set` | `status-set` / `status-clear` |
| `upload <file> --channel REF [--thread TS] [--comment TEXT]` | `files.getUploadURLExternal`, upload host, `files.completeUploadExternal` | `upload` |
| `edit <ref> [text]` | `chat.update` | `edit` |
| `delete <ref>` | `chat.delete` | `delete` |
| `forward <ref> --to DEST [--comment TEXT]` | `chat.postMessage` | `forward` |

Guard targets (what allow rules list under `channels`): message and channel
actions use the resolved conversation id; `dm` the recipient user ids;
`status-set`/`status-clear` your own user id; `channel-create` the marker
`new:<normalized-name>` (the channel has no id yet, so a rule can allow
`new:ops-room` or `*`). The action log stores the same targets, so a created
channel's name appears there. Every channel/status command takes exactly one
target, so there are no mass writes.

`<ref>` is a message permalink, or an exact conversation name/id followed by a
Slack timestamp (`flek react general 1700000000.123456 --emoji tada`). Channels
and people resolve with the exact matcher from the local database only: a
prefix or fuzzy match is reported as `ambiguous`/`not_found`, never guessed.
No command opens a DM implicitly; run `flek dm` first.

`edit` and `delete` touch only your own, stored, non-deleted messages (never a
bot's). Ownership is checked offline while the command is prepared, so a message
of someone else is `forbidden` (8) and an unknown or deleted one `not_found` (3)
before any credentials are read, even with `--yes` or `--dry-run`; core checks
again against the resolved session before HTTP. `edit` takes the replacement
text as `[text]`, `--text`, `--stdin`/`-` or `--file PATH` (same limits and
escaping as `send`, explicit `@name` mentions included), `<ref>` is a permalink or
`CHANNEL TS`. The replacement is plain text: previous rich blocks are removed, and
no signature is added. The receipt carries `message` (re-read from SQLite, `edit`
only). A workspace restriction (edit window, retention or admin policy) maps to
`forbidden` (8) too. `delete` is not undoable; the receipt has `action`, `channel`,
`ts` and `not_stored_locally` (accepted by Slack but the local tombstone could not
be written: sync, never resend). The app's own edit/delete UI is the human path
and is not guarded.

`forward` posts the source message's permalink plus an optional `--comment`
(escaped like `send` text; the agent signature, when configured, is appended)
into one exact existing destination (channel, group or an existing DM). It does
not copy the source's text or files, so access to the original still applies.
The source must be in the local archive and its permalink must be known offline
(cached, or built from the stored workspace URL); otherwise preparation fails
with `usage` and asks for a sync. The policy and the action log see the
destination conversation as the target; the receipt is a `send` receipt with
`action: "forward"`. The desktop app's forward picker is a human action and is
not guarded.

Message input: text as an argument, `-`/`--stdin`, piped stdin (when no text is
given and stdin is not a terminal) or `--file PATH` (UTF-8, at most 1 MiB).
`--blocks` takes a Block Kit JSON array, `@path` to a JSON file, or `-` for stdin
(1 to 50 objects, signature block included). `--thread TS [--broadcast]` replies
in a thread. Text is escaped (`&`, `<`, `>`); only an explicit `@name` at a word
start that resolves to exactly one active user becomes `<@U...>`. Emails, code
spans, `@channel`/`@here` and unknown or deleted users are never converted (the
last group fails as `not_found`), so an agent cannot ping a whole channel.
`schedule --at` accepts Unix seconds or RFC3339 with a timezone, in the future
and within 120 days. `save` adds a legacy Slack star; modern "Later" has no
documented API, so the receipt carries `warning: "legacy_star_only_later_api_unavailable"`
and the item may not show in Slack's "Later" view (not verified on a real
workspace). Idempotent outcomes (`already_reacted`, `no_reaction`, `already_pinned`,
`no_pin`, `already_starred`) count as success.

### Guard order

1. The command is prepared offline from SQLite (resolution, validation, the exact
   request form). Usage and resolution errors happen here (exit 2/3/4).
2. `--dry-run`: print the report and stop. No credentials, network, log entry or
   rate-limit slot is involved.
3. The policy is checked (exit 8 when denied).
4. A rate-limit slot is reserved atomically (exit 6 when exhausted).
5. Only then the session is read from the Keychain and the single request sent.
6. The outcome is stored in the action log. Local state (message, reaction, pin,
   conversation) is updated from the confirmed response and re-read from SQLite.

A denied write consumes no slot and leaves no log entry. A failed attempt keeps
its slot, so a failing loop cannot hammer Slack.

### Configuration: `~/.config/flek/agent.toml`

```toml
writes_per_minute = 10          # 1..60, default 10
signature = "via agent"         # optional, at most 256 bytes; omit to add nothing

[[allow]]                       # zero or more rules
team = "T0FAKE"
channels = ["C0FAKE1", "C0FAKE2"]   # conversation ids; "*" = any channel in this team
actions = ["send", "react"]         # action names from the table; "*" = any action
```

A missing file means defaults: deny every write unless `--yes` is passed. A write
is allowed without `--yes` only when one rule matches its team, every target and
its action (pieces of different rules never combine; `*` is the only wildcard).
For `dm` the targets are the recipient user ids, for `status-*` your own user
id, for `channel-create` the `new:<name>` marker. `--yes` authorizes that single
invocation for any valid action but never bypasses the rate limit or the
descriptor limits (exactly one target, at most eight DM recipients; the target
shape must fit the action). Unknown
keys, an invalid rate or a signature that looks like a credential make the file
invalid (`usage`, exit 2); parser diagnostics never quote the file. The
signature is appended after a blank line to `send`/`schedule` text and, when
`--blocks` is used, as a final context block.

This is a safety net against mistakes by an automated caller, not a security
boundary: a caller that can run `flek` can also pass `--yes`, `--agent-config`
or `--agent-log`. Keep the owner's own config out of the agent's reach if that
matters.

Global flags: `--yes`, `--dry-run`, `--agent-config PATH`, `--agent-log PATH`.

### `--dry-run`

Prints exactly the prepared form and whether the policy would authorize it:

```json
{"schema_version":1,"dry_run":true,"authorized":false,"write":{"action":{"action":"react","team_id":"T0FAKE","targets":["C0FAKE"]},"request":{"method":"reactions.add","params":{"channel":"C0FAKE","name":"tada","timestamp":"1700000000.123456"}},"no_op":false,"warning":null}}
```

`request.params` are the form fields before encoding; credentials are added by
the HTTP client only and never appear. `send` includes a random
`client_msg_id`. `no_op` is true when the write is already satisfied locally
(`mark-read` to an already read position). Without `--json` the same object is
printed pretty-printed (without `schema_version`).

### Receipt (successful write, without `--dry-run`)

`action`, `channel`, `ts`, `permalink`: string or null; `scheduled_message_id`
and `post_at` for `schedule`; `not_stored_locally`: boolean (the write was
accepted by Slack but local state could not be updated: sync before retrying,
never blindly resend); `warning`: string or null (`permalink_unavailable_message_accepted`,
`legacy_star_only_later_api_unavailable`); `message`: Message or null (`send`, `edit`);
`conversation`: Conversation or null (`dm`). Remote acceptance is always success,
even when the permalink lookup or local persistence fails.

### Action log: `flek log [--limit N]`

A separate SQLite file, `agent-actions.db` next to the main database (mode 0600),
also the shared store for the rate limit, so parallel `flek` processes share it.
`log` needs no credentials, creates nothing and prints newest first:
`{"entries":[{"id","started_at","finished_at","team_id","action","targets","outcome"}]}`
(unix seconds; `finished_at` null while pending). `targets` are conversation or
user ids. `outcome` is `pending` (in flight, or the process died: check history
before retrying), `accepted`, or `failed:<exit code>`. The log never stores
message text, blocks, signatures, request bodies, error text, tokens or cookies.
The limit counts entries of the last 60 seconds across all teams and actions.

## Live events for agents (M8.6, additive version 1)

Both commands always write JSON (`--json` is implied). Nothing but the stream or
the result goes to stdout.

### `flek events [--channel REF]... [--mentions] [--dm] [--since TS]`

An NDJSON stream; one object per line, `schema_version` on every line:

```json
{"schema_version":1,"event_id":"1042","event_ts":"1700000100.123000","type":"message_changed","mention":false,"actor_id":null,"reaction":null,"message":{ ...Message view... }}
```

- `type`: `message` (new), `message_changed` (text edited), `message_deleted`,
  `reaction_added`, `reaction_removed`. Reaction events carry `actor_id` and
  `reaction`; `actor_id`/`reaction` are absent on message events. Unknown types
  must be ignored.
- `event_id` is a decimal string: the position in the local event journal, strictly
  increasing, the same for every consumer, and unrelated to Slack timestamps.
- `event_ts`: Slack timestamp of the message for `message`; the local time of
  the change for the other types (Slack's edit/removal time is not stored).
- `message` is the Message view as it was at the time of the event (text, edit and
  delete flags, author, thread, reply count); files, reactions and permalink are
  current. `mention` is true when the text mentions you directly and you are not
  the author.
- Filters combine with AND. `--channel` is repeatable (name, `#name`, id; exact
  match). `--mentions` keeps events on messages that mention you. `--dm` keeps
  personal and group DMs.
- `--since TS` first replays the journal entries whose `event_ts` is after TS, then
  continues live. The boundary is a journal position captured before any network
  work, so nothing is repeated and nothing between replay and live is lost.
  Without `--since` only new events are printed.

The command also runs the sync engine (as `sync --follow` does: incremental
catch-up, websocket, reconnect with backoff), so it writes to the DB and must be
usable while the desktop app runs: both processes write to the same WAL
database. The journal is filled by SQLite triggers on `messages` and `reactions`,
so events written by the app are streamed too, and an unchanged re-write of a
message or reaction (every re-sync does this) produces no event. Live delivery
polls the local journal every 100 ms; it never polls Slack.

Retention: the journal keeps the newest 50000 entries (older ones are deleted by
a trigger on insert), so `--since` can only replay that window, and a consumer that
falls more than 50000 events behind skips the dropped ones. A bulk history
backfill also records its messages (old `event_ts`), so a long-running stream may
see them as `message` events during `sync --full`. Existing messages are seeded
into the journal once, by the migration.

Ctrl-C stops the engine, prints events committed up to that moment and exits 0. A
closed stdout (`| head`) ends the command quietly with exit 0. Errors use the
usual envelope on stderr.

### `flek wait --reply-to <permalink | CHANNEL TS> [--timeout SECONDS]`

Waits for the first message from someone other than you: in the thread of that
message (a channel message or a thread reply; the thread root is found from the
database), or any later message in a DM/group DM. Your own replies, deleted messages
and content-hidden messages do not count. Prints one object and exits 0:

```json
{"schema_version":1,"message":{ ...Message view... }}
```

Order: a matching stored reply is returned at once without credentials or
network; otherwise the engine's websocket is used, and after every reconnect the
thread (or DM) is fetched again, so a reply that arrived while disconnected is
still found. `--timeout` elapsing exits 9 (`timeout`); the default is to wait
forever. Ctrl-C exits 0 without output. An unknown reference is `not_found` (3)
and an invalid timestamp `usage` (2), both before any network work.

## Channel and own-status writes (M8.5, additive version 1)

```text
flek channel create <name> [--private]
flek channel invite <channel> <person>
flek channel kick <channel> <person>
flek channel join <channel>
flek channel leave <channel>
flek channel topic <channel> <text>
flek channel purpose <channel> <text>
flek channel rename <channel> <name>
flek channel archive <channel>
flek status set <text> [--emoji :name:] [--expires TIME]
flek status clear
```

These commands are listed in the guarded-writes table above (action names and
targets) and accept the shared `--dry-run`, `--yes` and `--json` flags. `channel info` retains its
existing read behavior. Invite/kick accept exactly one person per invocation.
An empty quoted topic/purpose clears the value. Create/rename share the M6.11
core normalization and validation: lowercase, whitespace to hyphens, 1–80
letters, ASCII digits, hyphens or underscores. Rename stores Slack's returned
name. Create has no optional purpose write; use `channel purpose` separately.

Team selection uses `--team` or the sole identity in SQLite. Channel/person
references use shared `Match::Exact` resolution **before** credentials are
accessed. Prefix-only matches, even a unique prefix, fail as `ambiguous` (4)
with `error.candidates`; unresolved targets fail as `not_found` (3). There is
no implicit directory refresh. Sync first when an identity is absent. Personal
and group DMs are rejected by these channel operations. Known general channels
cannot be left/archived; self-kick directs callers to `channel leave`.

Status targets only the stored self identity. Text is at most 100 Unicode code
points. Emoji is an empty string (default) or a Slack shortcode such as
`:calendar:`. `--expires` (alias `--expiration`) defaults to `0` (no expiry).
It accepts `0`/`never`, future integer Unix seconds, RFC3339, local calendar
date or an integer duration `30m`/`12h`/`7d` **from now**. Unlike read TIME
arguments, these durations point forward. Absolute fractional timestamps are
truncated to whole seconds. Past/out-of-range/overflow expirations are usage
errors (2). Clear sends empty text and emoji with expiration zero.

These commands are guarded like every other write (see "Agent writes" above:
deny unless `--yes` or an `agent.toml` allow rule, rate limit, action log, session
resolved only afterwards). `--dry-run`, `--yes`, `--agent-config` and `--agent-log`
are the shared global flags. Dry-run prints the same report as the M8.4 writes
(`schema_version`, `dry_run`, `authorized`, `write`); it never resolves a session,
sends a request or touches the log, and does not apply the action to SQLite
(normal DB opening/migrations still apply):

```json
{"schema_version":1,"dry_run":true,"authorized":false,"write":{"action":{"action":"channel-invite","team_id":"T1","targets":["C1"]},"request":{"method":"conversations.invite","params":{"channel":"C1","users":"U2"}},"no_op":false,"warning":null}}
```

`create` targets `new:<normalized-name>`; `status-*` target your own user id.
`params` are string form fields: the status `profile` is the JSON string that is
sent. The prepared plan is immutable; session team and user must match before
execution (`forbidden`, 8, otherwise).

Successful execution returns exactly one object with `schema_version:1`,
`ok:true`, `dry_run:false`, `action` (the action name string), `targets`
(the guard targets), nullable `conversation_id` and `user_id`, `conversation:Conversation|null`, `user:User|null`,
`warning:string|null`, and `not_stored_locally:boolean`. Channel commands return
the conversation view; status returns the self user view. All views are read
from SQLite after confirmation. For create, the resulting id is in the top-level
`conversation_id` (the guard target was the `new:` marker). Invite/kick identify
their target person in `user_id` but return no user view.

Join reuses the core history catch-up. Failure of that secondary read returns
`warning:"catch_up_failed"` and exit 0, with confirmed membership retained.
If persistence or post-write hydration fails after remote acceptance, the
affected views are null, `warning:"not_stored_locally"`,
`not_stored_locally:true`, and exit 0. **Do not retry an accepted mutation**;
sync to recover the local view. API rejection leaves local state unchanged and
uses the standard typed failure envelope. Missing channels/users map to exit 3;
permission, membership and archived-channel restrictions to 8; invalid names
and arguments to 2; auth/rate-limit/network retain 5/6/7.
This contextual API error classification applies only to these management
actions; existing callers retain the legacy `Error::Slack` exit contract.

Membership deltas are transactional/idempotent, retain history, and do not mark
an incomplete member list as complete. A known complete snapshot supplies the
updated count; otherwise a stale directory count is removed. Topic, purpose,
name, archived flag and own membership update both indexed columns and stored
raw metadata. Status merges the returned profile using the same core path as
M6.7. Confirmed writes emit directory invalidation; join also emits history
invalidation. No channel write advances history sync markers itself.

## Upload (M8.4.4, guarded)

`flek upload <file> --channel <exact-reference> [--thread <root-ts>] [--comment <text>]`
is a write like `send` and follows "Agent writes" above: action `upload`, the
policy target is the destination channel (a thread reply is still one target),
`--dry-run`, `--yes`, the shared rate limit and the action log all apply. The
channel resolves with the exact matcher from the local database only (no network
directory refresh). The file is only `stat`ed while preparing: name, size (1 byte
to 100 MiB), regular file. Its bytes are copied into a private snapshot after the
policy and rate-limit checks and before the session is read from the Keychain.
`agent.toml`'s `signature`, when set, is appended to the comment (or is the
comment); the comment is escaped like message text.

`--dry-run` prints the prepared form and whether the policy would authorize it,
never file bytes or the local path:

```json
{"schema_version":1,"dry_run":true,"authorized":false,"write":{"action":{"action":"upload","team_id":"T0FAKE","targets":["C0FAKE"]},"upload":{"filename":"notes.txt","size":27,"channel":"C0FAKE","thread_ts":null,"comment":""}}}
```

Receipt (one schema-versioned object; `message` always comes from SQLite; `ts`
can be null even after remote acceptance because external completion supplies
no authoritative message timestamp):

```json
{"schema_version":1,"action":"upload","channel":"CEXAMPLE","conversation_id":"CEXAMPLE","file_id":"FEXAMPLE","ts":null,"permalink":null,"not_stored_locally":true,"completion_unknown":false,"warning":null,"message":null}
```

`not_stored_locally: true` means normal sync must recover the shared message.
`completion_unknown: true` means the final response was ambiguous and no matching
stored message was found; this is an uncertain remote write, not permission to
retry as a new upload: the receipt carries
`warning: "completion_unconfirmed_check_history_before_retrying"` and the action
log entry stays `pending` instead of `accepted`. Check the conversation before
uploading again. Both fields may be true; a confirmed/locally hydrated upload sets
both false. This partial result exits successfully to preserve the file id.
Definitive Slack refusals use the existing typed failure envelope/exits and a
`failed:<exit code>` log entry. Ctrl+C cancels ticket/byte work; after completion
starts it waits for the result because the file may already have been shared.
Progress goes only to stderr in human mode. The upload host receives only the
file bytes: never the token, the cookie or any `Authorization` header, and
redirects from it are not followed. See api-notes.md for the transport and limits.
