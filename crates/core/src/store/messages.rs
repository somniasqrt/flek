//! Messages and per-conversation sync progress.

use rusqlite::{params, Connection, OptionalExtension};

use super::files::{self, FileRow};
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
    pub latest_reply: Option<String>,
    pub edited_ts: Option<String>,
    /// Deleted in Slack (tombstone, live event, or gone from a refreshed window).
    pub deleted: bool,
    pub hidden_by_limit: bool,
    pub raw_json: String,
    /// `(emoji name, user id)`; replaces the stored set when the row is written.
    pub reactions: Vec<(String, String)>,
    /// Files attached to the message; replace the stored links likewise.
    pub files: Vec<FileRow>,
    /// Author as written in an imported archive, when it maps to no user.
    pub author_name: Option<String>,
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
    write_messages(conn, conversation_id, rows, Mode::Sync).map(drop)
}

/// Writes messages from an archive: they only fill gaps, i.e. new messages
/// and ones Slack already hides. Anything Slack still serves stays as is.
pub fn import_messages(
    conn: &mut Connection,
    conversation_id: &str,
    rows: &[MessageRow],
) -> Result<usize> {
    write_messages(conn, conversation_id, rows, Mode::Import)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Sync,
    Import,
}

fn write_messages(
    conn: &mut Connection,
    conversation_id: &str,
    rows: &[MessageRow],
    mode: Mode,
) -> Result<usize> {
    let guard = match mode {
        Mode::Sync => "excluded.hidden_by_limit = 0 OR messages.hidden_by_limit = 1",
        Mode::Import => "excluded.hidden_by_limit = 0 AND messages.hidden_by_limit = 1",
    };
    let tx = conn.transaction()?;
    let mut total = 0;
    {
        let mut stmt = tx.prepare(&format!(
            "INSERT INTO messages
                (conversation_id, ts, thread_ts, user_id, subtype, text, blocks_json,
                 reply_count, latest_reply, edited_ts, deleted, hidden_by_limit, raw_json, author_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(conversation_id, ts) DO UPDATE SET
                thread_ts = excluded.thread_ts, user_id = excluded.user_id,
                subtype = excluded.subtype, text = excluded.text,
                blocks_json = excluded.blocks_json, reply_count = excluded.reply_count,
                latest_reply = excluded.latest_reply,
                edited_ts = excluded.edited_ts, deleted = excluded.deleted,
                hidden_by_limit = excluded.hidden_by_limit,
                raw_json = excluded.raw_json,
                author_name = COALESCE(excluded.author_name, messages.author_name)
             WHERE {guard}"
        ))?;
        let mut clear_reactions =
            tx.prepare("DELETE FROM reactions WHERE conversation_id = ?1 AND ts = ?2")?;
        let mut add_reaction =
            tx.prepare("INSERT OR IGNORE INTO reactions VALUES (?1, ?2, ?3, ?4)")?;
        let mut clear_files =
            tx.prepare("DELETE FROM message_files WHERE conversation_id = ?1 AND ts = ?2")?;
        let mut link_file =
            tx.prepare("INSERT OR IGNORE INTO message_files VALUES (?1, ?2, ?3)")?;
        for r in rows {
            let written = stmt.execute(params![
                conversation_id,
                r.ts,
                r.thread_ts,
                r.user_id,
                r.subtype,
                r.text,
                r.blocks_json,
                r.reply_count,
                r.latest_reply,
                r.edited_ts,
                r.deleted,
                r.hidden_by_limit,
                r.raw_json,
                r.author_name
            ])?;
            // Upsert skipped by the hidden rule → keep everything attached too.
            if written == 0 {
                continue;
            }
            total += 1;
            clear_reactions.execute(params![conversation_id, r.ts])?;
            for (name, user) in &r.reactions {
                add_reaction.execute(params![conversation_id, r.ts, name, user])?;
            }
            clear_files.execute(params![conversation_id, r.ts])?;
            for f in &r.files {
                files::upsert_file(&tx, f)?;
                link_file.execute(params![conversation_id, r.ts, f.id])?;
            }
        }
    }
    tx.commit()?;
    Ok(total)
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

/// Thread roots of a conversation whose replies are missing or stale:
/// never synced, or the root's `latest_reply` is newer than what was fetched.
/// Roots hidden by the Free limit are skipped (their replies are hidden too).
pub fn stale_threads(conn: &Connection, conversation_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT m.ts FROM messages m
         LEFT JOIN thread_sync t ON t.conversation_id = m.conversation_id AND t.thread_ts = m.ts
         WHERE m.conversation_id = ?1 AND m.reply_count > 0 AND m.hidden_by_limit = 0
           AND (t.latest_reply IS NULL OR t.latest_reply < COALESCE(m.latest_reply, ''))
         ORDER BY m.ts DESC",
    )?;
    let rows = stmt
        .query_map([conversation_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Threads to fetch: stale ones (see [`stale_threads`]), plus roots active
/// since `active_since` (by root ts or stored `latest_reply`), or every
/// root when `all`.
pub fn threads_to_fetch(
    conn: &Connection,
    conversation_id: &str,
    active_since: Option<&str>,
    all: bool,
) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT m.ts FROM messages m
         LEFT JOIN thread_sync t ON t.conversation_id = m.conversation_id AND t.thread_ts = m.ts
         WHERE m.conversation_id = ?1 AND m.reply_count > 0 AND m.hidden_by_limit = 0
           AND (?3
                OR t.latest_reply IS NULL OR t.latest_reply < COALESCE(m.latest_reply, '')
                OR (?2 IS NOT NULL AND (m.ts > ?2 OR COALESCE(m.latest_reply, '') > ?2
                                        OR t.latest_reply > ?2)))
         ORDER BY m.ts DESC",
    )?;
    let rows = stmt
        .query_map(params![conversation_id, active_since, all], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Stored, visible top-level messages (and broadcasts) with `lower < ts <= upper`.
pub fn top_level_between(
    conn: &Connection,
    conversation_id: &str,
    lower: &str,
    upper: &str,
) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT ts FROM messages
         WHERE conversation_id = ?1 AND ts > ?2 AND ts <= ?3 AND deleted = 0 AND hidden_by_limit = 0
           AND (thread_ts IS NULL OR thread_ts = ts OR subtype = 'thread_broadcast')",
    )?;
    let rows = stmt
        .query_map(params![conversation_id, lower, upper], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Stored, visible replies of a thread (root excluded).
pub fn thread_replies(
    conn: &Connection,
    conversation_id: &str,
    thread_ts: &str,
) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT ts FROM messages
         WHERE conversation_id = ?1 AND thread_ts = ?2 AND ts != ?2 AND deleted = 0 AND hidden_by_limit = 0",
    )?;
    let rows = stmt
        .query_map(params![conversation_id, thread_ts], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Records that a thread was fetched up to `latest_reply`.
pub fn mark_thread_synced(
    conn: &Connection,
    conversation_id: &str,
    thread_ts: &str,
    latest_reply: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO thread_sync (conversation_id, thread_ts, latest_reply, synced_at)
         VALUES (?1, ?2, ?3, unixepoch())
         ON CONFLICT(conversation_id, thread_ts) DO UPDATE SET
            latest_reply = excluded.latest_reply, synced_at = excluded.synced_at",
        params![conversation_id, thread_ts, latest_reply],
    )?;
    Ok(())
}

/// Marks messages deleted (live `message_deleted`, or missing from a refresh).
pub fn mark_deleted(conn: &Connection, conversation_id: &str, ts: &[String]) -> Result<usize> {
    let mut stmt = conn.prepare(
        "UPDATE messages SET deleted = 1 WHERE conversation_id = ?1 AND ts = ?2 AND deleted = 0 AND hidden_by_limit = 0",
    )?;
    let mut n = 0;
    for t in ts {
        n += stmt.execute(params![conversation_id, t])?;
    }
    Ok(n)
}

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
        "SELECT EXISTS(SELECT 1 FROM messages WHERE conversation_id = ?1 AND ts = ?2)",
        params![conversation_id, ts],
        |r| r.get(0),
    )?;
    if exists {
        let sql = if on {
            "INSERT OR IGNORE INTO reactions VALUES (?1, ?2, ?3, ?4)"
        } else {
            "DELETE FROM reactions WHERE conversation_id = ?1 AND ts = ?2 AND name = ?3 AND user_id = ?4"
        };
        conn.execute(sql, params![conversation_id, ts, name, user])?;
    }
    Ok(exists)
}

pub fn conversation_exists(conn: &Connection, conversation_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1)",
        [conversation_id],
        |r| r.get(0),
    )?)
}

