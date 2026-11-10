-- Newest message ts Slack reports per conversation (`client.counts`
-- `latest`) at the last directory sync; NULL when unknown. A sync pass
-- skips a conversation whose stored history already reaches it.
ALTER TABLE conversations ADD COLUMN remote_latest TEXT;

-- When the refresh window (edits, reactions, deletions of recent messages)
-- was last re-read in full for a team.
CREATE TABLE refresh_runs (
    team_id       TEXT PRIMARY KEY,
    refreshed_at  INTEGER NOT NULL
);
