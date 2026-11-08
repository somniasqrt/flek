-- Stored per-conversation counters, kept by triggers, so listing
-- conversations never counts messages.
--   message_count  messages not deleted (thread replies included)
--   unread_count   top-level messages (and thread broadcasts) from others
--                  after last_read; NULL while the read marker is unknown
--   latest_ts      newest message ever stored (activity order)
-- A message counts as unread when it is not deleted, newer than last_read,
-- top-level or a broadcast, and not written by the team's signed-in user.
-- Rebuilding `messages` drops these triggers: recreate them afterwards.

ALTER TABLE conversations ADD COLUMN message_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE conversations ADD COLUMN unread_count INTEGER;
ALTER TABLE conversations ADD COLUMN latest_ts TEXT;

UPDATE conversations SET
    message_count = (SELECT COUNT(*) FROM messages m
                     WHERE m.conversation_id = conversations.id AND m.deleted = 0),
    latest_ts = (SELECT MAX(ts) FROM messages m WHERE m.conversation_id = conversations.id),
    unread_count = CASE WHEN last_read IS NULL THEN NULL ELSE (
        SELECT COUNT(*) FROM messages m
        WHERE m.conversation_id = conversations.id AND m.deleted = 0 AND m.ts > last_read
          AND (m.thread_ts IS NULL OR m.thread_ts = m.ts OR m.subtype IS 'thread_broadcast')
          AND m.user_id IS NOT (SELECT self_user_id FROM teams WHERE teams.id = conversations.team_id)
    ) END;

CREATE TRIGGER messages_count_insert AFTER INSERT ON messages BEGIN
    UPDATE conversations SET
        latest_ts = CASE WHEN latest_ts IS NULL OR new.ts > latest_ts THEN new.ts ELSE latest_ts END,
        message_count = message_count + (new.deleted = 0),
        unread_count = unread_count + (
            new.deleted = 0 AND last_read IS NOT NULL AND new.ts > last_read
            AND (new.thread_ts IS NULL OR new.thread_ts = new.ts OR new.subtype IS 'thread_broadcast')
            AND new.user_id IS NOT (SELECT self_user_id FROM teams WHERE teams.id = conversations.team_id))
    WHERE id = new.conversation_id;
END;

CREATE TRIGGER messages_count_delete AFTER DELETE ON messages WHEN old.deleted = 0 BEGIN
    UPDATE conversations SET
        message_count = message_count - 1,
        unread_count = unread_count - (
            last_read IS NOT NULL AND old.ts > last_read
            AND (old.thread_ts IS NULL OR old.thread_ts = old.ts OR old.subtype IS 'thread_broadcast')
            AND old.user_id IS NOT (SELECT self_user_id FROM teams WHERE teams.id = conversations.team_id))
    WHERE id = old.conversation_id;
END;

-- Upserts rewrite every column; only react when a counted property changed.
CREATE TRIGGER messages_count_update
AFTER UPDATE OF conversation_id, ts, thread_ts, user_id, subtype, deleted ON messages
WHEN old.conversation_id IS NOT new.conversation_id OR old.ts IS NOT new.ts
  OR old.thread_ts IS NOT new.thread_ts OR old.user_id IS NOT new.user_id
  OR old.subtype IS NOT new.subtype OR old.deleted IS NOT new.deleted
BEGIN
    UPDATE conversations SET
        message_count = message_count - (old.deleted = 0),
        unread_count = unread_count - (
            old.deleted = 0 AND last_read IS NOT NULL AND old.ts > last_read
            AND (old.thread_ts IS NULL OR old.thread_ts = old.ts OR old.subtype IS 'thread_broadcast')
            AND old.user_id IS NOT (SELECT self_user_id FROM teams WHERE teams.id = conversations.team_id))
    WHERE id = old.conversation_id;
    UPDATE conversations SET
        latest_ts = CASE WHEN latest_ts IS NULL OR new.ts > latest_ts THEN new.ts ELSE latest_ts END,
        message_count = message_count + (new.deleted = 0),
        unread_count = unread_count + (
            new.deleted = 0 AND last_read IS NOT NULL AND new.ts > last_read
            AND (new.thread_ts IS NULL OR new.thread_ts = new.ts OR new.subtype IS 'thread_broadcast')
            AND new.user_id IS NOT (SELECT self_user_id FROM teams WHERE teams.id = conversations.team_id))
    WHERE id = new.conversation_id;
END;

-- A moved read marker recounts one conversation (index range on (conversation_id, ts)).
CREATE TRIGGER conversations_unread_recount AFTER UPDATE OF last_read ON conversations
WHEN old.last_read IS NOT new.last_read
BEGIN
    UPDATE conversations SET unread_count = CASE WHEN new.last_read IS NULL THEN NULL ELSE (
        SELECT COUNT(*) FROM messages m
        WHERE m.conversation_id = new.id AND m.deleted = 0 AND m.ts > new.last_read
          AND (m.thread_ts IS NULL OR m.thread_ts = m.ts OR m.subtype IS 'thread_broadcast')
          AND m.user_id IS NOT (SELECT self_user_id FROM teams WHERE teams.id = new.team_id)
    ) END
    WHERE id = new.id;
END;

CREATE TRIGGER teams_unread_recount AFTER UPDATE OF self_user_id ON teams
WHEN old.self_user_id IS NOT new.self_user_id
BEGIN
    UPDATE conversations SET unread_count = CASE WHEN last_read IS NULL THEN NULL ELSE (
        SELECT COUNT(*) FROM messages m
        WHERE m.conversation_id = conversations.id AND m.deleted = 0 AND m.ts > last_read
          AND (m.thread_ts IS NULL OR m.thread_ts = m.ts OR m.subtype IS 'thread_broadcast')
          AND m.user_id IS NOT new.self_user_id
    ) END
    WHERE team_id = new.id;
END;