/// Reactions of one message as `(name, count)`, most used first.
pub fn reactions(conn: &Connection, conversation_id: &str, ts: &str) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT name, COUNT(*) FROM reactions WHERE conversation_id = ?1 AND ts = ?2
         GROUP BY name ORDER BY COUNT(*) DESC, name",
    )?;
    let rows = stmt
        .query_map(params![conversation_id, ts], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
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
    /// `:name: count` pairs, most used first.
    pub reactions: Option<String>,
    pub thread_ts: Option<String>,
    /// Files attached to the message.
    pub files: Vec<FileRef>,
}

/// A file attached to a message, for display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileRef {
    pub id: String,
    pub name: Option<String>,
    pub title: Option<String>,
    pub mimetype: Option<String>,
    pub filetype: Option<String>,
    pub size: Option<i64>,
    /// Cached copy on disk, once downloaded.
    pub local_path: Option<String>,
    pub permalink: Option<String>,
    pub hidden_by_limit: bool,
    /// Pixel size of images and videos, when Slack told it.
    pub width: Option<i64>,
    pub height: Option<i64>,
    /// Cached preview image (first page of a PDF, a video frame) and its size.
    pub preview_path: Option<String>,
    pub preview_width: Option<i64>,
    pub preview_height: Option<i64>,
}

