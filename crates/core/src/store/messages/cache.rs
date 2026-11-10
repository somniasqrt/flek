//! Message cache operations.

use crate::Result;
use rusqlite::{Connection, OptionalExtension};

/// What the cache holds for a file: the file itself and its preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedFile {
    pub local_path: Option<String>,
    pub preview_path: Option<String>,
    pub mimetype: Option<String>,
}

pub fn cached_file(conn: &Connection, file_id: &str) -> Result<Option<CachedFile>> {
    Ok(conn
        .query_row(
            "
                SELECT local_path, preview_path, mimetype
                FROM files
                WHERE id = ?1
            ",
            [file_id],
            |r| {
                Ok(CachedFile {
                    local_path: r.get(0)?,
                    preview_path: r.get(1)?,
                    mimetype: r.get(2)?,
                })
            },
        )
        .optional()?)
}

/// Local path of a cached file, if downloaded.
pub fn file_local_path(conn: &Connection, file_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "
                SELECT local_path
                FROM files
                WHERE id = ?1
            ",
            [file_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
}
