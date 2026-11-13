//! Offline unread reads.

use crate::{views, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Serialize)]
pub struct Unread {
    #[serde(flatten)]
    pub conversation: views::Conversation,
    pub mentions: i64,
    pub first_unread_ts: Option<String>,
    pub messages: Vec<views::Message>,
}

pub fn unread(
    c: &Connection,
    team: &str,
    n: usize,
) -> Result<(Vec<Unread>, Vec<views::Conversation>)> {
    let me: Option<String> = c
        .query_row(
            "
                SELECT self_user_id
                FROM teams
                WHERE id = ?1
            ",
            [team],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let token = format!("\"{}\"", me.unwrap_or_default().replace('"', "\"\""));
    let mut stmt = c.prepare(
        "
            WITH mention_counts AS MATERIALIZED (
                SELECT m.conversation_id, COUNT(*) AS mentions
                FROM messages_fts f
                JOIN messages m ON m.id = f.rowid
                JOIN conversations c ON c.id = m.conversation_id
                JOIN teams t ON t.id = c.team_id
                WHERE messages_fts MATCH ?2
                AND c.team_id = ?1
                AND c.is_member = 1
                AND c.unread_count > 0
                AND m.deleted = 0
                AND m.ts > c.last_read
                AND (m.thread_ts IS NULL
                    OR m.thread_ts = m.ts
                    OR m.subtype IS 'thread_broadcast')
                AND m.user_id IS NOT t.self_user_id
                AND instr(m.text, '<@' || t.self_user_id || '>') > 0
                GROUP BY m.conversation_id)
            SELECT c.id, COALESCE(mc.mentions, 0), (
                SELECT m.ts
                FROM messages m
                WHERE m.conversation_id = c.id
                AND m.deleted = 0
                AND m.ts > c.last_read
                AND (m.thread_ts IS NULL
                    OR m.thread_ts = m.ts
                    OR m.subtype IS 'thread_broadcast')
                AND m.user_id IS NOT t.self_user_id
                ORDER BY m.ts
                LIMIT 1)
            FROM conversations c
            JOIN teams t ON t.id = c.team_id
            LEFT JOIN mention_counts mc ON mc.conversation_id = c.id
            WHERE c.team_id = ?1
            AND c.is_member = 1
            AND c.unread_count > 0
            ORDER BY CASE
            WHEN c.kind IN ('im', 'mpim') THEN 0
            WHEN mc.mentions > 0 THEN 1
            ELSE 2 END, c.latest_ts DESC, c.id
        ",
    )?;
    let rows = stmt
        .query_map(params![team, token], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let all = views::conversations(c, team, true)?;
    let mut attached: std::collections::BTreeMap<String, Vec<views::Message>> =
        std::collections::BTreeMap::new();
    if n > 0 {
        let mut stmt = c.prepare(
            "
                SELECT c.id, m.ts
                FROM conversations c
                JOIN teams t ON t.id = c.team_id
                JOIN messages m ON m.id IN (
                    SELECT m2.id
                    FROM messages m2
                    WHERE m2.conversation_id = c.id
                    AND m2.ts > c.last_read
                    AND m2.deleted = 0
                    AND (m2.thread_ts IS NULL
                        OR m2.thread_ts = m2.ts
                        OR m2.subtype IS 'thread_broadcast')
                    AND m2.user_id IS NOT t.self_user_id
                    ORDER BY m2.ts
                    LIMIT ?2)
                WHERE c.team_id = ?1
                AND c.is_member = 1
                AND c.unread_count > 0
                ORDER BY c.id, m.ts
            ",
        )?;
        let keys = stmt
            .query_map(params![team, i64::try_from(n).unwrap_or(i64::MAX)], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for message in views::messages(c, &keys)? {
            attached
                .entry(message.conversation_id.clone())
                .or_default()
                .push(message);
        }
    }
    let by_id: std::collections::BTreeMap<_, _> = all.iter().map(|c| (c.id.as_str(), c)).collect();
    let known = rows
        .into_iter()
        .filter_map(|(id, mentions, first_unread_ts)| {
            Some(Unread {
                conversation: (*by_id.get(id.as_str())?).clone(),
                mentions,
                first_unread_ts,
                messages: attached.remove(&id).unwrap_or_default(),
            })
        })
        .collect();
    Ok((
        known,
        all.into_iter()
            .filter(|x| x.is_member && x.unread_count.is_none())
            .collect(),
    ))
}
