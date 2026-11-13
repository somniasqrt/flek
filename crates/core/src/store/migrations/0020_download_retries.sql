-- Retry state survives cache eviction, reconnects and process restarts.
ALTER TABLE files ADD COLUMN auto_download_paused INTEGER NOT NULL DEFAULT 0;
CREATE TABLE file_download_failures (
    file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    preview INTEGER NOT NULL DEFAULT 0,
    failure_count INTEGER NOT NULL DEFAULT 0,
    last_failure_at INTEGER NOT NULL,
    permanent INTEGER NOT NULL DEFAULT 0,
    retry_after INTEGER NOT NULL,
    PRIMARY KEY (file_id, preview)
);
ALTER TABLE link_images ADD COLUMN failure_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE link_images ADD COLUMN last_failure_at INTEGER;
ALTER TABLE link_images ADD COLUMN retry_after INTEGER NOT NULL DEFAULT 0;
-- Old failures did not distinguish HTTP refusals from disconnected networks.
-- Give them another paced attempt, rather than preserving an unknown verdict.
UPDATE link_images SET failed = 0, retry_after = unixepoch() + 60 WHERE failed = 1;

-- A compact dirty queue: updating an old message gives it a new position.
CREATE TABLE media_message_changes (
    position INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL UNIQUE REFERENCES messages(id) ON DELETE CASCADE
);
INSERT INTO media_message_changes(message_id) SELECT id FROM messages;
CREATE TRIGGER media_message_insert AFTER INSERT ON messages BEGIN
    DELETE FROM media_message_changes WHERE message_id = new.id;
    INSERT INTO media_message_changes(message_id) VALUES (new.id);
END;
CREATE TRIGGER media_message_update AFTER UPDATE OF text, raw_json, deleted ON messages
WHEN old.text IS NOT new.text OR old.raw_json IS NOT new.raw_json OR old.deleted IS NOT new.deleted
BEGIN
    DELETE FROM media_message_changes WHERE message_id = new.id;
    INSERT INTO media_message_changes(message_id) VALUES (new.id);
END;
-- Old imported collisions must not reuse the service file as an attachment.
UPDATE files SET local_path = NULL, preview_path = NULL WHERE lower(id) = '_links';
UPDATE files SET local_path = NULL WHERE lower(name) = '_flek_preview.png';
CREATE TABLE media_images (
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    PRIMARY KEY (message_id, url)
);
CREATE TABLE media_emoji (
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    PRIMARY KEY (message_id, name)
);
