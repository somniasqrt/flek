//! File metadata and the local cache path.

use rusqlite::{params, Connection, Transaction};
use serde_json::Value;

use crate::Result;

/// File metadata as stored.
#[derive(Debug, Clone, Default)]
pub struct FileRow {
    pub id: String,
    pub name: Option<String>,
    pub title: Option<String>,
    pub mimetype: Option<String>,
    pub filetype: Option<String>,
    pub size: Option<i64>,
    pub url_private: Option<String>,
    pub url_private_download: Option<String>,
    pub permalink: Option<String>,
    pub is_external: bool,
    pub hidden_by_limit: bool,
    pub raw_json: String,
}

impl FileRow {
    /// From a Slack file object. `None` when it has no id.
    pub fn from_slack(v: &Value) -> Option<Self> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        Some(Self {
            id: s("id")?,
            name: s("name"),
            title: s("title"),
            mimetype: s("mimetype"),
            filetype: s("filetype"),
            size: v.get("size").and_then(Value::as_i64),
            url_private: s("url_private"),
            url_private_download: s("url_private_download"),
            permalink: s("permalink"),
            is_external: v
                .get("is_external")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            hidden_by_limit: s("mode").as_deref() == Some("hidden_by_limit"),
            raw_json: v.to_string(),
        })
    }
}

/// Upserts a file; hidden versions cannot replace full ones, and tombstones
/// never overwrite stored metadata. Cache paths are never touched here.
pub(crate) fn upsert_file(tx: &Transaction<'_>, f: &FileRow) -> Result<()> {
    tx.prepare_cached(
        "
            INSERT INTO files(id, name, title, mimetype, filetype, SIZE, url_private,
                    url_private_download, permalink, is_external, hidden_by_limit, raw_json)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT (id) DO UPDATE
            SET name = excluded.name, title = excluded.title, mimetype = excluded.mimetype,
                filetype = excluded.filetype, SIZE = excluded.size, url_private = excluded.url_private,
                url_private_download = excluded.url_private_download, permalink = excluded.permalink,
                is_external = excluded.is_external, hidden_by_limit = excluded.hidden_by_limit,
                raw_json = excluded.raw_json
            WHERE COALESCE(json_extract(excluded.raw_json, '$.mode'), '') != 'tombstone'
            AND (excluded.hidden_by_limit = 0
                OR files.hidden_by_limit = 1)
        ",
    )?
    .execute(params![
        f.id,
        f.name,
        f.title,
        f.mimetype,
        f.filetype,
        f.size,
        f.url_private,
        f.url_private_download,
        f.permalink,
        f.is_external,
        f.hidden_by_limit,
        f.raw_json
    ])?;
    Ok(())
}

/// A file that should be in the local cache.
#[derive(Debug, Clone)]
pub struct PendingDownload {
    pub id: String,
    pub name: Option<String>,
    pub mimetype: Option<String>,
    pub size: Option<i64>,
    pub url: String,
    /// Recorded path (the file may have been removed from the cache since).
    pub local_path: Option<String>,
}

/// Downloadable files: hosted by Slack, not hidden, within `max_size`.
pub fn downloadable(conn: &Connection, max_size: Option<i64>) -> Result<Vec<PendingDownload>> {
    let mut stmt = conn.prepare(
        "
            SELECT id, name, mimetype, SIZE, url_private_download, local_path
            FROM files
            WHERE hidden_by_limit = 0
            AND is_external = 0
            AND auto_download_paused = 0
            AND NOT EXISTS (
                SELECT 1 FROM file_download_failures f
                WHERE f.file_id = files.id AND f.preview = 0
                AND (f.permanent = 1 OR f.retry_after > unixepoch()))
            AND url_private_download IS NOT NULL
            AND (?1 IS NULL
                OR COALESCE(SIZE, 0) <= ?1)
            ORDER BY id
        ",
    )?;
    let rows = stmt
        .query_map([max_size], |r| {
            Ok(PendingDownload {
                id: r.get(0)?,
                name: r.get(1)?,
                mimetype: r.get(2)?,
                size: r.get(3)?,
                url: r.get(4)?,
                local_path: r.get(5)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

pub fn set_local_path(conn: &Connection, id: &str, path: &str) -> Result<()> {
    conn.execute(
        "
            UPDATE files
            SET local_path = ?2
            WHERE id = ?1
        ",
        params![id, path],
    )?;
    clear_failure(conn, id, false)?;
    conn.execute(
        "UPDATE files SET auto_download_paused = 0 WHERE id = ?1",
        [id],
    )?;
    Ok(())
}

/// Previews Slack rendered for files (first page of a PDF, a video frame)
/// that should be cached. `local_path` is the recorded preview path.
pub fn previews_to_fetch(conn: &Connection) -> Result<Vec<PendingDownload>> {
    let mut stmt = conn.prepare(
        "
            SELECT id, url, preview_path
            FROM (
                SELECT id, preview_path, hidden_by_limit, is_external, auto_download_paused, COALESCE(json_extract(
                        raw_json, '$.thumb_pdf'), json_extract(raw_json, '$.thumb_video')) AS
                    url
                FROM files)
            WHERE hidden_by_limit = 0
            AND is_external = 0
            AND auto_download_paused = 0
            AND NOT EXISTS (
                SELECT 1 FROM file_download_failures f
                WHERE f.file_id = id AND f.preview = 1
                AND (f.permanent = 1 OR f.retry_after > unixepoch()))
            AND url IS NOT NULL
            ORDER BY id
        ",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(PendingDownload {
                id: r.get(0)?,
                name: Some(crate::files::PREVIEW_NAME.to_owned()),
                mimetype: Some("image/png".to_owned()),
                size: None,
                url: r.get(1)?,
                local_path: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

pub fn set_preview_path(conn: &Connection, id: &str, path: &str) -> Result<()> {
    conn.execute(
        "
            UPDATE files
            SET preview_path = ?2
            WHERE id = ?1
        ",
        params![id, path],
    )?;
    clear_failure(conn, id, true)?;
    Ok(())
}

/// First transient failure waits one minute, doubling up to one day.
pub fn record_failure(conn: &Connection, id: &str, preview: bool, permanent: bool) -> Result<()> {
    conn.execute(
        "
            INSERT INTO file_download_failures(file_id, preview, failure_count,
                last_failure_at, permanent, retry_after)
            VALUES (?1, ?2, 1, unixepoch(), ?3, unixepoch() + 60)
            ON CONFLICT (file_id, preview) DO UPDATE
            SET failure_count = MIN(failure_count + 1, 31), last_failure_at = unixepoch(),
                permanent = ?3,
                retry_after = unixepoch() + MIN(86400, 60 * (1 << MIN(failure_count, 11)))
        ",
        params![id, preview, permanent],
    )?;
    Ok(())
}

fn clear_failure(conn: &Connection, id: &str, preview: bool) -> Result<()> {
    conn.execute(
        "DELETE FROM file_download_failures WHERE file_id = ?1 AND preview = ?2",
        params![id, preview],
    )?;
    Ok(())
}

/// Snapshot only the small failure table so cache hits can clear stale retries
/// without making unchanged cached files take the writer on every pass.
pub(crate) fn retrying(conn: &Connection) -> Result<std::collections::HashSet<(String, bool)>> {
    let mut stmt = conn.prepare("SELECT file_id, preview FROM file_download_failures")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}
