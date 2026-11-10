//! Message reactions operations.

use crate::Result;
use rusqlite::{params, Connection};

/// One reaction from a live event. Returns whether the message is stored.
pub fn set_reaction(
    conn: &Connection,
    conversation_id: &str,
    ts: &str,
    name: &str,
    user: &str,
    on: bool,
) -> Result<bool> {
    let exists: bool = conn.query_row(
        "
            SELECT EXISTS (
                SELECT 1
                FROM messages
                WHERE conversation_id = ?1
                AND ts = ?2)
        ",
        params![conversation_id, ts],
        |r| r.get(0),
    )?;
    if exists {
        let sql = if on {
            "
                INSERT OR IGNORE INTO reactions(conversation_id, ts, name, user_id)
                VALUES (?1, ?2, ?3, ?4)
            "
        } else {
            "
                DELETE
                FROM reactions
                WHERE conversation_id = ?1
                AND ts = ?2
                AND name = ?3
                AND user_id = ?4
            "
        };
        conn.execute(sql, params![conversation_id, ts, name, user])?;
    }
    Ok(exists)
}

/// Reactions of one message as `(name, count)`, most used first.
pub fn reactions(conn: &Connection, conversation_id: &str, ts: &str) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "
            SELECT name, COUNT(*)
            FROM reactions
            WHERE conversation_id = ?1
            AND ts = ?2
            GROUP BY name
            ORDER BY COUNT(*) DESC, name
        ",
    )?;
    let rows = stmt
        .query_map(params![conversation_id, ts], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}
