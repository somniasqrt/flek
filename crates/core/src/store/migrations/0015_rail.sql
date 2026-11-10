-- Unknown backfill times stay NULL and never count as unread activity.
ALTER TABLE reactions ADD COLUMN event_ts TEXT;
CREATE INDEX reactions_activity ON reactions(event_ts DESC) WHERE event_ts IS NOT NULL;
CREATE INDEX messages_activity ON messages(ts DESC, conversation_id) WHERE deleted=0;
CREATE INDEX messages_participation ON messages(user_id, conversation_id, COALESCE(thread_ts,ts))
    WHERE deleted=0;
CREATE INDEX messages_thread_activity ON messages(conversation_id, thread_ts, ts DESC)
    WHERE deleted=0 AND thread_ts IS NOT NULL;
