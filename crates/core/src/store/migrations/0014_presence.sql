CREATE TABLE presence (
    user_id TEXT PRIMARY KEY,
    presence TEXT,
    dnd_enabled INTEGER NOT NULL DEFAULT 0,
    dnd_start INTEGER NOT NULL DEFAULT 0,
    dnd_end INTEGER NOT NULL DEFAULT 0,
    snooze_enabled INTEGER NOT NULL DEFAULT 0,
    snooze_end INTEGER NOT NULL DEFAULT 0,
    manual_away INTEGER,
    auto_away INTEGER,
    online INTEGER,
    connection_count INTEGER,
    presence_fresh INTEGER NOT NULL DEFAULT 0,
    dnd_fresh INTEGER NOT NULL DEFAULT 0,
    presence_version INTEGER NOT NULL DEFAULT 0,
    dnd_version INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);
