//! Messages and per-conversation sync progress.

use rusqlite::{params, Connection, OptionalExtension};

use crate::Result;

/// A message as stored. Built by `sync` from API or live data.
#[derive(Debug, Clone, Default)]
pub struct MessageRow {
    pub ts: String,
    pub thread_ts: Option<String>,
    pub user_id: Option<String>,
    pub subtype: Option<String>,
    pub text: String,
    pub blocks_json: Option<String>,
    pub reply_count: i64,
    pub edited_ts: Option<String>,
    pub hidden_by_limit: bool,
    pub raw_json: String,
}

/// Inserts or updates messages of one conversation in a transaction.
///
/// Rule: a hidden-by-limit (content-stripped) version never replaces a
/// stored message that still has its content. It lives in the `WHERE` of
/// the upsert so every write path obeys it.
pub fn upsert_messages(
    conn: &mut Connection,
    conversation_id: &str,
    rows: &[MessageRow],
) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO messages
                (conversation_id, ts, thread_ts, user_id, subtype, text, blocks_json,
                 reply_count, edited_ts, hidden_by_limit, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(conversation_id, ts) DO UPDATE SET
                thread_ts = excluded.thread_ts, user_id = excluded.user_id,
                subtype = excluded.subtype, text = excluded.text,
                blocks_json = excluded.blocks_json, reply_count = excluded.reply_count,
                edited_ts = excluded.edited_ts, hidden_by_limit = excluded.hidden_by_limit,
                raw_json = excluded.raw_json
             WHERE excluded.hidden_by_limit = 0 OR messages.hidden_by_limit = 1",
        )?;
        for r in rows {
            stmt.execute(params![
                conversation_id,
                r.ts,
                r.thread_ts,
                r.user_id,
                r.subtype,
                r.text,
                r.blocks_json,
                r.reply_count,
                r.edited_ts,
                r.hidden_by_limit,
                r.raw_json
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// How far backfill got for one conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncState {
    /// Oldest ts fetched so far; backfill continues below it.
    pub oldest_ts: Option<String>,
    /// Newest ts fetched by a finished pass; catch-up starts above it.
    pub newest_ts: Option<String>,
    /// Backfill reached the beginning of the history.
    pub complete: bool,
}

pub fn sync_state(conn: &Connection, conversation_id: &str) -> Result<SyncState> {
    Ok(conn
        .query_row(
            "SELECT oldest_ts, newest_ts, complete FROM sync_state WHERE conversation_id = ?1",
            [conversation_id],
            |r| {
                Ok(SyncState {
                    oldest_ts: r.get(0)?,
                    newest_ts: r.get(1)?,
                    complete: r.get(2)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

pub fn set_sync_state(conn: &Connection, conversation_id: &str, s: &SyncState) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state (conversation_id, oldest_ts, newest_ts, complete, updated_at)
         VALUES (?1, ?2, ?3, ?4, unixepoch())
         ON CONFLICT(conversation_id) DO UPDATE SET
            oldest_ts = excluded.oldest_ts, newest_ts = excluded.newest_ts,
            complete = excluded.complete, updated_at = excluded.updated_at",
        params![conversation_id, s.oldest_ts, s.newest_ts, s.complete],
    )?;
    Ok(())
}

/// A message prepared for display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryRow {
    pub ts: String,
    pub user_id: Option<String>,
    pub author: Option<String>,
    pub text: String,
    pub subtype: Option<String>,
    pub reply_count: i64,
    pub edited: bool,
    pub hidden_by_limit: bool,
}

/// The latest `limit` top-level messages (thread replies excluded), oldest first.
pub fn recent(conn: &Connection, conversation_id: &str, limit: usize) -> Result<Vec<HistoryRow>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM (
            SELECT m.ts, m.user_id, COALESCE(u.display_name, u.name), COALESCE(m.text, ''),
                   m.subtype, m.reply_count, m.edited_ts IS NOT NULL, m.hidden_by_limit
            FROM messages m LEFT JOIN users u ON u.id = m.user_id
            WHERE m.conversation_id = ?1 AND m.deleted = 0
              AND (m.thread_ts IS NULL OR m.thread_ts = m.ts OR m.subtype = 'thread_broadcast')
            ORDER BY m.ts DESC LIMIT ?2
         ) ORDER BY ts",
    )?;
    let rows = stmt
        .query_map(params![conversation_id, limit as i64], |r| {
            Ok(HistoryRow {
                ts: r.get(0)?,
                user_id: r.get(1)?,
                author: r.get(2)?,
                text: r.get(3)?,
                subtype: r.get(4)?,
                reply_count: r.get(5)?,
                edited: r.get(6)?,
                hidden_by_limit: r.get(7)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}