const DISPLAY_COLUMNS: &str =
    "m.ts, m.user_id, COALESCE(u.display_name, u.name, m.author_name), COALESCE(m.text, ''),
    m.subtype, m.reply_count, m.edited_ts IS NOT NULL, m.hidden_by_limit,
    (SELECT group_concat(':' || name || ': ' || n, '  ') FROM (
        SELECT name, COUNT(*) AS n FROM reactions r
        WHERE r.conversation_id = m.conversation_id AND r.ts = m.ts
        GROUP BY name ORDER BY n DESC, name)),
    m.thread_ts
    FROM messages m LEFT JOIN users u ON u.id = m.user_id";

/// The latest `limit` top-level messages (thread replies excluded, broadcasts
/// kept), oldest first.
pub fn recent(conn: &Connection, conversation_id: &str, limit: usize) -> Result<Vec<HistoryRow>> {
    let sql = format!(
        "SELECT * FROM (
            SELECT {DISPLAY_COLUMNS}
            WHERE m.conversation_id = ?1 AND m.deleted = 0
              AND (m.thread_ts IS NULL OR m.thread_ts = m.ts OR m.subtype = 'thread_broadcast')
            ORDER BY m.ts DESC LIMIT ?2
         ) ORDER BY ts"
    );
    query_display(
        conn,
        conversation_id,
        &sql,
        params![conversation_id, limit as i64],
    )
}

/// Up to `limit` top-level messages older than `before` (the newest ones
/// without it), oldest first: one page of the timeline, scrolling back.
pub fn page(
    conn: &Connection,
    conversation_id: &str,
    before: Option<&str>,
    limit: usize,
) -> Result<Vec<HistoryRow>> {
    let sql = format!(
        "SELECT * FROM (
            SELECT {DISPLAY_COLUMNS}
            WHERE m.conversation_id = ?1 AND m.deleted = 0 AND (?2 IS NULL OR m.ts < ?2)
              AND (m.thread_ts IS NULL OR m.thread_ts = m.ts OR m.subtype = 'thread_broadcast')
            ORDER BY m.ts DESC LIMIT ?3
         ) ORDER BY ts"
    );
    query_display(
        conn,
        conversation_id,
        &sql,
        params![conversation_id, before, limit as i64],
    )
}

