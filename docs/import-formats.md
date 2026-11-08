# Import formats

`flek import <path>` (`crates/core/src/sync/import.rs`) loads existing
archives into the local database so no history is lost.

Rules for every format:

- Run `flek sync` first: messages are matched to conversations and authors
  that the database already knows.
- Archived messages **only fill gaps**: a message is written when it is new,
  or when the stored copy is one Slack already hides (`hidden_by_limit`).
  Anything Slack still serves is left as is.
- Messages are keyed by `(conversation id, ts)`, so importing the same
  archive twice changes nothing. Keep the original `ts`: without it a message
  cannot be matched to the same message coming from the API.

The format is detected automatically: a directory that contains
`channels.json` or `users.json` is a Slack export, anything else is the
simple format.

## Simple JSON (per channel)

- **Source:** hand-made or scripted archives.
- **Layout:** one file per channel, `<channel>.json`, or a directory of such
  files. The file name (without `.json`) is matched against channel names,
  DM partner names and conversation ids; `--channel <name>` overrides it for
  a single file.
- **Fields** (array of objects):

  | Field | Type | Required | Maps to |
  |---|---|---|---|
  | `ts` | string `"1700000000.123456"` (a number is accepted, string preferred) | yes | `messages.ts` |
  | `author` | string: display name, real name or handle | no | `messages.user_id` if it matches a user, else `messages.author_name` |
  | `text` | string | no | `messages.text` |
  | `links` | array of strings | no | appended to `text` unless already in it |
  | `files` | array of `{name, url}` or strings | no | appended to `text` as `[file] name url` (archives have no Slack file ids) |
  | `thread_ts` | string | no | `messages.thread_ts` |

- **Example:**

  ```json
  [
    {"ts": "1700000000.000100", "author": "Alice", "text": "Grades sheet is here",
     "links": ["https://example.com/sheet"], "files": [], "thread_ts": null},
    {"ts": "1700000050.000200", "author": "bob", "text": "thanks!",
     "thread_ts": "1700000000.000100"}
  ]
  ```

- **Status:** supported (M1.8.2).

## Slack export / slackdump

- **Source:** the official Slack workspace export, and slackdump's
  `export` command (it writes the same layout). Unzip it first.
- **Layout:**

  ```
  export/
  ├── users.json                 users (added only if unknown)
  ├── channels.json              public channels
  ├── groups.json                private channels   (optional)
  ├── mpims.json                 group DMs          (optional)
  ├── dms.json                   DMs                (optional)
  ├── general/2024-01-15.json    raw Slack messages, one file per day
  └── D0123ABCD/2024-01-15.json  DM folders are named by id
  ```

- **Fields:** messages are raw Slack message objects, parsed exactly like
  `conversations.history` results (text, threads, reactions, file metadata).
  Conversations missing from the API listing (e.g. channels you left) are
  added from the `*.json` lists with their real ids.
- **Status:** supported (M1.8.4). slackdump's own `dump` JSON and SQLite
  archive formats are not supported yet.
