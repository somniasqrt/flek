//! Users storage views.

use crate::api::User;
use crate::Result;
use rusqlite::{params, Connection, OptionalExtension};

/// Upserts users. Users from other teams (shared channels) get a stub team row.
pub fn upsert_users(conn: &mut Connection, default_team: &str, users: &[User]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stub_team = tx.prepare(
            "
                INSERT OR IGNORE INTO teams(id, name)
                VALUES (?1, '')
            ",
        )?;
        let mut upsert = tx.prepare(
            "
                INSERT INTO users(id, team_id, name, real_name, display_name, is_bot, deleted,
                        raw_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT (id) DO UPDATE
                SET team_id = excluded.team_id, name = excluded.name, real_name = excluded.real_name,
                    display_name = excluded.display_name, is_bot = excluded.is_bot, deleted =
                    excluded.deleted, raw_json = excluded.raw_json
            ",
        )?;
        for u in users {
            let team = u
                .team_id
                .as_deref()
                .filter(|id| !id.is_empty())
                .unwrap_or(default_team);
            if team.is_empty() {
                continue;
            }
            stub_team.execute([team])?;
            let real_name = Some(u.profile.real_name.as_str())
                .filter(|s| !s.is_empty())
                .or(u.real_name.as_deref());
            upsert.execute(params![
                u.id,
                team,
                u.name,
                real_name,
                u.label(),
                u.is_bot,
                u.deleted,
                u.raw.to_string()
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// `id → display label` of every known user of a team, for mentions.
pub fn people(
    conn: &Connection,
    team_id: &str,
) -> Result<std::collections::HashMap<String, String>> {
    let mut stmt = conn.prepare(
        "
            SELECT id, COALESCE(NULLIF(display_name, ''), NULLIF(real_name, ''), name)
            FROM users
            WHERE team_id = ?1
        ",
    )?;
    let rows = stmt
        .query_map([team_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Display label of one user, if known.
pub fn user_label(conn: &Connection, user_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "
                SELECT COALESCE(display_name, name)
                FROM users
                WHERE id = ?1
            ",
            [user_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// Live profile changes can be partial. Keep cached names/photo URLs that
/// were omitted, while honouring explicit empty status fields and nulls.
pub fn upsert_user_change(c: &mut Connection, team: &str, user: &User) -> Result<()> {
    let merged: Option<String> = c
        .query_row(
            "
                SELECT json_patch(COALESCE(raw_json, '{}'), ?2)
                FROM users
                WHERE id = ?1
            ",
            params![user.id, user.raw.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    let raw = match merged {
        Some(raw) => serde_json::from_str(&raw)?,
        None => user.raw.clone(),
    };
    let mut user: User = serde_json::from_value(raw.clone())?;
    user.raw = raw;
    upsert_users(c, team, &[user])
}
