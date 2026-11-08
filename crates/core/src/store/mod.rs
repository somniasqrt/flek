//! Local SQLite database: the single source of truth for the UI.

pub mod directory;
pub mod files;
pub mod messages;
pub mod migrations;
pub mod search;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::{Error, Result};

fn open_reader(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "query_only", true)?;
    Ok(conn)
}

/// `~/Library/Application Support/flek/flek.db` on macOS.
pub fn default_path() -> Result<PathBuf> {
    let dir = dirs::data_dir().ok_or_else(|| Error::Store("data directory is unknown".into()))?;
    Ok(dir.join("flek").join("flek.db"))
}

/// Idle read-only connections kept around for reuse.
const MAX_IDLE_READERS: usize = 4;

/// Shared handle to the database: one writer connection behind a mutex, plus
/// read-only connections for readers (UI, CLI), so reads never queue behind a
/// long sync transaction. WAL lets them see the last committed state while
/// the writer works. Blocking work goes through [`Store::call`] /
/// [`Store::read`], which run it on tokio's blocking pool.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
    path: Option<PathBuf>,
    /// `None` for in-memory databases: reads share the writer.
    readers: Option<Arc<Mutex<Vec<Connection>>>>,
}

impl Store {
    /// Opens (creating if needed) the database at `path` and migrates it.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn, Some(path.to_owned()))
    }

    /// Private in-memory database, for tests.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, None)
    }

    fn init(mut conn: Connection, path: Option<PathBuf>) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        // Table rebuilds in migrations must not cascade, so foreign keys are
        // checked once afterwards instead of during.
        conn.pragma_update(None, "foreign_keys", false)?;
        migrations::migrate(&mut conn)?;
        let broken: i64 =
            conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })?;
        if broken > 0 {
            return Err(Error::Store(format!(
                "{broken} foreign key violations after migration"
            )));
        }
        conn.pragma_update(None, "foreign_keys", true)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            readers: path.as_ref().map(|_| Arc::default()),
            path,
        })
    }

    /// File path, `None` for in-memory databases.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Runs `f` with the connection on the blocking thread pool.
    pub async fn call<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let mut guard = conn
                .lock()
                .map_err(|_| Error::Store("connection mutex poisoned".into()))?;
            f(&mut guard)
        })
        .await
        .map_err(|e| Error::Store(format!("database task failed: {e}")))?
    }

    /// Synchronous access, for callers already off the async runtime.
    pub fn with_conn<R>(&self, f: impl FnOnce(&mut Connection) -> Result<R>) -> Result<R> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| Error::Store("connection mutex poisoned".into()))?;
        f(&mut guard)
    }

    /// Runs a read-only `f` on a reader connection on the blocking pool.
    pub async fn read<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.with_read(f))
            .await
            .map_err(|e| Error::Store(format!("database task failed: {e}")))?
    }

    /// Synchronous [`Store::read`]. Sees committed data only; never waits for
    /// the writer.
    pub fn with_read<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let (Some(readers), Some(path)) = (&self.readers, &self.path) else {
            return self.with_conn(|c| f(c));
        };
        let idle = readers
            .lock()
            .map_err(|_| Error::Store("reader pool poisoned".into()))?
            .pop();
        let conn = match idle {
            Some(c) => c,
            None => open_reader(path)?,
        };
        let out = f(&conn);
        if let Ok(mut pool) = readers.lock() {
            if pool.len() < MAX_IDLE_READERS {
                pool.push(conn);
            }
        }
        out
    }

    /// Schema version currently applied.
    pub fn schema_version(&self) -> Result<u32> {
        self.with_conn(|c| migrations::current_version(c))
    }
}
