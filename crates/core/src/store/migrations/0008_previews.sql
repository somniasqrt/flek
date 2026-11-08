-- Slack-rendered previews of files (first page of a PDF, a video frame),
-- cached next to the file.
ALTER TABLE files ADD COLUMN preview_path TEXT;
