//! Conversations storage views.

use super::text::datetime;
use crate::references;
use crate::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Conversation {
    pub id: String,
    pub name: Option<String>,
    pub label: String,
    pub kind: String,
    pub is_archived: bool,
    pub is_member: bool,
    pub topic: Option<String>,
    pub purpose: Option<String>,
    pub unread_count: Option<i64>,
    pub last_read: Option<String>,
    pub latest_ts: Option<String>,
    pub synced_at: Option<String>,
    pub history_complete: bool,
}

const CONVERSATION_SELECT: &str = "
    SELECT c.id, c.name, COALESCE(c.label, c.name, c.id), c.kind, c.is_archived, c.is_member,
        c.topic, c.purpose, c.unread_count, c.last_read, c.latest_ts, s.updated_at,
        COALESCE(s.complete, 0)
    FROM conversations c
    LEFT JOIN sync_state s ON s.conversation_id = c.id
";

fn conversation_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Conversation> {
    let updated: Option<i64> = r.get(11)?;
    Ok(Conversation {
        id: r.get(0)?,
        name: r.get(1)?,
        label: r.get(2)?,
        kind: references::kind(&r.get::<_, String>(3)?).into(),
        is_archived: r.get(4)?,
        is_member: r.get(5)?,
        topic: r.get(6)?,
        purpose: r.get(7)?,
        unread_count: r.get(8)?,
        last_read: r.get(9)?,
        latest_ts: r.get(10)?,
        synced_at: updated.and_then(|s| datetime(&s.to_string())),
        history_complete: r.get(12)?,
    })
}

pub fn conversation(conn: &Connection, id: &str) -> Result<Option<Conversation>> {
    Ok(conn
        .query_row(
            &format!(
                "
                    {CONVERSATION_SELECT}
                    WHERE c.id = ?1
                "
            ),
            [id],
            conversation_row,
        )
        .optional()?)
}

pub fn conversations(conn: &Connection, team: &str, archived: bool) -> Result<Vec<Conversation>> {
    conversations_including(conn, team, archived, false)
}

pub fn conversations_including(
    conn: &Connection,
    team: &str,
    archived: bool,
    non_member: bool,
) -> Result<Vec<Conversation>> {
    let mut stmt = conn.prepare(&format!(
        "
            {CONVERSATION_SELECT}
            WHERE c.team_id = ?1
            AND (?3
                OR c.is_member = 1)
            AND (?2
                OR NOT c.is_archived)
            ORDER BY CASE c.kind
            WHEN 'public_channel' THEN 0
            WHEN 'private_channel' THEN 0
            WHEN 'mpim' THEN 1
            ELSE 2 END, c.is_archived, lower(COALESCE(c.label, c.name, c.id))
        "
    ))?;
    let rows = stmt
        .query_map(
            rusqlite::params![team, archived, non_member],
            conversation_row,
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Conversation permalink using the same cached workspace URL as message permalinks.
pub fn conversation_link(conn: &Connection, id: &str) -> Result<Option<String>> {
    let url: Option<String> = conn.query_row(
        "
            SELECT t.url
            FROM conversations c
            JOIN teams t ON t.id = c.team_id
            WHERE c.id = ?1
        ",
        [id],
        |r| r.get(0),
    )?;
    Ok(url
        .filter(|u| !u.is_empty())
        .map(|url| format!("{}/archives/{id}", url.trim_end_matches('/'))))
}
