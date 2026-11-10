CREATE TABLE channel_directory_sync (
    team_id TEXT PRIMARY KEY REFERENCES teams(id),
    fetched_at INTEGER NOT NULL,
    truncated INTEGER NOT NULL DEFAULT 0
);
