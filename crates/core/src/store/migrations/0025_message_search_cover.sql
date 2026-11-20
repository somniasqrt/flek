-- M6.6.3: newest-first search sorts every FTS hit by ts. Reading those
-- columns from the full messages row (with raw_json) costs ~2.5x more than
-- this narrow covering index; store/search.rs selects it explicitly.
CREATE INDEX messages_search_cover ON messages(id, deleted, ts, conversation_id, user_id);
