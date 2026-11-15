-- 0016's 'all' default did not record whether the user chose it. Preserve
-- unambiguous existing choices; legacy 'all' inherits the conversation default.
ALTER TABLE conversations ADD COLUMN notification_explicit INTEGER NOT NULL DEFAULT 0
    CHECK (notification_explicit IN (0, 1));
UPDATE conversations SET notification_explicit = 1 WHERE notification IN ('mentions', 'muted');
