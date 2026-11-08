-- Full-text search. FTS5 external content needs a stable integer key, so
-- messages is rebuilt with an explicit INTEGER PRIMARY KEY (a plain rowid
-- may change on VACUUM). (conversation_id, ts) stays unique and remains the
-- identity everything else refers to. Runs with foreign keys off (see
-- Store::init), so reactions and message_files survive the rebuild.

CREATE TABLE messages_new (
    id               INTEGER PRIMARY KEY,
    conversation_id  TEXT NOT NULL REFERENCES conversations(id),
    ts               TEXT NOT NULL,
    thread_ts        TEXT,
    user_id          TEXT,
    subtype          TEXT,
    text             TEXT,
    blocks_json      TEXT,
    reply_count      INTEGER NOT NULL DEFAULT 0,
    latest_reply     TEXT,
    edited_ts        TEXT,
    deleted          INTEGER NOT NULL DEFAULT 0,
    hidden_by_limit  INTEGER NOT NULL DEFAULT 0,
    author_name      TEXT,
    raw_json         TEXT,
    UNIQUE (conversation_id, ts)
);

INSERT INTO messages_new
    (conversation_id, ts, thread_ts, user_id, subtype, text, blocks_json, reply_count,
     latest_reply, edited_ts, deleted, hidden_by_limit, author_name, raw_json)
SELECT conversation_id, ts, thread_ts, user_id, subtype, text, blocks_json, reply_count,
       latest_reply, edited_ts, deleted, hidden_by_limit, author_name, raw_json
FROM messages ORDER BY ts;

DROP TABLE messages;
ALTER TABLE messages_new RENAME TO messages;

CREATE INDEX messages_thread ON messages(conversation_id, thread_ts) WHERE thread_ts IS NOT NULL;
CREATE INDEX messages_user ON messages(user_id);
CREATE INDEX messages_ts ON messages(ts);

CREATE VIRTUAL TABLE messages_fts USING fts5(
    text,
    content = 'messages',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER messages_fts_update AFTER UPDATE OF text ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO messages_fts (rowid, text) VALUES (new.id, new.text);
END;

INSERT INTO messages_fts (messages_fts) VALUES ('rebuild');
