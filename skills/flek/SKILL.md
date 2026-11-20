---
name: flek
description: Read, search and write Slack through the owner's local `flek` CLI (Flek archive) instead of a Slack connector. Use when the user asks about their Slack messages, unread items, mentions, threads, files or people, or asks to reply, react, forward or post in Slack.
---

# Slack through `flek`

`flek` is the owner's local Slack client. It keeps the whole workspace in a
local SQLite archive (including history the free plan hides) and writes to
Slack as the owner, not as a bot. Everything you send appears under their name.

Full recipes: `docs/cli.md` in the Flek repository. Field shapes:
`docs/cli-json.md`. Live description of every command, argument and exit code:

```bash
flek schema --json
```

## Rules

1. **Always pass `--json`.** Parse stdout; on failure read the JSON object on
   stderr and branch on the exit code (2 usage, 3 not found, 4 ambiguous,
   5 auth, 6 rate limited, 7 network, 8 forbidden, 9 timeout).
2. **Reads are free; writes need the owner's consent.** Read commands touch only
   the local archive. Before any write (`guarded_action` is not null in
   `flek schema`), show the owner what will be sent and where, and get a yes in
   this conversation. Then run it once with `--dry-run`, check the prepared
   request, and only then repeat with `--yes`.
3. **Never pass `--yes` to get around exit 8.** Forbidden means the owner's
   policy said no. Report it.
4. **No mass actions.** One message, one reaction, one destination per
   command. Do not loop writes over channels or people.
5. **Never guess a destination.** On exit 4 show the `candidates` to the owner
   or pick by `id` only when the intent is unambiguous. Writes accept only exact
   names or ids.
6. **Keep `ts` values as strings** and pass permalinks or `CHANNEL TS` pairs
   exactly as `flek` returned them.
7. **Don't resend after an unclear result.** If a write receipt has
   `not_stored_locally: true`, or `flek log` shows `pending`, Slack may already
   have the message: check with `flek history ... --fresh` first.
8. **Don't dump private messages into other places** (files, other chats,
   commits) unless the owner asks for that specific content.
9. Never read, print or ask for tokens or cookies. Session problems (exit 5):
   ask the owner to open Flek or run `flek auth import` themselves.

## Quick reference

| Task | Command |
|---|---|
| What needs attention | `flek unread --messages 5 --json` |
| Mentions of the owner | `flek mentions --since 7d --json` |
| Recent messages | `flek history '#channel' --limit 50 --json` |
| A thread | `flek thread <permalink> --json` |
| Search | `flek search 'words from:alex in:#channel after:2026-01-01' --json` |
| People | `flek users --query alex --json`, `flek user @alex --json` |
| Files | `flek files --type pdf --json`, `flek file get F… --to ./x.pdf --json` |
| Make a read current | add `--fresh` (calls Slack with the stored session) |
| Reply in a thread | `printf '%s' "$text" \| flek send C… --thread <root ts> --dry-run --json` |
| React | `flek react <permalink> --emoji white_check_mark --dry-run --json` |
| Forward | `flek forward <permalink> --to '#channel' --comment '…' --dry-run --json` |
| Wait for an answer | `flek wait --reply-to <channel> <ts> --timeout 3600` |
| Live stream | `flek events --mentions` (NDJSON, runs until stopped) |
| What was written | `flek log --json` |

Pass message text through stdin or `--file`, not by splicing it into the shell
command. Text is Slack mrkdwn; `@handle` becomes a real mention.

Results reflect the archive as last synced. If the archive looks stale (old
`synced_at`, missing recent messages), use `--fresh` on that read rather than
`flek sync`, which is a long full pass.
