-- Files deleted in Slack. Slack then sends `{"id": "F…", "mode": "tombstone"}`
-- in place of the file; the metadata and any cached copy we already have stay.
-- The flag is sticky: Slack cannot restore a deleted file.
ALTER TABLE files ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0;

-- Rows first stored as a tombstone.
UPDATE files SET deleted = 1 WHERE json_extract(raw_json, '$.mode') = 'tombstone';

-- Full rows whose later tombstone was (correctly) not allowed to overwrite
-- them: the message that carried the tombstone still has it in raw_json.
UPDATE files SET deleted = 1
WHERE deleted = 0
AND id IN (
    SELECT json_extract(f.value, '$.id')
    FROM messages m, json_each(m.raw_json, '$.files') f
    WHERE m.raw_json LIKE '%"tombstone"%'
    AND json_valid(m.raw_json)
    AND json_extract(f.value, '$.mode') = 'tombstone');
