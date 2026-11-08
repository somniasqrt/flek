-- Unread counts: the signed-in user per team, and Slack's read marker per conversation.
ALTER TABLE teams ADD COLUMN self_user_id TEXT;
ALTER TABLE conversations ADD COLUMN last_read TEXT;
