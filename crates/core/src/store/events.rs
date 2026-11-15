//! Durable, bounded event reads. A cursor is a committed SQLite sequence,
//! independent of Slack timestamps and of the process that wrote the event.

use crate::{views, Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub channels: Vec<String>,
    pub mentions: bool,
    pub dm: bool,
    pub since: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Event {
    /// String to avoid JavaScript integer precision loss.
    pub event_id: String,
    pub event_ts: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub mention: bool,
    pub message: views::Message,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reaction: Option<String>,
}

#[derive(Deserialize)]
struct Version {
    text: String,
    thread_ts: Option<String>,
    user_id: Option<String>,
    author_name: Option<String>,
    reply_count: i64,
    edited: i64,
    deleted: i64,
    hidden_by_limit: i64,
}

pub fn self_id(c: &Connection, team: &str) -> Result<String> {
    c.query_row(
        "SELECT self_user_id FROM teams WHERE id = ?1 AND self_user_id != ''",
        [team],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .ok_or_else(|| Error::NotFound("no stored self user id for this team".into()))
}

pub fn head(c: &Connection, team: &str) -> Result<i64> {
    Ok(c.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM event_log WHERE team_id = ?1",
        [team],
        |r| r.get(0),
    )?)
}

/// Returns notifications and the next cursor, including when all rows were
/// filtered out. The high-water mark and hydrated views share one read snapshot.
pub fn page(
    c: &Connection,
    team: &str,
    after: i64,
    through: i64,
    filter: &Filter,
    limit: usize,
) -> Result<(Vec<Event>, i64)> {
    let tx = c.unchecked_transaction()?;
    let high = head(&tx, team)?.min(through);
    let me = self_id(&tx, team)?;
    let channels = serde_json::to_string(&filter.channels)?;
    let mut stmt = tx.prepare(
        "SELECT e.id, e.event_ts, e.kind, e.conversation_id, e.ts,
                e.actor_id, e.reaction, e.message_json
         FROM event_log e JOIN conversations c ON c.id = e.conversation_id
         WHERE e.team_id = ?1 AND e.id > ?2 AND e.id <= ?3
           AND (?4 IS NULL OR e.event_ts > ?4)
           AND (json_array_length(?5) = 0 OR e.conversation_id IN (SELECT value FROM json_each(?5)))
           AND (NOT ?6 OR c.kind IN ('im', 'mpim'))
           AND (NOT ?7 OR (instr(json_extract(e.message_json, '$.text'), '<@' || ?8 || '>') > 0
                AND COALESCE(json_extract(e.message_json, '$.user_id'), '') != ?8))
         ORDER BY e.id LIMIT ?9",
    )?;
    let rows = stmt
        .query_map(
            params![
                team,
                after,
                high,
                filter.since,
                channels,
                filter.dm,
                filter.mentions,
                me,
                i64::try_from(limit.max(1)).unwrap_or(i64::MAX)
            ],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, String>(7)?,
                ))
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let next = if rows.len() == limit.max(1) {
        rows.last().map_or(high, |r| r.0)
    } else {
        high
    };
    let mut events = Vec::with_capacity(rows.len());
    for (id, event_ts, kind, channel, ts, actor_id, reaction, json) in rows {
        let Some(mut message) = views::message(&tx, &channel, &ts)? else {
            continue;
        };
        let version: Version = serde_json::from_str(&json)?;
        message.text = version.text;
        message.text_resolved = views::resolve_text(&tx, &message.text)?;
        message.thread_ts = version.thread_ts.filter(|root| root != &ts);
        message.user_id = version.user_id;
        message.user = match &message.user_id {
            Some(id) => crate::store::directory::user_label(&tx, id)?.or(version.author_name),
            None => version.author_name,
        };
        message.reply_count = version.reply_count;
        message.edited = version.edited != 0;
        message.deleted = version.deleted != 0;
        message.hidden_by_limit = version.hidden_by_limit != 0;
        if let Some(link) = &mut message.permalink {
            link.truncate(link.find('?').unwrap_or(link.len()));
            if let Some(root) = &message.thread_ts {
                link.push_str(&format!("?thread_ts={root}&cid={channel}"));
            }
        }
        let mention =
            message.user_id.as_deref() != Some(&me) && message.text.contains(&format!("<@{me}>"));
        events.push(Event {
            event_id: id.to_string(),
            event_ts,
            kind,
            mention,
            message,
            actor_id,
            reaction,
        });
    }
    drop(stmt);
    tx.commit()?;
    Ok((events, next))
}

/// The oldest stored reply after the question; unknown authors cannot be
/// established to be someone other than self.
pub fn reply(
    c: &Connection,
    channel: &str,
    after: &str,
    root: Option<&str>,
    me: &str,
) -> Result<Option<views::Message>> {
    let ts = c
        .query_row(
            "SELECT ts FROM messages WHERE conversation_id = ?1 AND ts > ?2
         AND deleted = 0 AND hidden_by_limit = 0 AND user_id IS NOT NULL AND user_id != ?4
         AND (?3 IS NULL OR (thread_ts = ?3 AND ts != ?3)) ORDER BY ts LIMIT 1",
            params![channel, after, root, me],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    ts.map(|ts| views::message(c, channel, &ts))
        .transpose()
        .map(Option::flatten)
}
