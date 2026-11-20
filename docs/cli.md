# Using `flek` from scripts and AI agents

Recipes for the common jobs. Field-level details are in the
[JSON contract](cli-json.md); the README lists every command for people.
Examples use invented ids (`C0FAKE`, `U0FAKE`, `1700000000.123456`).

## Ground rules

- Always pass `--json`. Success is one object on stdout (`events` is NDJSON);
  failure is one object on stderr and a typed exit code. Branch on the exit
  code, not on message text.
- Keep Slack timestamps (`ts`) as strings. `1700000000.100000` and
  `1700000000.1` are different messages.
- Reads are local and need no session: they show what the app or
  `flek sync` has stored. Add `--fresh` to a read when it must be current;
  that reads the stored session and calls Slack.
- Writes act as the account owner. Every write is guarded: it needs `--yes`
  or an allow rule in `~/.config/flek/agent.toml`, is rate limited, and is
  logged without its text (`flek log`). Preview with `--dry-run` first.
- Never guess a destination. An ambiguous name fails with exit 4 and a
  `candidates` list; pick an `id` from it and retry.

## Discover the command line

```bash
flek schema --json | jq '.commands[] | {path, guarded_action, needs_session}'
flek schema channel --json            # one namespace
flek schema --json | jq '.exit_codes'
```

`flek schema` is built from the parser, so it always matches the binary.

## Exit codes

| Code | Kind | What to do |
|---|---|---|
| 0 | ok | |
| 1 | internal | Report; do not retry in a loop. |
| 2 | usage | Fix the arguments (see `flek schema <command>`). |
| 3 | not_found | Check the name with `flek channels` / `flek users --query`. |
| 4 | ambiguous | Choose an `id` from `error.candidates`. |
| 5 | auth | Ask the owner to open Flek or run `flek auth import`. |
| 6 | rate_limited | Wait a minute; writes are capped per minute. |
| 7 | network | Retry later; local reads still work. |
| 8 | forbidden | The write guard said no. Ask the owner; do not add `--yes` on your own. |
| 9 | timeout | `flek wait --timeout` elapsed. |

## Scenarios

### Catch up on what is unread

```bash
flek unread --messages 5 --json \
  | jq '.conversations[] | {id, label, unread_count, mentions, messages: [.messages[].text_resolved]}'
```

DMs and conversations that mention you come first. Add `--fresh` if Flek has
not been running. Reading does not mark anything as read; that is a separate,
guarded write:

```bash
flek mark-read C0FAKE 1700000000.123456 --yes --json
```

### Read a conversation or a thread

```bash
flek history '#project-demo' --limit 50 --json | jq -r '.messages[] | "\(.user): \(.text_resolved)"'
flek history '#project-demo' --since 2026-11-01 --until 2026-11-08 --json
flek thread https://fake.slack.com/archives/C0FAKE/p1700000000123456 --json
flek message C0FAKE 1700000000.123456 --context 3 --json
```

Messages come oldest first. For the next older page pass `next_cursor`
verbatim as `--cursor`, keeping the same `--since`/`--until`.

### Find something

```bash
flek search '"release notes" from:alex in:#project-demo after:2026-10-01' --json \
  | jq '.results[] | {conversation, ts, permalink, snippet}'
flek search 'invoice' --recent --context 2 --json
flek mentions --since 7d --json
flek files --type pdf --channel '#project-demo' --json
flek file get F0FAKE --to ./invoice.pdf --json
```

Local search includes history Slack no longer shows on the free plan.
`--remote` also asks Slack's search and stores what it finds before answering.

### Reply in a thread

```bash
# 1. Find the root: a reply permalink resolves to its thread root.
root=$(flek thread "$PERMALINK" --json | jq -r '.root_ts')

# 2. Preview the exact request, then send.
printf 'Done, see the updated sheet.' | flek send C0FAKE --thread "$root" --dry-run --json
printf 'Done, see the updated sheet.' | flek send C0FAKE --thread "$root" --yes --json \
  | jq '{ts, permalink, not_stored_locally}'
```

Text is Slack mrkdwn. `@handle` mentions in the text are resolved to user ids.
Read multi-line text from stdin or `--file`, never by splicing it into the
command line. If the receipt says `not_stored_locally: true`, Slack accepted
the message: sync before doing anything else, do not resend.

### Ask a person and wait for the answer

```bash
receipt=$(flek send @alex 'Can you review the draft today?' --yes --json)
flek wait --reply-to "$(jq -r .channel <<<"$receipt")" "$(jq -r .ts <<<"$receipt")" --timeout 3600
```

`send @alex` needs an existing DM; open one with `flek dm @alex --yes --json`.
`wait` prints the first message from someone else in that thread (or, in a DM,
any later message). Exit 9 means nobody answered in time.

### Find a message and forward it

```bash
link=$(flek search 'quarterly plan in:#project-demo' --limit 1 --json | jq -r '.results[0].permalink')
flek forward "$link" --to '#team-leads' --comment 'FYI for Monday' --dry-run --json
flek forward "$link" --to '#team-leads' --comment 'FYI for Monday' --yes --json
```

### React, edit, delete

```bash
flek react "$link" --emoji white_check_mark --yes --json
flek edit C0FAKE 1700000000.123456 --text 'Corrected text' --yes --json   # your own messages only
flek delete C0FAKE 1700000000.123456 --yes --json                  # not undoable
```

### Follow live activity

```bash
flek events --mentions --json | while read -r line; do
  jq -r 'select(.type == "message") | .message.permalink' <<<"$line"
done
```

`events` runs the sync engine itself and can run next to the Flek app. Resume
after a restart with `--since <last event_ts>`.

## Allowing writes without `--yes`

Let an agent post and react in one channel only:

```toml
# ~/.config/flek/agent.toml
writes_per_minute = 6
signature = "via agent"

[[allow]]
team = "T0FAKE"
channels = ["C0FAKE"]
actions = ["send", "react"]
```

Action names are the `guarded_action` values from `flek schema`. A missing file
denies every write without `--yes`. The guard protects against mistakes, not
against a caller determined to bypass it.
