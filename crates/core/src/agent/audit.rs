//! An IMMEDIATE transaction claims a slot across independent CLI processes.
use super::{ActionDescriptor, AgentConfig};
use crate::{Error, Result};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub struct ActionLog {
    path: PathBuf,
}
pub struct Reservation {
    path: PathBuf,
    id: i64,
}
#[derive(Serialize)]
pub struct LogEntry {
    pub id: i64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub team_id: String,
    pub action: String,
    pub targets: Vec<String>,
    /// `pending` includes interrupted processes; never retry blindly.
    pub outcome: String,
}
impl ActionLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    fn open(&self) -> Result<Connection> {
        if let Some(parent) = self.path.parent() {
            crate::private_fs::dir(parent)?;
        }
        match crate::private_fs::database(&self.path) {
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            result => result?,
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).truncate(false).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        opts.open(&self.path)?;
        let c = Connection::open(&self.path)?;
        c.busy_timeout(Duration::from_secs(5))?;
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS actions (
            id INTEGER PRIMARY KEY, started_at INTEGER NOT NULL, finished_at INTEGER,
            team_id TEXT NOT NULL, action TEXT NOT NULL, targets TEXT NOT NULL,
            outcome TEXT NOT NULL
        ); CREATE INDEX IF NOT EXISTS actions_started ON actions(started_at);",
        )?;
        crate::private_fs::database(&self.path)?;
        Ok(c)
    }
    /// Reserve immediately before session/network access. Failed attempts consume a slot.
    pub fn reserve(
        &self,
        config: &AgentConfig,
        action: &ActionDescriptor,
        yes: bool,
    ) -> Result<Reservation> {
        config.authorize(action, yes)?;
        let mut c = self.open()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now: i64 = tx.query_row("SELECT unixepoch()", [], |r| r.get(0))?;
        // Future timestamps count too: a wall-clock rollback must not grant a burst.
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM actions WHERE started_at > ?1 - 60",
            [now],
            |r| r.get(0),
        )?;
        if count >= i64::from(config.writes_per_minute) {
            return Err(Error::Slack {
                code: "rate_limited".into(),
                needed: None,
            });
        }
        tx.execute(
            "INSERT INTO actions (started_at, team_id, action, targets, outcome)
            VALUES (?1, ?2, ?3, ?4, 'pending')",
            params![
                now,
                action.team_id,
                action.action,
                serde_json::to_string(&action.targets)?
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Reservation {
            path: self.path.clone(),
            id,
        })
    }
    /// Read without creating storage or requiring credentials/configuration.
    pub fn entries(&self, limit: usize) -> Result<Vec<LogEntry>> {
        if !self.path.try_exists()? {
            return Ok(Vec::new());
        }
        let c = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        c.busy_timeout(Duration::from_secs(5))?;
        let mut stmt = c.prepare(
            "SELECT id, started_at, finished_at, team_id, action, targets, outcome
            FROM actions ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit.min(1000) as i64], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get::<_, String>(5)?,
                    r.get(6)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(id, started_at, finished_at, team_id, action, targets, outcome)| {
                    Ok(LogEntry {
                        id,
                        started_at,
                        finished_at,
                        team_id,
                        action,
                        targets: serde_json::from_str(&targets)?,
                        outcome,
                    })
                },
            )
            .collect()
    }
}
impl Reservation {
    /// Only stable categories enter the log; never exception text or request/response data.
    pub fn finish(self, result: std::result::Result<(), crate::error::ErrorKind>) -> Result<()> {
        let outcome = match result {
            Ok(()) => "accepted".to_owned(),
            Err(kind) => format!("failed:{}", kind.exit_code()),
        };
        let c = Connection::open(Path::new(&self.path))?;
        c.busy_timeout(Duration::from_secs(5))?;
        c.execute(
            "UPDATE actions SET finished_at = unixepoch(), outcome = ?2 WHERE id = ?1",
            params![self.id, outcome],
        )?;
        Ok(())
    }
}
