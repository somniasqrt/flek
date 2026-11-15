-- Durable commit-ordered notifications shared by independent WAL writers.
-- Content snapshots preserve successive edits; no raw API/session JSON is copied.
CREATE TABLE event_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    ts TEXT NOT NULL,
    event_ts TEXT NOT NULL,
    kind TEXT NOT NULL,
    actor_id TEXT,
    reaction TEXT,
    message_json TEXT NOT NULL
);
CREATE INDEX event_log_team_id ON event_log(team_id, id);

-- Existing archives have current state, not historical edit/remove events.
INSERT INTO event_log(team_id, conversation_id, ts, event_ts, kind, message_json)
SELECT c.team_id, m.conversation_id, m.ts, m.ts,
    CASE WHEN m.deleted THEN 'message_deleted' ELSE 'message' END, json_object('text', COALESCE(m.text, ''), 'thread_ts', m.thread_ts,
        'user_id', m.user_id, 'author_name', m.author_name, 'reply_count', m.reply_count,
        'edited', m.edited_ts IS NOT NULL, 'deleted', m.deleted,
        'hidden_by_limit', m.hidden_by_limit)
FROM messages m JOIN conversations c ON c.id = m.conversation_id
ORDER BY m.ts, m.conversation_id;

INSERT INTO event_log(team_id, conversation_id, ts, event_ts, kind, actor_id, reaction, message_json)
SELECT c.team_id, m.conversation_id, m.ts, COALESCE(r.event_ts, m.ts),
    'reaction_added', r.user_id, r.name, json_object('text', COALESCE(m.text, ''), 'thread_ts', m.thread_ts,
        'user_id', m.user_id, 'author_name', m.author_name, 'reply_count', m.reply_count,
        'edited', m.edited_ts IS NOT NULL, 'deleted', m.deleted,
        'hidden_by_limit', m.hidden_by_limit)
FROM reactions r JOIN messages m ON m.conversation_id = r.conversation_id AND m.ts = r.ts
JOIN conversations c ON c.id = m.conversation_id
ORDER BY COALESCE(r.event_ts, m.ts), m.conversation_id, r.name, r.user_id;

CREATE TRIGGER event_message_insert AFTER INSERT ON messages BEGIN
    INSERT INTO event_log(team_id, conversation_id, ts, event_ts, kind, message_json)
    SELECT c.team_id, m.conversation_id, m.ts, m.ts,
        CASE WHEN m.deleted THEN 'message_deleted' ELSE 'message' END, json_object('text', COALESCE(m.text, ''), 'thread_ts', m.thread_ts,
        'user_id', m.user_id, 'author_name', m.author_name, 'reply_count', m.reply_count,
        'edited', m.edited_ts IS NOT NULL, 'deleted', m.deleted,
        'hidden_by_limit', m.hidden_by_limit)
    FROM messages m JOIN conversations c ON c.id = m.conversation_id
    WHERE m.id = NEW.id;
END;

CREATE TRIGGER event_message_update AFTER UPDATE ON messages
WHEN OLD.text IS NOT NEW.text
    OR OLD.edited_ts IS NOT NEW.edited_ts OR OLD.deleted IS NOT NEW.deleted
    OR OLD.hidden_by_limit IS NOT NEW.hidden_by_limit
BEGIN
    INSERT INTO event_log(team_id, conversation_id, ts, event_ts, kind, message_json)
    SELECT c.team_id, m.conversation_id, m.ts, strftime('%s', 'now') || substr(strftime('%f', 'now'), 3) || '000',
        CASE WHEN m.deleted THEN 'message_deleted' ELSE 'message_changed' END, json_object('text', COALESCE(m.text, ''), 'thread_ts', m.thread_ts,
        'user_id', m.user_id, 'author_name', m.author_name, 'reply_count', m.reply_count,
        'edited', m.edited_ts IS NOT NULL, 'deleted', m.deleted,
        'hidden_by_limit', m.hidden_by_limit)
    FROM messages m JOIN conversations c ON c.id = m.conversation_id
    WHERE m.id = NEW.id;
END;

CREATE TRIGGER event_reaction_insert AFTER INSERT ON reactions BEGIN
    INSERT INTO event_log(team_id, conversation_id, ts, event_ts, kind, actor_id, reaction, message_json)
    SELECT c.team_id, m.conversation_id, m.ts, strftime('%s', 'now') || substr(strftime('%f', 'now'), 3) || '000', 'reaction_added', NEW.user_id, NEW.name,
        json_object('text', COALESCE(m.text, ''), 'thread_ts', m.thread_ts,
        'user_id', m.user_id, 'author_name', m.author_name, 'reply_count', m.reply_count,
        'edited', m.edited_ts IS NOT NULL, 'deleted', m.deleted,
        'hidden_by_limit', m.hidden_by_limit)
    FROM messages m JOIN conversations c ON c.id = m.conversation_id
    WHERE m.conversation_id = NEW.conversation_id AND m.ts = NEW.ts;
END;

CREATE TRIGGER event_reaction_delete AFTER DELETE ON reactions BEGIN
    INSERT INTO event_log(team_id, conversation_id, ts, event_ts, kind, actor_id, reaction, message_json)
    SELECT c.team_id, m.conversation_id, m.ts, strftime('%s', 'now') || substr(strftime('%f', 'now'), 3) || '000', 'reaction_removed', OLD.user_id, OLD.name,
        json_object('text', COALESCE(m.text, ''), 'thread_ts', m.thread_ts,
        'user_id', m.user_id, 'author_name', m.author_name, 'reply_count', m.reply_count,
        'edited', m.edited_ts IS NOT NULL, 'deleted', m.deleted,
        'hidden_by_limit', m.hidden_by_limit)
    FROM messages m JOIN conversations c ON c.id = m.conversation_id
    WHERE m.conversation_id = OLD.conversation_id AND m.ts = OLD.ts;
END;


-- Retention: the journal is a bounded window, never an archive (messages hold
-- the history). Ids only grow, so "older than the newest 50000" is an index
-- range delete that normally removes at most one row per insert.
DELETE FROM event_log WHERE id <= (SELECT COALESCE(MAX(id), 0) FROM event_log) - 50000;
CREATE TRIGGER event_log_retention AFTER INSERT ON event_log BEGIN
    DELETE FROM event_log WHERE id <= NEW.id - 50000;
END;
