-- Workspace (custom) emoji from `emoji.list`, per team. A row is either an
-- image (`url`) or an alias of another emoji (`alias_of`, which may be a
-- standard one like `thumbsup`). Images are cached in `link_images`.
CREATE TABLE custom_emoji (
    team_id   TEXT NOT NULL,
    name      TEXT NOT NULL,
    url       TEXT,
    alias_of  TEXT,
    PRIMARY KEY (team_id, name)
) WITHOUT ROWID;

-- When `emoji.list` was last fetched for a team (it changes rarely).
CREATE TABLE custom_emoji_sync (
    team_id    TEXT PRIMARY KEY,
    synced_at  INTEGER NOT NULL
);
