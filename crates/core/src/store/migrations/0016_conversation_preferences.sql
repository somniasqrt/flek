-- Flek-local stars and notification preferences. Slack sections/prefs are undocumented.
ALTER TABLE conversations ADD COLUMN starred INTEGER NOT NULL DEFAULT 0;
ALTER TABLE conversations ADD COLUMN notification TEXT NOT NULL DEFAULT 'all'
    CHECK (notification IN ('all', 'mentions', 'muted'));
ALTER TABLE conversations ADD COLUMN closed_at TEXT;
CREATE TRIGGER reopen_direct_message AFTER INSERT ON messages
WHEN NEW.deleted = 0
BEGIN
    UPDATE conversations SET closed_at = NULL
    WHERE id = NEW.conversation_id AND closed_at IS NOT NULL AND NEW.ts > closed_at;
END;
