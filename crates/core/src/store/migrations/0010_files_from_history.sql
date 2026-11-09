-- Messages stored before files were tracked (M1.6) kept their files only in
-- raw_json, and sync refreshes just the recent days, so those files never
-- showed up. Derive them once from what is already stored (same fields as
-- FileRow::from_slack); existing rows are left alone.
INSERT OR IGNORE INTO files (id, name, title, mimetype, filetype, size, url_private,
                             url_private_download, permalink, is_external, hidden_by_limit, raw_json)
SELECT json_extract(j.value, '$.id'),
       json_extract(j.value, '$.name'),
       json_extract(j.value, '$.title'),
       json_extract(j.value, '$.mimetype'),
       json_extract(j.value, '$.filetype'),
       json_extract(j.value, '$.size'),
       json_extract(j.value, '$.url_private'),
       json_extract(j.value, '$.url_private_download'),
       json_extract(j.value, '$.permalink'),
       COALESCE(json_extract(j.value, '$.is_external'), 0),
       json_extract(j.value, '$.mode') IS 'hidden_by_limit',
       j.value
FROM messages m, json_each(m.raw_json, '$.files') j
WHERE json_valid(m.raw_json) AND json_type(j.value, '$.id') = 'text';

INSERT OR IGNORE INTO message_files (conversation_id, ts, file_id)
SELECT m.conversation_id, m.ts, json_extract(j.value, '$.id')
FROM messages m, json_each(m.raw_json, '$.files') j
WHERE json_valid(m.raw_json) AND json_type(j.value, '$.id') = 'text';
