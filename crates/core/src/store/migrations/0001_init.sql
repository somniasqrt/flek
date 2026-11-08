-- Schema v1. Slack ts values are kept as TEXT ("1700000000.123456"):
-- fixed width, so string order is time order.

CREATE TABLE teams (
    id        TEXT PRIMARY KEY,
    name      TEXT NOT NULL,
    domain    TEXT,
    url       TEXT,
    raw_json  TEXT
);

CREATE TABLE users (
    id            TEXT PRIMARY KEY,
    team_id       TEXT NOT NULL REFERENCES teams(id),
    name          TEXT NOT NULL,
    real_name     TEXT,
    display_name  TEXT,
    is_bot        INTEGER NOT NULL DEFAULT 0,
    deleted       INTEGER NOT NULL DEFAULT 0,
    raw_json      TEXT
);
CREATE INDEX users_team ON users(team_id);

CREATE TABLE conversations (
    id           TEXT PRIMARY KEY,
    team_id      TEXT NOT NULL REFERENCES teams(id),
    kind         TEXT NOT NULL CHECK (kind IN ('public_channel', 'private_channel', 'mpim', 'im')),
    name         TEXT,
    dm_user_id   TEXT,
    topic        TEXT,
    purpose      TEXT,
    is_archived  INTEGER NOT NULL DEFAULT 0,
    is_member    INTEGER NOT NULL DEFAULT 1,
    created      INTEGER,
    raw_json     TEXT
);
CREATE INDEX conversations_team ON conversations(team_id);

CREATE TABLE messages (
    conversation_id  TEXT NOT NULL REFERENCES conversations(id),
    ts               TEXT NOT NULL,
    thread_ts        TEXT,
    user_id          TEXT,
    subtype          TEXT,
    text             TEXT,
    blocks_json      TEXT,
    reply_count      INTEGER NOT NULL DEFAULT 0,
    edited_ts        TEXT,
    deleted          INTEGER NOT NULL DEFAULT 0,
    hidden_by_limit  INTEGER NOT NULL DEFAULT 0,
    raw_json         TEXT,
    PRIMARY KEY (conversation_id, ts)
) WITHOUT ROWID;
CREATE INDEX messages_thread ON messages(conversation_id, thread_ts) WHERE thread_ts IS NOT NULL;
CREATE INDEX messages_user ON messages(user_id);

CREATE TABLE reactions (
    conversation_id  TEXT NOT NULL,
    ts               TEXT NOT NULL,
    name             TEXT NOT NULL,
    user_id          TEXT NOT NULL,
    PRIMARY KEY (conversation_id, ts, name, user_id),
    FOREIGN KEY (conversation_id, ts) REFERENCES messages(conversation_id, ts) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE TABLE files (
    id                    TEXT PRIMARY KEY,
    name                  TEXT,
    title                 TEXT,
    mimetype              TEXT,
    filetype              TEXT,
    size                  INTEGER,
    url_private           TEXT,
    url_private_download  TEXT,
    permalink             TEXT,
    is_external           INTEGER NOT NULL DEFAULT 0,
    hidden_by_limit       INTEGER NOT NULL DEFAULT 0,
    local_path            TEXT,
    raw_json              TEXT
);

-- A file can be shared in several messages.
CREATE TABLE message_files (
    conversation_id  TEXT NOT NULL,
    ts               TEXT NOT NULL,
    file_id          TEXT NOT NULL REFERENCES files(id),
    PRIMARY KEY (conversation_id, ts, file_id),
    FOREIGN KEY (conversation_id, ts) REFERENCES messages(conversation_id, ts) ON DELETE CASCADE
) WITHOUT ROWID;
CREATE INDEX message_files_file ON message_files(file_id);

CREATE TABLE sync_state (
    conversation_id  TEXT PRIMARY KEY REFERENCES conversations(id),
    oldest_ts        TEXT,
    newest_ts        TEXT,
    -- 1 once backfill reached the beginning of the history.
    complete         INTEGER NOT NULL DEFAULT 0,
    updated_at       INTEGER NOT NULL
);
