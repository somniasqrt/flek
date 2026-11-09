-- Images of link previews (unfurls), fetched through Slack's image proxy.
-- `hash` (sha1 of the URL) names the cached file and the flekfile:// path.
CREATE TABLE link_images (
    hash        TEXT PRIMARY KEY,
    url         TEXT NOT NULL,
    local_path  TEXT,
    failed      INTEGER NOT NULL DEFAULT 0,
    fetched_at  INTEGER NOT NULL
);
