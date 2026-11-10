//! Message display operations.

use crate::Result;
use rusqlite::{params, Connection};

/// A message prepared for display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryRow {
    pub ts: String,
    pub user_id: Option<String>,
    pub author: Option<String>,
    pub text: String,
    pub subtype: Option<String>,
    pub reply_count: i64,
    /// Thread roots: who replied (distinct, most recent first, at most
    /// [`MAX_REPLIERS`]), from stored replies or Slack's `reply_users`.
    pub repliers: Vec<String>,
    /// Thread roots: ts of the newest reply (stored, or as Slack reported it).
    pub latest_reply: Option<String>,
    pub edited: bool,
    pub hidden_by_limit: bool,
    /// `:name: count` pairs, most used first.
    pub reactions: Option<String>,
    /// Emoji names the workspace owner reacted with.
    pub own_reactions: Vec<String>,
    pub blocks: Option<serde_json::Value>,
    pub thread_ts: Option<String>,
    /// Files attached to the message.
    pub files: Vec<FileRef>,
    /// Link previews.
    pub unfurls: Vec<crate::store::unfurls::Unfurl>,
    /// Id the sending client chose; matches an optimistic copy in the UI.
    pub client_msg_id: Option<String>,
}

/// A file attached to a message, for display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileRef {
    pub id: String,
    pub name: Option<String>,
    pub title: Option<String>,
    pub mimetype: Option<String>,
    pub filetype: Option<String>,
    pub can_open: bool,
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
/// How many repliers a thread root lists (Slack shows a few avatars).
pub const MAX_REPLIERS: usize = 4;

// Repliers and the latest reply are only looked up for thread roots
// (`reply_count > 0`), through the `messages_thread` index.
const DISPLAY_COLUMNS: &str =
    "
        m.ts, m.user_id, COALESCE(u.display_name, u.name, m.author_name), COALESCE(m.text,
                ''), m.subtype, m.reply_count, m.edited_ts IS NOT NULL, m.hidden_by_limit, (
            SELECT group_concat(':' || name || ': ' || n, '  ')
            FROM (
                SELECT name, COUNT(*) AS n
                FROM reactions r
                WHERE r.conversation_id = m.conversation_id
                AND r.ts = m.ts
                GROUP BY name
                ORDER BY n DESC, name)), m.thread_ts, m.raw_json, CASE
        WHEN m.reply_count > 0 THEN COALESCE( (
                SELECT group_concat(user_id, ',')
                FROM (
                    SELECT r.user_id, MAX(r.ts) AS LAST
                    FROM messages r
                    WHERE r.conversation_id = m.conversation_id
                    AND r.thread_ts = m.ts
                    AND r.ts != m.ts
                    AND r.deleted = 0
                    AND r.user_id IS NOT NULL
                    GROUP BY r.user_id
                    ORDER BY LAST DESC
                    LIMIT 4)), (
                SELECT group_concat(value, ',')
                FROM json_each(m.raw_json, '$.reply_users'))) END, CASE
        WHEN m.reply_count > 0 THEN COALESCE( (
                SELECT MAX(r.ts)
                FROM messages r
                WHERE r.conversation_id = m.conversation_id
                AND r.thread_ts = m.ts
                AND r.ts != m.ts
                AND r.deleted = 0), m.latest_reply) END, CASE
        WHEN instr(m.raw_json, 'client_msg_id') > 0
        AND json_valid(m.raw_json) THEN json_extract(m.raw_json, '$.client_msg_id') END, m.blocks_json
        FROM messages m
        LEFT JOIN users u ON u.id = m.user_id
    ";

