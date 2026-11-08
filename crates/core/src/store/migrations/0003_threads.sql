-- Thread roots: ts of the newest reply, as reported by Slack.
ALTER TABLE messages ADD COLUMN latest_reply TEXT;

-- How far each thread's replies were fetched.
CREATE TABLE thread_sync (
    conversation_id  TEXT NOT NULL REFERENCES conversations(id),
    thread_ts        TEXT NOT NULL,
    latest_reply     TEXT NOT NULL,
    synced_at        INTEGER NOT NULL,
    PRIMARY KEY (conversation_id, thread_ts)
) WITHOUT ROWID;
