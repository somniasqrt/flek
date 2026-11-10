-- Empty sets need a sync marker distinct from never fetched.
CREATE TABLE conversation_members (
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    PRIMARY KEY (conversation_id,user_id)
) WITHOUT ROWID;
CREATE TABLE pins (
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    ts TEXT NOT NULL,
    pinned_by TEXT,
    pinned_at INTEGER,
    PRIMARY KEY (conversation_id,ts)
) WITHOUT ROWID;
CREATE TABLE conversation_details_sync (
    conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    synced_at INTEGER NOT NULL
);
CREATE INDEX conversations_dm_user ON conversations(team_id,dm_user_id);
