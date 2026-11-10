//! Message progress operations.

use crate::Result;
use rusqlite::{params, Connection, OptionalExtension};

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
            "
                SELECT oldest_ts, newest_ts, complete
                FROM sync_state
                WHERE conversation_id = ?1
            ",
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
        "
            INSERT INTO sync_state(conversation_id, oldest_ts, newest_ts, complete, updated_at)
            VALUES (?1, ?2, ?3, ?4, unixepoch())
            ON CONFLICT (conversation_id) DO UPDATE
            SET oldest_ts = excluded.oldest_ts, newest_ts = excluded.newest_ts, complete =
                excluded.complete, updated_at = excluded.updated_at
        ",
        params![conversation_id, s.oldest_ts, s.newest_ts, s.complete],
    )?;
    Ok(())
}

/// Unix time the refresh window was last re-read in full for a team.
pub fn refreshed_at(conn: &Connection, team_id: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "
                SELECT refreshed_at
                FROM refresh_runs
                WHERE team_id = ?1
            ",
            [team_id],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn set_refreshed(conn: &Connection, team_id: &str) -> Result<()> {
    conn.execute(
        "
            INSERT INTO refresh_runs(team_id, refreshed_at)
            VALUES (?1, unixepoch())
            ON CONFLICT (team_id) DO UPDATE
            SET refreshed_at = excluded.refreshed_at
        ",
        [team_id],
    )?;
    Ok(())
}

/// Thread roots of a conversation whose replies are missing or stale:
/// never synced, or the root's `latest_reply` is newer than what was fetched.
/// Roots hidden by the Free limit are skipped (their replies are hidden too).
pub fn stale_threads(conn: &Connection, conversation_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "
            SELECT m.ts
            FROM messages m
            LEFT JOIN thread_sync t ON t.conversation_id = m.conversation_id
            AND t.thread_ts = m.ts
            WHERE m.conversation_id = ?1
            AND m.reply_count > 0
            AND m.hidden_by_limit = 0
            AND (t.latest_reply IS NULL
                OR t.latest_reply < COALESCE(m.latest_reply, ''))
            ORDER BY m.ts DESC
        ",
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
        "
            SELECT m.ts
            FROM messages m
            LEFT JOIN thread_sync t ON t.conversation_id = m.conversation_id
            AND t.thread_ts = m.ts
            WHERE m.conversation_id = ?1
            AND m.reply_count > 0
            AND m.hidden_by_limit = 0
            AND (?3
                OR t.latest_reply IS NULL
                OR t.latest_reply < COALESCE(m.latest_reply, '')
                OR (?2 IS NOT NULL
                    AND (m.ts > ?2
                        OR COALESCE(m.latest_reply, '') > ?2
                        OR t.latest_reply > ?2)))
            ORDER BY m.ts DESC
        ",
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
        "
            SELECT ts
            FROM messages
            WHERE conversation_id = ?1
            AND ts > ?2
            AND ts <= ?3
            AND deleted = 0
            AND hidden_by_limit = 0
            AND (thread_ts IS NULL
                OR thread_ts = ts
                OR subtype = 'thread_broadcast')
        ",
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
        "
            SELECT ts
            FROM messages
            WHERE conversation_id = ?1
            AND thread_ts = ?2
            AND ts != ?2
            AND deleted = 0
            AND hidden_by_limit = 0
        ",
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
        "
            INSERT INTO thread_sync(conversation_id, thread_ts, latest_reply, synced_at)
            VALUES (?1, ?2, ?3, unixepoch())
            ON CONFLICT (conversation_id, thread_ts) DO UPDATE
            SET latest_reply = excluded.latest_reply, synced_at = excluded.synced_at
        ",
        params![conversation_id, thread_ts, latest_reply],
    )?;
    Ok(())
}
