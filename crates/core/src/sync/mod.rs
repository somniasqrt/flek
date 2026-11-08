//! Network → database.

pub mod backfill;
pub mod directory;
pub mod files;
pub mod import;
pub mod threads;

use std::time::{SystemTime, UNIX_EPOCH};

/// Knobs shared by history and thread sync.
#[derive(Debug, Clone, Default)]
pub struct SyncOptions {
    /// Re-read history and active threads newer than this ts, to pick up
    /// edits, reactions, deletions and replies made while we were offline.
    pub refresh_since: Option<String>,
    /// Walk the whole history and every thread again, merging without loss.
    pub full: bool,
}

impl SyncOptions {
    /// Refresh window of `days` days back from now.
    pub fn refresh_days(days: u32) -> Self {
        Self {
            refresh_since: (days > 0).then(|| ts_days_ago(days)),
            full: false,
        }
    }
}

/// Slack ts for `days` days ago.
pub fn ts_days_ago(days: u32) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("{}.000000", now.saturating_sub(u64::from(days) * 86_400))
}
