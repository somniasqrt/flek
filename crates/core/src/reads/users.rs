//! Offline users reads.

use crate::{views, Result};
use rusqlite::{params, Connection};

pub fn users(
    c: &Connection,
    team: &str,
    query: Option<&str>,
    bots: bool,
    deleted: bool,
    limit: usize,
) -> Result<Vec<views::User>> {
    let mut stmt = c.prepare(
        "
            SELECT u.id, u.name, u.display_name, u.real_name, u.is_bot, u.deleted, u.raw_json, (
                SELECT id
                FROM conversations
                WHERE dm_user_id = u.id
                AND team_id = u.team_id
                ORDER BY id
                LIMIT 1), p.user_id, p.presence, p.dnd_enabled, p.dnd_start, p.dnd_end, p.snooze_enabled,
                p.snooze_end, p.manual_away, p.auto_away, p.online, p.connection_count, p.presence_fresh,
                p.dnd_fresh, p.updated_at
            FROM users u
            LEFT JOIN presence p ON p.user_id = u.id
            WHERE u.team_id = ?1
            AND (?2
                OR NOT u.is_bot)
            AND (?3
                OR NOT u.deleted)
            ORDER BY u.name, u.id
        ",
    )?;
    let rows = stmt
        .query_map(params![team, bots, deleted], views::user_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let q = query.unwrap_or("").to_lowercase();
    Ok(rows
        .into_iter()
        .filter(|u| {
            [
                Some(&u.name),
                u.display_name.as_ref(),
                u.real_name.as_ref(),
                u.email.as_ref(),
            ]
            .iter()
            .flatten()
            .any(|s| s.to_lowercase().contains(&q))
        })
        .take(limit)
        .collect())
}
