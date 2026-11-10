//! Numbered SQL migrations, applied in order at startup.

use rusqlite::{params, Connection};

use crate::Result;

/// `(version, sql)`; versions must be increasing. Never edit an applied one,
/// add a new file instead.
const MIGRATIONS: &[(u32, &str)] = &[
    (1, include_str!("migrations/0001_init.sql")),
    (2, include_str!("migrations/0002_conversation_label.sql")),
    (3, include_str!("migrations/0003_threads.sql")),
    (4, include_str!("migrations/0004_author_name.sql")),
    (5, include_str!("migrations/0005_search.sql")),
    (6, include_str!("migrations/0006_unread.sql")),
    (7, include_str!("migrations/0007_counters.sql")),
    (8, include_str!("migrations/0008_previews.sql")),
    (9, include_str!("migrations/0009_link_images.sql")),
    (10, include_str!("migrations/0010_files_from_history.sql")),
    (11, include_str!("migrations/0011_custom_emoji.sql")),
    (12, include_str!("migrations/0012_sync_skip.sql")),
];

/// Latest schema version this build knows.
pub fn latest_version() -> u32 {
    MIGRATIONS.last().map_or(0, |(v, _)| *v)
}

/// Current schema version of the database (0 for a fresh one).
pub fn current_version(conn: &Connection) -> Result<u32> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version     INTEGER PRIMARY KEY,
            applied_at  INTEGER NOT NULL
        );",
    )?;
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |r| r.get(0),
    )?)
}

/// Applies every pending migration, each in its own transaction.
/// Returns the versions that were applied.
pub fn migrate(conn: &mut Connection) -> Result<Vec<u32>> {
    migrate_to(conn, latest_version())
}

/// Applies pending migrations up to `target` (tests use it to build old schemas).
pub fn migrate_to(conn: &mut Connection, target: u32) -> Result<Vec<u32>> {
    let current = current_version(conn)?;
    if current > latest_version() {
        return Err(crate::Error::Store(format!(
            "database schema v{current} is newer than this build (v{})",
            latest_version()
        )));
    }

    let mut applied = Vec::new();
    for &(version, sql) in MIGRATIONS
        .iter()
        .filter(|(v, _)| *v > current && *v <= target)
    {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (?1, unixepoch())",
            params![version],
        )?;
        tx.commit()?;
        tracing::info!(version, "applied migration");
        applied.push(version);
    }
    Ok(applied)
}
