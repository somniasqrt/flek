-- One row per material per message, retaining each share's jump context.
-- Rust backfills links from stored text/blocks/unfurls inside this migration's
-- transaction, then maintains the catalogue after the message merge guards.
CREATE TABLE search_materials (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    team_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    ts TEXT NOT NULL,
    user_id TEXT,
    author_name TEXT,
    kind TEXT NOT NULL CHECK (kind IN ('link', 'file')),
    material_key TEXT NOT NULL,
    title TEXT NOT NULL,
    title_key TEXT NOT NULL,
    url TEXT,
    context TEXT NOT NULL,
    UNIQUE (message_id, kind, material_key)
);
CREATE INDEX materials_recent ON search_materials(team_id, ts DESC, id DESC, conversation_id, user_id, author_name, kind);
CREATE INDEX materials_name ON search_materials(team_id, title_key, id, ts, conversation_id, user_id, author_name, kind);
CREATE INDEX materials_conversation ON search_materials(team_id, conversation_id, ts DESC, id DESC);
CREATE INDEX materials_file ON search_materials(material_key) WHERE kind = 'file';
CREATE VIRTUAL TABLE materials_fts USING fts5(
    title, url, context,
    content = 'search_materials', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER materials_insert AFTER INSERT ON search_materials BEGIN
    INSERT INTO materials_fts(rowid, title, url, context)
    VALUES (new.id, new.title, new.url, new.context);
END;
CREATE TRIGGER materials_delete AFTER DELETE ON search_materials BEGIN
    INSERT INTO materials_fts(materials_fts, rowid, title, url, context)
    VALUES ('delete', old.id, old.title, old.url, old.context);
END;
CREATE TRIGGER materials_update AFTER UPDATE OF title, url, context ON search_materials BEGIN
    INSERT INTO materials_fts(materials_fts, rowid, title, url, context)
    VALUES ('delete', old.id, old.title, old.url, old.context);
    INSERT INTO materials_fts(rowid, title, url, context)
    VALUES (new.id, new.title, new.url, new.context);
END;
-- A later share can hydrate metadata for every earlier share of that file.
CREATE TRIGGER materials_file_update AFTER UPDATE OF name, title, permalink ON files
WHEN old.name IS NOT new.name OR old.title IS NOT new.title OR old.permalink IS NOT new.permalink BEGIN
    UPDATE search_materials
    SET title = COALESCE(new.name, new.title, new.id),
        title_key = lower(COALESCE(new.name, new.title, new.id)),
        url = new.permalink,
        context = (SELECT COALESCE(text, '') FROM messages WHERE id = message_id) || ' ' || COALESCE(new.title, '')
    WHERE kind = 'file' AND material_key = new.id;
END;

CREATE TRIGGER materials_message_deleted AFTER UPDATE OF deleted ON messages
WHEN new.deleted = 1 BEGIN
    DELETE FROM search_materials WHERE message_id = new.id;
END;
