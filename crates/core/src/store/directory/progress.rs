//! Progress storage views.

use crate::Result;
use rusqlite::{params, Connection};
use std::collections::HashSet;

/// Stores Slack's read markers (`client.counts`, later `channel_marked` events).
pub fn set_read_markers(conn: &mut Connection, markers: &[(String, Option<String>)]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "
                UPDATE conversations
                SET last_read = ?2
                WHERE id = ?1
                AND is_member = 1
            ",
        )?;
        for (id, last_read) in markers {
            stmt.execute(params![id, last_read])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Stores the newest ts Slack reports per conversation (`client.counts`).
/// Conversations missing from `latest`, or every one when it is `None`
/// (the call failed), become unknown, so they are never skipped on old data.
pub fn set_remote_latest(
    conn: &mut Connection,
    team_id: &str,
    latest: Option<&[(String, Option<String>)]>,
) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "
            UPDATE conversations
            SET remote_latest = NULL
            WHERE team_id = ?1
        ",
        [team_id],
    )?;
    {
        let mut stmt = tx.prepare(
            "
                UPDATE conversations
                SET remote_latest = ?2
                WHERE id = ?1
                AND team_id = ?3
            ",
        )?;
        for (id, ts) in latest.unwrap_or_default() {
            stmt.execute(params![id, ts, team_id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Conversations whose stored history already reaches the newest message
/// Slack reports: a catch-up there would fetch nothing.
pub fn unchanged_conversations(conn: &Connection, team_id: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(
        "
            SELECT c.id
            FROM conversations c
            JOIN sync_state s ON s.conversation_id = c.id
            WHERE c.team_id = ?1
            AND c.is_member = 1
            AND c.remote_latest IS NOT NULL
            AND s.newest_ts IS NOT NULL
            AND c.remote_latest <= s.newest_ts
        ",
    )?;
    let ids = stmt
        .query_map([team_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

/// `(id, label)` of a team's conversations, most recently active first
/// (then never-synced ones), so fresh conversations are synced first.
pub fn conversation_activity(conn: &Connection, team_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "
            SELECT c.id, COALESCE(c.label, c.name, c.id)
            FROM conversations c
            LEFT JOIN sync_state s ON s.conversation_id = c.id
            WHERE c.team_id = ?1
            AND c.is_member = 1
            ORDER BY s.newest_ts IS NULL, s.newest_ts DESC, c.is_archived, lower(COALESCE(c.label,
                        c.name, c.id))
        ",
    )?;
    let rows = stmt
        .query_map([team_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}
