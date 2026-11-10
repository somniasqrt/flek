//! Offline mentions reads.

use crate::{views, Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// The indexed mention candidate source, shared by mentions and Activity.
/// Arguments are internal SQL expressions, never user input.
pub(crate) fn mention_source(terms: &str, team: &str, me: &str, broadcasts: &str) -> String {
    format!(
        "
            FROM messages_fts f
            JOIN messages m ON m.id = f.rowid
            JOIN conversations c ON c.id = m.conversation_id
            WHERE messages_fts MATCH {terms}
            AND c.team_id = {team}
            AND m.deleted = 0
            AND m.user_id IS NOT {me}
            AND (instr(m.text, '<@' || {me} || '>') > 0
                OR ({broadcasts}
                    AND (instr(m.text, '<!here>') > 0
                        OR instr(m.text, '<!channel>') > 0
                        OR instr(m.text, '<!everyone>') > 0)))
        "
    )
}

pub fn mentions(
    c: &Connection,
    team: &str,
    since: &str,
    until: Option<&str>,
    broadcasts: bool,
    limit: usize,
) -> Result<Vec<views::Message>> {
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
    let me = me.ok_or_else(|| Error::NotFound("no stored self user id for mentions".into()))?;
    let mut terms = vec![format!("\"{}\"", me.replace('"', "\"\""))];
    if broadcasts {
        terms.extend(["here", "channel", "everyone"].map(|x| format!("\"{x}\"")));
    }
    let source = mention_source("?1", "?2", "?3", "?6");
    let mut stmt = c.prepare(&format!(
        "
            SELECT m.conversation_id, m.ts {source}
            AND m.ts >= ?4
            AND (?5 IS NULL
                OR m.ts <= ?5)
            ORDER BY m.ts DESC, m.conversation_id
            LIMIT ?7
        "
    ))?;
    let keys = stmt
        .query_map(
            params![
                terms.join(" OR "),
                team,
                me,
                since,
                until,
                broadcasts,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    views::messages(c, &keys)
}
