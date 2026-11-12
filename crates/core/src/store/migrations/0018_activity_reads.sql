-- Activity read state that the conversation cursor cannot express. Slack's
-- thread read API is undocumented: thread cursors come from the local thread
-- panel and the `thread_marked` event. activity_seen is when this or another
-- client last looked at the conversation; it clears reactions and mentions.
ALTER TABLE conversations ADD COLUMN activity_seen TEXT;
CREATE TABLE thread_reads (
    conversation_id TEXT NOT NULL,
    thread_ts TEXT NOT NULL,
    last_read TEXT NOT NULL,
    PRIMARY KEY (conversation_id, thread_ts)
) WITHOUT ROWID;
