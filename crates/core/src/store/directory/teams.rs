//! Teams storage views.

use crate::Result;
use rusqlite::{params, Connection, OptionalExtension};

/// Inserts or updates the team row.
pub fn upsert_team(
    conn: &Connection,
    id: &str,
    name: &str,
    url: &str,
    self_user: &str,
) -> Result<()> {
    let domain = url
        .trim_start_matches("https://")
        .split('.')
        .next()
        .filter(|d| !d.is_empty());
    conn.execute(
        "
            INSERT INTO teams(id, name, domain, url, self_user_id)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT (id) DO UPDATE
            SET name = excluded.name, domain = excluded.domain, url = excluded.url, self_user_id
                = excluded.self_user_id
        ",
        params![id, name, domain, url, self_user],
    )?;
    Ok(())
}

/// Who the stored session belongs to, as recorded by the last directory
/// sync; lets the app start offline.
pub fn stored_identity(conn: &Connection, team_id: &str) -> Result<Option<crate::api::AuthTest>> {
    Ok(conn
        .query_row(
            "
                SELECT t.url, t.name, u.name, t.id, t.self_user_id
                FROM teams t
                JOIN users u ON u.id = t.self_user_id
                WHERE t.id = ?1
            ",
            [team_id],
            |r| {
                Ok(crate::api::AuthTest {
                    url: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    team: r.get(1)?,
                    user: r.get(2)?,
                    team_id: r.get(3)?,
                    user_id: r.get(4)?,
                    enterprise_id: None,
                })
            },
        )
        .optional()?)
}

/// Store the documented team payload; credentials never belong in it.
pub fn set_team_info(c: &Connection, team: &str, info: &serde_json::Value) -> Result<()> {
    c.execute(
        "
            UPDATE teams
            SET raw_json = ?2
            WHERE id = ?1
        ",
        params![team, info.to_string()],
    )?;
    Ok(())
}

pub fn team_icon_url(c: &Connection, team: &str) -> Result<Option<String>> {
    let raw: Option<String> = c
        .query_row(
            "
                SELECT raw_json
                FROM teams
                WHERE id = ?1
            ",
            [team],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let value = raw.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    Ok(value
        .as_ref()
        .and_then(crate::api::team::icon_url)
        .map(str::to_owned))
}

pub fn team_icon(c: &Connection, team: &str) -> Result<Option<String>> {
    Ok(team_icon_url(c, team)?
        .filter(|url| crate::store::unfurls::is_cached(c, url))
        .map(|url| crate::store::unfurls::url_hash(&url)))
}