/// The latest `limit` top-level messages (thread replies excluded, broadcasts
/// kept), oldest first.
pub fn recent(conn: &Connection, conversation_id: &str, limit: usize) -> Result<Vec<HistoryRow>> {
    let sql = format!(
        "
            SELECT *
            FROM (
                SELECT {DISPLAY_COLUMNS}
                WHERE m.conversation_id = ?1
                AND m.deleted = 0
                AND (m.thread_ts IS NULL
                    OR m.thread_ts = m.ts
                    OR m.subtype = 'thread_broadcast')
                ORDER BY m.ts DESC
                LIMIT ?2)
            ORDER BY ts
        "
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
        "
            SELECT *
            FROM (
                SELECT {DISPLAY_COLUMNS}
                WHERE m.conversation_id = ?1
                AND m.deleted = 0
                AND (?2 IS NULL
                    OR m.ts < ?2)
                AND (m.thread_ts IS NULL
                    OR m.thread_ts = m.ts
                    OR m.subtype = 'thread_broadcast')
                ORDER BY m.ts DESC
                LIMIT ?3)
            ORDER BY ts
        "
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
        "
            SELECT {DISPLAY_COLUMNS}
            WHERE m.conversation_id = ?1
            AND m.ts = ?2
            AND m.deleted = 0
        "
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
        "
            SELECT {DISPLAY_COLUMNS}
            WHERE m.conversation_id = ?1
            AND m.deleted = 0
            AND (m.ts = ?2
                OR m.thread_ts = ?2)
            ORDER BY m.ts
        "
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
                repliers: r
                    .get::<_, Option<String>>(11)?
                    .map(|s| {
                        s.split(',')
                            .filter(|u| !u.is_empty())
                            .take(MAX_REPLIERS)
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default(),
                latest_reply: r.get(12)?,
                edited: r.get(6)?,
                hidden_by_limit: r.get(7)?,
                reactions: r.get(8)?,
                own_reactions: Vec::new(),
                blocks: r
                    .get::<_, Option<String>>(14)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                thread_ts: r.get(9)?,
                files: Vec::new(),
                unfurls: r
                    .get::<_, Option<String>>(10)?
                    .map(|raw| {
                        crate::store::unfurls::from_raw(&raw, &|u| {
                            crate::store::unfurls::is_cached(conn, u)
                        })
                    })
                    .unwrap_or_default(),
                client_msg_id: r.get(13)?,
            })
        })?
        .collect::<Result<Vec<HistoryRow>, _>>()?;
    attach_files(conn, conversation_id, rows)
}

/// Fills files and the signed-in user's reactions with queries over the page range.
fn attach_files(
    conn: &Connection,
    conversation_id: &str,
    mut rows: Vec<HistoryRow>,
) -> Result<Vec<HistoryRow>> {
    let (Some(lo), Some(hi)) = (
        rows.iter().map(|r| &r.ts).min().cloned(),
        rows.iter().map(|r| &r.ts).max().cloned(),
    ) else {
        return Ok(rows);
    };
    let mut own = conn.prepare_cached(
        "
            SELECT r.ts, r.name
            FROM reactions r
            JOIN conversations c ON c.id = r.conversation_id
            JOIN teams t ON t.id = c.team_id
            AND t.self_user_id = r.user_id
            WHERE r.conversation_id = ?1
            AND r.ts BETWEEN ?2
            AND ?3
        ",
    )?;
    let found = own.query_map(params![conversation_id, lo, hi], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut by_reaction: std::collections::HashMap<String, Vec<String>> = Default::default();
    for item in found {
        let (ts, name) = item?;
        by_reaction.entry(ts).or_default().push(name);
    }
    for row in &mut rows {
        row.own_reactions = by_reaction.remove(&row.ts).unwrap_or_default();
    }
    let mut stmt = conn.prepare_cached(
        "
            SELECT mf.ts, f.id, f.name, f.title, f.mimetype, f.filetype, f.size, f.local_path, f.permalink,
                f.hidden_by_limit, json_extract(f.raw_json, '$.original_w'), json_extract(f.raw_json,
                    '$.original_h'), f.preview_path, COALESCE(json_extract(f.raw_json,
                        '$.thumb_pdf_w'), json_extract(f.raw_json, '$.thumb_video_w')), COALESCE(
                    json_extract(f.raw_json, '$.thumb_pdf_h'), json_extract(f.raw_json,
                        '$.thumb_video_h'))
            FROM message_files mf
            JOIN files f ON f.id = mf.file_id
            WHERE mf.conversation_id = ?1
            AND mf.ts BETWEEN ?2
            AND ?3
            ORDER BY mf.ts, f.rowid
        ",
    )?;
    let mut by_ts: std::collections::HashMap<String, Vec<FileRef>> = Default::default();
    let found = stmt.query_map(params![conversation_id, lo, hi], |r| {
        let name: Option<String> = r.get(2)?;
        let mimetype: Option<String> = r.get(4)?;
        let local_path: Option<String> = r.get(7)?;
        let path = local_path
            .as_deref()
            .or(name.as_deref())
            .unwrap_or_default();
        let can_open = crate::files::can_open(std::path::Path::new(path), mimetype.as_deref());
        Ok((
            r.get::<_, String>(0)?,
            FileRef {
                id: r.get(1)?,
                name: name.as_deref().map(crate::files::display_name),
                title: r
                    .get::<_, Option<String>>(3)?
                    .as_deref()
                    .map(crate::files::display_name),
                mimetype,
                can_open,
                filetype: r.get(5)?,
                size: r.get(6)?,
                local_path,
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
