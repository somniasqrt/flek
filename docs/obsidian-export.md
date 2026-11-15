# Offline Obsidian export (M6.5.1)

```sh
flek export --obsidian ~/Notes
flek --db ./archive.db --team T_EXAMPLE export --obsidian ./vault --json
flek export --obsidian ~/Notes --channel '#project-demo' \
  --since 2026-01-01 --until 2026-02-01
```

The command opens an **existing** SQLite database read-only, including committed
WAL data. It never creates/migrates the database, resolves credentials, cleans up
desktop session copies, downloads files or contacts Slack. Use the normal Flek
database command separately if an older archive needs migration. Team selection
uses `--team` or the only stored account team, like other offline commands.

## Layout and references

Generated files are confined to `Flek/team-<digest>/` inside the chosen vault:

```text
index.md
links-and-files.md
conversations/conversation-<digest>.md
attachments/<file-id-digest>-<content-digest>/<portable-name>
ranges/<bounds-digest>/...same layout for a date projection...
```

Digests are deterministic SHA-1 identifiers, not security signatures. Team and
conversation identity determines the path; renaming a channel cannot orphan its
note. The index provides readable names, kinds are recorded in each note, and all
stored conversations are included (public/private channels, DMs, group DMs,
archived and non-member conversations). An empty conversation still gets a note.
Deleted messages are omitted; ordinary replies are included once in chronological
order, quoted and linked to their root. Missing/deleted/out-of-range roots are
explicitly labeled rather than linked to nonexistent blocks. Reported reply
counts can exceed the locally stored replies.

Notes group messages by **UTC day**, with UTC time, author, edited/hidden markers
and named reactions with counts and participants. People and channel references
use stored display names, falling back to names, supplied labels or IDs. Channel
mentions link to other selected notes. Known Slack message permalinks link to
exported message blocks; unavailable destinations retain the original URL.
Standard/custom emoji remain readable `:name:` shortcodes; stored custom aliases
are followed with bounded cycle detection. Emoji images are not embedded or
fetched. Slack formatting is best effort, with plain-text Block Kit fallback
when stored text is empty, rather than a lossless rich-block conversion.

The catalog has a links table (one deduplicated URL per message, including stored
Block Kit/unfurl content) and a files table (one row per file share). Rows link
back to their message. The file table includes the canonical local cache path,
vault copy, stored file permalink and availability/hidden status. Raw payloads,
private download URLs and session metadata are never dumped. Pasted Slack
credential-shaped strings in rendered text are redacted.

## Attachments and filesystem behavior

Attachments are **copied**, never symlinked. Only canonical regular files inside
the trusted Flek cache root qualify. A stale/missing/outside-cache recorded path
produces an unavailable row and no download. Copies use the existing quarantine
policy, are staged privately and published atomically. A content hash detects
same-size edits; changed content gets a different directory. Copying fails if
the source changes during the operation, so a retry cannot silently label the
wrong content. A vault copy works after the original cache is removed; a later
export with no cached source reports it unavailable (old copies are retained).

Attachment basenames reuse `files::download_name`: portable Windows restrictions,
reserved-device handling, bidi removal and bounded UTF-8 length on every OS.
Additional `#`, `^`, `[`, `]` and `|` characters become underscores for Obsidian
links. Identity and content directories avoid collisions between same-name files.
Generated directories and file targets refuse existing symlinks/non-regular
entries. The supplied vault root may have platform directory aliases in its
ancestors; the vault itself must not be a symlink. As with other local exporters,
the output directory must not be concurrently replaced by another process.

## Filters and incremental runs

`--channel` resolves an exact, unambiguous stored identity (including DM aliases).
Only that conversation is updated; other notes/copies are left alone. A selected
conversation gets its own `index-<id-digest>.md` and
`links-and-files-<id-digest>.md`, preserving the whole-archive catalogs.

`--since` is inclusive, `--until` exclusive. Both use `core::time` and accept Slack
timestamps, RFC3339, local-calendar `YYYY-MM-DD`, or lookback durations (`30m`,
`12h`, `7d`). Both relative bounds share one captured current time. Day headings
remain UTC even when a calendar-date argument represents local midnight.
Reversed/equal bounds are usage errors. Normalized bounds determine a separate
`ranges/` directory, preventing a projection from truncating the full archive.
Relative filters naturally select a new projection each run; repeat absolute
bounds to update the same one.

Each note is compared byte-for-byte and replaced atomically only when different;
unchanged notes and attachment copies keep their modification times. Names do not
depend on channel labels or filesystem enumeration order. Export reads one SQLite
snapshot, hydrates 500 messages per query batch and renders one conversation at a
time (memory grows with the largest conversation and catalog). It does not prune
old notes/copies or merge user edits: generated notes should be treated as output.
Individual writes are atomic; a failure can leave an already completed subset.
The index and catalogs are published after conversation notes.

`--json` emits the standard schema-version-1 envelope with `summary`: output
directory; conversation/message/link/unique-file counts; note written/unchanged
counts (including index/catalog); unique-attachment copied/unchanged/unavailable
counts. Errors use the existing CLI error envelope and exit codes.

## Tests

`crates/core/tests/export.rs` exports a temporary SQLite archive (thread, reaction,
file, link, deleted and hidden messages): a golden Markdown note, byte/mtime
stability of re-exports, rewriting only the changed conversation, `--channel` and
`--since/--until` projections, files outside the cache never copied, and read-only
opening. `export::render` has unit tests for escaping, link extraction and
redaction. `crates/cli/tests/contract.rs` covers the `--json` summary and typed
errors without credentials.

Owner check (native Obsidian rendering and Windows filesystems are not covered):
open an exported vault and follow index, channel, reply, message-block and
attachment links; re-export and confirm unchanged modification times.