/// One message for display (live updates), deleted ones excluded.
pub fn message(conn: &Connection, conversation_id: &str, ts: &str) -> Result<Option<HistoryRow>> {
    let sql = format!(
        "SELECT {DISPLAY_COLUMNS}
         WHERE m.conversation_id = ?1 AND m.ts = ?2 AND m.deleted = 0"
    );
    Ok(
        query_display(conn, conversation_id, &sql, params![conversation_id, ts])?
            .into_iter()
            .next(),
    )
}

/// A thread: root and all replies, oldest first.
pub fn thread(
    conn: &Connection,
    conversation_id: &str,
    thread_ts: &str,
) -> Result<Vec<HistoryRow>> {
    let sql = format!(
        "SELECT {DISPLAY_COLUMNS}
         WHERE m.conversation_id = ?1 AND m.deleted = 0 AND (m.ts = ?2 OR m.thread_ts = ?2)
         ORDER BY m.ts"
    );
    query_display(
        conn,
        conversation_id,
        &sql,
        params![conversation_id, thread_ts],
    )
}

fn query_display(
    conn: &Connection,
    conversation_id: &str,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<HistoryRow>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params, |r| {
            Ok(HistoryRow {
                ts: r.get(0)?,
                user_id: r.get(1)?,
                author: r.get(2)?,
                text: r.get(3)?,
                subtype: r.get(4)?,
                reply_count: r.get(5)?,
                edited: r.get(6)?,
                hidden_by_limit: r.get(7)?,
                reactions: r.get(8)?,
                thread_ts: r.get(9)?,
                files: Vec::new(),
            })
        })?
        .collect::<Result<Vec<HistoryRow>, _>>()?;
    attach_files(conn, conversation_id, rows)
}

/// Fills `files` of `rows` (one query over their ts range).
fn attach_files(
    conn: &Connection,
    conversation_id: &str,
    mut rows: Vec<HistoryRow>,
) -> Result<Vec<HistoryRow>> {
    let (Some(lo), Some(hi)) = (
        rows.iter().map(|r| r.ts.as_str()).min(),
        rows.iter().map(|r| r.ts.as_str()).max(),
    ) else {
        return Ok(rows);
    };
    let mut stmt = conn.prepare_cached(
        "SELECT mf.ts, f.id, f.name, f.title, f.mimetype, f.filetype, f.size, f.local_path,
                f.permalink, f.hidden_by_limit,
                json_extract(f.raw_json, '$.original_w'), json_extract(f.raw_json, '$.original_h'),
                f.preview_path,
                COALESCE(json_extract(f.raw_json, '$.thumb_pdf_w'), json_extract(f.raw_json, '$.thumb_video_w')),
                COALESCE(json_extract(f.raw_json, '$.thumb_pdf_h'), json_extract(f.raw_json, '$.thumb_video_h'))
         FROM message_files mf JOIN files f ON f.id = mf.file_id
         WHERE mf.conversation_id = ?1 AND mf.ts BETWEEN ?2 AND ?3
         ORDER BY mf.ts, f.rowid",
    )?;
    let mut by_ts: std::collections::HashMap<String, Vec<FileRef>> = Default::default();
    let found = stmt.query_map(params![conversation_id, lo, hi], |r| {
        Ok((
            r.get::<_, String>(0)?,
            FileRef {
                id: r.get(1)?,
                name: r.get(2)?,
                title: r.get(3)?,
                mimetype: r.get(4)?,
                filetype: r.get(5)?,
                size: r.get(6)?,
                local_path: r.get(7)?,
                permalink: r.get(8)?,
                hidden_by_limit: r.get(9)?,
                width: r.get(10)?,
                height: r.get(11)?,
                preview_path: r.get(12)?,
                preview_width: r.get(13)?,
                preview_height: r.get(14)?,
            },
        ))
    })?;
    for item in found {
        let (ts, file) = item?;
        by_ts.entry(ts).or_default().push(file);
    }
    for row in &mut rows {
        if let Some(files) = by_ts.remove(&row.ts) {
            row.files = files;
        }
    }
    Ok(rows)
}

/// Local path of a cached file, if downloaded.
pub fn file_local_path(conn: &Connection, file_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT local_path FROM files WHERE id = ?1",
            [file_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
}
