CREATE TABLE message_permalinks (
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    ts TEXT NOT NULL,
    permalink TEXT NOT NULL,
    PRIMARY KEY (conversation_id, ts)
) WITHOUT ROWID;
-- Per-user state: switching the signed-in user must not share private bookmarks.
CREATE TABLE saved_messages (
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    ts TEXT NOT NULL,
    user_id TEXT NOT NULL,
    local_only INTEGER NOT NULL DEFAULT 1,
    saved_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (conversation_id, ts, user_id)
) WITHOUT ROWID;
