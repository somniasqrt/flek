-- A failed thread must not abort or be retried on every conversation pass.
-- Existing successful progress remains eligible under the usual freshness rules.
ALTER TABLE thread_sync ADD COLUMN retry_after INTEGER;
