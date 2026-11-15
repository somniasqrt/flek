//! Local notification policy and committed-message collection. No OS or network access.
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::{settings::Settings, store::presence::Presence, Result};

#[derive(Clone, Serialize, Deserialize)]
pub struct Target {
    pub team_id: String,
    pub conversation_id: String,
    pub ts: String,
    pub thread_ts: Option<String>,
}

pub struct Candidate {
    pub target: Target,
    pub sender: Option<String>,
    pub kind: String,
    /// Explicit all/mentions/muted, or "default" for an inherited choice.
    pub preference: String,
    pub text: String,
    pub subtype: Option<String>,
    pub deleted: bool,
    pub hidden: bool,
    pub member: bool,
    pub archived: bool,
    pub last_read: Option<String>,
    pub conversation: String,
    pub author: String,
}

pub struct Context<'a> {
    pub user_id: &'a str,
    /// The native shell supplies this only when main is visible and focused.
    pub focused_conversation: Option<&'a str>,
    pub presence: Option<&'a Presence>,
    pub now: i64,
}

/// Unknown DND is deliberately quiet until a fresh self snapshot/event arrives.
/// Explicit all includes channel messages; mentions requires a mention even in DMs.
/// Default means all for im/mpim and mentions for channels.
pub fn should_notify(message: &Candidate, settings: &Settings, context: &Context<'_>) -> bool {
    if !settings.notifications_enabled
        || message.sender.as_deref() == Some(context.user_id)
        || message.deleted
        || message.hidden
        || !message.member
        || message.archived
        || message.preference == "muted"
        || context.focused_conversation == Some(message.target.conversation_id.as_str())
        || message
            .last_read
            .as_ref()
            .is_some_and(|read| read >= &message.target.ts)
        || !context
            .presence
            .is_some_and(|p| p.dnd_fresh && p.dnd_until(context.now).is_none())
        || !matches!(
            message.subtype.as_deref(),
            None | Some("bot_message" | "file_share" | "thread_broadcast" | "me_message")
        )
    {
        return false;
    }
    let mention = message.text.contains(&format!("<@{}>", context.user_id))
        || ["<!channel>", "<!here>", "<!everyone>"]
            .iter()
            .any(|token| message.text.contains(token));
    match message.preference.as_str() {
        "mentions" => mention,
        "all" => true,
        "default" => matches!(message.kind.as_str(), "im" | "mpim") || mention,
        _ => false,
    }
}

#[derive(Default)]
pub struct Batch {
    pub count: usize,
    pub latest: Option<Candidate>,
}

pub fn watermark(conn: &Connection) -> Result<i64> {
    Ok(
        conn.query_row("SELECT COALESCE(MAX(id), 0) FROM messages", [], |r| {
            r.get(0)
        })?,
    )
}

/// Scan only newly inserted identities, never edits/echoes. The fixed startup
/// cutoff excludes historical backfill and previous-run notifications. Reconnect
/// collection is delayed by the shell until catch-up + self DND have settled.
/// Cursor advancement and aggregation are returned together, even if all are suppressed.
pub fn collect(
    conn: &Connection,
    after: i64,
    since: &str,
    team: &str,
    settings: &Settings,
    context: &Context<'_>,
) -> Result<(i64, Batch)> {
    let until = watermark(conn)?;
    let mut statement = conn.prepare(
        "SELECT m.conversation_id, m.ts, m.thread_ts, m.user_id, c.kind,
         CASE WHEN c.notification_explicit = 1 OR c.notification = 'muted'
              THEN c.notification ELSE 'default' END,
         m.text, m.subtype, m.deleted, m.hidden_by_limit, c.is_member, c.is_archived,
         CASE WHEN tr.last_read IS NOT NULL
              AND (c.last_read IS NULL OR tr.last_read > c.last_read)
              THEN tr.last_read ELSE c.last_read END,
         COALESCE(c.label, c.name, c.id)
         FROM messages m JOIN conversations c ON c.id = m.conversation_id
         LEFT JOIN thread_reads tr ON tr.conversation_id = m.conversation_id
              AND tr.thread_ts = COALESCE(m.thread_ts, m.ts)
         WHERE m.id > ?1 AND m.id <= ?2 AND m.ts >= ?3 AND c.team_id = ?4 ORDER BY m.id",
    )?;
    let rows = statement.query_map(params![after, until, since, team], |r| {
        Ok(Candidate {
            target: Target {
                team_id: team.into(),
                conversation_id: r.get(0)?,
                ts: r.get(1)?,
                thread_ts: r.get(2)?,
            },
            sender: r.get(3)?,
            kind: r.get(4)?,
            preference: r.get(5)?,
            text: r.get(6)?,
            subtype: r.get(7)?,
            deleted: r.get(8)?,
            hidden: r.get(9)?,
            member: r.get(10)?,
            archived: r.get(11)?,
            last_read: r.get(12)?,
            conversation: r.get(13)?,
            author: String::new(),
        })
    })?;
    let mut batch = Batch::default();
    for row in rows {
        let message = row?;
        if should_notify(&message, settings, context) {
            batch.count += 1;
            if batch
                .latest
                .as_ref()
                .is_none_or(|previous| previous.target.ts < message.target.ts)
            {
                batch.latest = Some(message);
            }
        }
    }
    if let Some(message) = batch.latest.as_mut() {
        message.author = message
            .sender
            .as_deref()
            .map(|id| crate::store::directory::user_label(conn, id))
            .transpose()?
            .flatten()
            .unwrap_or_default();
        message.text = crate::views::resolve_text(conn, &message.text)?
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(240)
            .collect();
    }
    Ok((until, batch))
}

/// Same cached top-level unread counts as the sidebar, excluding muted/closed
/// conversations. DND and banner preferences do not erase unread state.
pub fn badge_count(conn: &Connection, team: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(unread_count), 0) FROM conversations
         WHERE team_id = ?1 AND is_member = 1 AND is_archived = 0
         AND closed_at IS NULL AND notification != 'muted'",
        [team],
        |r| r.get(0),
    )?)
}
