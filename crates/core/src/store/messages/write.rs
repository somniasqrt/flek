//! Message write operations.

use crate::store::files::{self, FileRow};
use crate::Result;
use rusqlite::{params, Connection, Transaction};

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
    let tx = conn.transaction()?;
    let total = write_messages_in_transaction(&tx, conversation_id, rows, mode)?;
    tx.commit()?;
    Ok(total)
}

/// Merge messages inside a caller-owned transaction, preserving all sync merge rules.
pub(crate) fn upsert_messages_in_transaction(
    tx: &Transaction<'_>,
    conversation_id: &str,
    rows: &[MessageRow],
) -> Result<()> {
    write_messages_in_transaction(tx, conversation_id, rows, Mode::Sync).map(drop)
}

fn write_messages_in_transaction(
    tx: &Transaction<'_>,
    conversation_id: &str,
    rows: &[MessageRow],
    mode: Mode,
) -> Result<usize> {
    let guard = match mode {
        Mode::Sync => {
            "
                excluded.hidden_by_limit = 0
                OR messages.hidden_by_limit = 1
            "
        }
        Mode::Import => {
            "
                excluded.hidden_by_limit = 0
                AND messages.hidden_by_limit = 1
            "
        }
    };
    // An archive only knows the content. What Slack still reports about a
    // hidden stub (thread, replies, author, reactions) stays.
    let thread_columns = match mode {
        Mode::Sync => {
            "
                thread_ts = excluded.thread_ts, user_id = excluded.user_id, subtype = excluded.subtype,
                    reply_count = excluded.reply_count, latest_reply = excluded.latest_reply
            "
        }
        Mode::Import => {
            "
                thread_ts = COALESCE(excluded.thread_ts, messages.thread_ts), user_id = COALESCE(
                        excluded.user_id, messages.user_id), subtype = COALESCE(excluded.subtype,
                        messages.subtype), reply_count = MAX(excluded.reply_count, messages.reply_count),
                    latest_reply = COALESCE(MAX(excluded.latest_reply, messages.latest_reply),
                        excluded.latest_reply, messages.latest_reply)
            "
        }
    };
    // Edits can arrive out of order (live vs. catch-up): the content of the
    // newest version by `edited.ts` wins, and an unedited version never
    // replaces an edited one. A hidden stub has nothing worth keeping.
    let newer = "
        (messages.hidden_by_limit = 1
            OR messages.edited_ts IS NULL
            OR excluded.edited_ts >= messages.edited_ts)
    ";
    let mut total = 0;
    {
        let mut stmt = tx.prepare(&format!(
            "
                INSERT INTO messages(conversation_id, ts, thread_ts, user_id, subtype, text,
                        blocks_json, reply_count, latest_reply, edited_ts, deleted, hidden_by_limit,
                        raw_json, author_name)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                ON CONFLICT (conversation_id, ts) DO UPDATE
                SET {thread_columns}, text = CASE
                WHEN {newer} THEN excluded.text
                ELSE messages.text END, blocks_json = CASE
                WHEN {newer} THEN excluded.blocks_json
                ELSE messages.blocks_json END, edited_ts = CASE
                WHEN {newer} THEN excluded.edited_ts
                ELSE messages.edited_ts END, raw_json = CASE
                WHEN {newer} THEN excluded.raw_json
                ELSE messages.raw_json END, deleted = excluded.deleted, hidden_by_limit = excluded.hidden_by_limit,
                    author_name = COALESCE(excluded.author_name, messages.author_name)
                WHERE {guard}
            "
        ))?;
        let mut clear_reactions = tx.prepare(
            "
                DELETE
                FROM reactions
                WHERE conversation_id = ?1
                AND ts = ?2
                AND NOT EXISTS (
                    SELECT 1
                    FROM json_each(?3) j
                    WHERE json_extract(j.value, '$[0]') = reactions.name
                    AND json_extract(j.value, '$[1]') = reactions.user_id)
            ",
        )?;
        let mut add_reaction = tx.prepare(
            "
                INSERT OR IGNORE INTO reactions(conversation_id, ts, name, user_id)
                VALUES (?1, ?2, ?3, ?4)
            ",
        )?;
        let mut clear_files = tx.prepare(
            "
                DELETE
                FROM message_files
                WHERE conversation_id = ?1
                AND ts = ?2
            ",
        )?;
        let mut link_file = tx.prepare(
            "
                INSERT OR IGNORE INTO message_files
                VALUES (?1, ?2, ?3)
            ",
        )?;
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
            // Sync carries Slack's full set; an archive adds to what is stored.
            if mode == Mode::Sync {
                let keep = serde_json::to_string(&r.reactions)?;
                clear_reactions.execute(params![conversation_id, r.ts, keep])?;
            }
            for (name, user) in &r.reactions {
                add_reaction.execute(params![conversation_id, r.ts, name, user])?;
            }
            if mode == Mode::Import && r.files.is_empty() {
                continue;
            }
            clear_files.execute(params![conversation_id, r.ts])?;
            for f in &r.files {
                files::upsert_file(tx, f)?;
                link_file.execute(params![conversation_id, r.ts, f.id])?;
            }
        }
    }
    Ok(total)
}

/// Marks messages deleted (live `message_deleted`, or missing from a refresh).
pub fn mark_deleted(conn: &Connection, conversation_id: &str, ts: &[String]) -> Result<usize> {
    let mut stmt = conn.prepare(
        "
            UPDATE messages
            SET deleted = 1
            WHERE conversation_id = ?1
            AND ts = ?2
            AND deleted = 0
            AND hidden_by_limit = 0
        ",
    )?;
    let mut n = 0;
    for t in ts {
        n += stmt.execute(params![conversation_id, t])?;
    }
    Ok(n)
}

/// Counts a reply that arrived live on its stored root. Idempotent: the
/// count only grows for a reply newer than the root's `latest_reply`, and
/// never drops below the replies actually stored. A later `message_replied`
/// or history fetch writes Slack's own count over it.
pub fn note_reply(
    conn: &Connection,
    conversation_id: &str,
    thread_ts: &str,
    reply_ts: &str,
) -> Result<()> {
    conn.execute(
        "
            UPDATE messages
            SET reply_count = MAX(CASE
                WHEN COALESCE(latest_reply, '') < ?3 THEN reply_count + 1
                ELSE reply_count END, (
                    SELECT COUNT(*)
                    FROM messages r
                    WHERE r.conversation_id = ?1
                    AND r.thread_ts = ?2
                    AND r.ts != ?2
                    AND r.deleted = 0)), latest_reply = MAX(COALESCE(latest_reply, ''), ?3),
                thread_ts = COALESCE(thread_ts, ts)
            WHERE conversation_id = ?1
            AND ts = ?2
        ",
        params![conversation_id, thread_ts, reply_ts],
    )?;
    Ok(())
}

/// Whether the message is stored and marked deleted.
pub fn is_deleted(conn: &Connection, conversation_id: &str, ts: &str) -> Result<bool> {
    Ok(conn.query_row(
        "
            SELECT EXISTS (
                SELECT 1
                FROM messages
                WHERE conversation_id = ?1
                AND ts = ?2
                AND deleted = 1)
        ",
        params![conversation_id, ts],
        |r| r.get(0),
    )?)
}
