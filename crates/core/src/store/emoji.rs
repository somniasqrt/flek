//! Workspace (custom) emoji from `emoji.list` and their cached images.

use std::collections::{BTreeSet, HashMap};

use rusqlite::{params, Connection, OptionalExtension};

use crate::store::unfurls;
use crate::Result;

/// Longest alias chain followed (Slack allows only one level, be lenient).
const MAX_ALIAS_DEPTH: usize = 8;

/// Custom emoji as the UI needs them.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct CustomEmoji {
    /// `name → cached image hash` (served at `flekfile://localhost/link/<hash>`),
    /// aliases of custom emoji included. Not yet cached ones are left out.
    pub images: HashMap<String, String>,
    /// `name → standard emoji name` for aliases of standard emoji
    /// (`alias:thumbsup`).
    pub aliases: HashMap<String, String>,
}

/// Replaces a team's custom emoji with `list` (`name → url | "alias:other"`,
/// as `emoji.list` returns it) and records the time.
pub fn replace(conn: &mut Connection, team_id: &str, list: &HashMap<String, String>) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM custom_emoji WHERE team_id = ?1", [team_id])?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO custom_emoji (team_id, name, url, alias_of) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (name, value) in list {
            let (url, alias) = match value.strip_prefix("alias:") {
                Some(target) => (None, Some(target)),
                None if value.starts_with("https://") || value.starts_with("http://") => {
                    (Some(value.as_str()), None)
                }
                None => continue,
            };
            insert.execute(params![team_id, name, url, alias])?;
        }
    }
    tx.execute(
        "INSERT INTO custom_emoji_sync (team_id, synced_at) VALUES (?1, unixepoch())
         ON CONFLICT(team_id) DO UPDATE SET synced_at = excluded.synced_at",
        [team_id],
    )?;
    tx.commit()?;
    Ok(())
}

/// Unix time `emoji.list` was last stored for the team.
pub fn synced_at(conn: &Connection, team_id: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT synced_at FROM custom_emoji_sync WHERE team_id = ?1",
            [team_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// Every custom emoji with aliases followed: `(name → image url, name →
/// standard emoji)`. Broken or circular aliases are dropped.
fn resolved(
    conn: &Connection,
    team_id: &str,
) -> Result<(HashMap<String, String>, HashMap<String, String>)> {
    let mut stmt =
        conn.prepare("SELECT name, url, alias_of FROM custom_emoji WHERE team_id = ?1")?;
    let rows: HashMap<String, (Option<String>, Option<String>)> = stmt
        .query_map([team_id], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
        .collect::<Result<_, _>>()?;
    let mut images = HashMap::new();
    let mut aliases = HashMap::new();
    for name in rows.keys() {
        let mut current = name.as_str();
        for _ in 0..=MAX_ALIAS_DEPTH {
            match rows.get(current) {
                Some((Some(url), _)) => {
                    images.insert(name.clone(), url.clone());
                    break;
                }
                Some((None, Some(target))) => current = target,
                Some((None, None)) => break,
                // Not a custom one: a standard emoji (the UI knows those).
                None => {
                    aliases.insert(name.clone(), current.to_owned());
                    break;
                }
            }
        }
    }
    Ok((images, aliases))
}

/// Custom emoji for the UI: only images already in the cache.
pub fn custom_emoji(conn: &Connection, team_id: &str) -> Result<CustomEmoji> {
    let (urls, aliases) = resolved(conn, team_id)?;
    let images = urls
        .into_iter()
        .filter(|(_, url)| unfurls::is_cached(conn, url))
        .map(|(name, url)| (name, unfurls::url_hash(&url)))
        .collect();
    Ok(CustomEmoji { images, aliases })
}

/// `:name:` candidates in a message text.
fn names_in(text: &str) -> impl Iterator<Item = &str> {
    let parts: Vec<&str> = text.split(':').collect();
    let inner = parts.len().saturating_sub(1);
    parts
        .into_iter()
        .take(inner)
        .skip(1)
        .filter(|s| !s.is_empty() && s.len() <= 100 && !s.contains(char::is_whitespace))
}

/// Image URLs of custom emoji that stored messages or reactions use and that
/// were not tried yet, at most `limit`. Unused emoji are never fetched.
pub fn to_fetch(conn: &Connection, team_id: &str, limit: usize) -> Result<Vec<String>> {
    let (urls, _) = resolved(conn, team_id)?;
    let mut known = conn.prepare_cached("SELECT 1 FROM link_images WHERE hash = ?1")?;
    let mut pending: HashMap<&str, &str> = HashMap::new();
    for (name, url) in &urls {
        if !known.exists([unfurls::url_hash(url)])? {
            pending.insert(name.as_str(), url.as_str());
        }
    }
    if pending.is_empty() {
        return Ok(Vec::new());
    }

    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut add = |name: &str| {
        let base = name.split("::").next().unwrap_or(name);
        if let Some(url) = pending.get(base) {
            used.insert((*url).to_owned());
        }
    };
    let mut reactions = conn.prepare(
        "SELECT DISTINCT r.name FROM reactions r
         JOIN conversations c ON c.id = r.conversation_id WHERE c.team_id = ?1",
    )?;
    let mut rows = reactions.query([team_id])?;
    while let Some(r) = rows.next()? {
        add(&r.get::<_, String>(0)?);
    }
    let mut texts = conn.prepare(
        "SELECT m.text FROM messages m
         JOIN conversations c ON c.id = m.conversation_id
         WHERE c.team_id = ?1 AND m.deleted = 0 AND m.text LIKE '%:%:%'",
    )?;
    let mut rows = texts.query([team_id])?;
    while let Some(r) = rows.next()? {
        let text: String = r.get(0)?;
        names_in(&text).for_each(&mut add);
    }
    Ok(used.into_iter().take(limit).collect())
}

#[cfg(test)]
mod tests {
    use super::names_in;

    #[test]
    fn finds_names_between_colons() {
        let names: Vec<_> =
            names_in("hi :party: and :+1::skin-tone-2: at 10:30 :not one:").collect();
        assert!(names.contains(&"party"));
        assert!(names.contains(&"+1"));
        assert!(names.contains(&"skin-tone-2"));
        assert!(!names.iter().any(|n| n.contains(' ')));
        assert_eq!(names_in("no emoji: here").count(), 0);
    }
}
