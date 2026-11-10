//! Network → database.

pub mod backfill;
pub mod directory;
pub mod engine;
pub mod files;
pub mod follow;
pub mod import;
pub mod live;
pub mod reactions;
pub mod send;
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
    /// Only fetch what is newer than the stored history (no deep backfill);
    /// the engine's first, fast pass.
    pub catch_up_only: bool,
    /// Leave out the catch-up above the stored history (the fast pass just
    /// did it); the deep pass.
    pub skip_catch_up: bool,
}

impl SyncOptions {
    /// Refresh window of `days` days back from now.
    pub fn refresh_days(days: u32) -> Self {
        Self {
            refresh_since: (days > 0).then(|| ts_days_ago(days)),
            ..Self::default()
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

/// Catch up one conversation and its stale threads, or refresh a requested
/// thread explicitly. All output is persisted before callers read it.
pub async fn fresh_conversation(
    client: &crate::api::Client,
    store: &crate::store::Store,
    conversation_id: &str,
    thread: Option<&str>,
) -> crate::Result<()> {
    let mut limiter = backfill::RateLimiter::new(backfill::HISTORY_INTERVAL);
    let opts = SyncOptions {
        catch_up_only: true,
        ..Default::default()
    };
    backfill::backfill_conversation(client, store, &mut limiter, conversation_id, &opts).await?;
    match thread {
        Some(root) => {
            threads::sync_thread(client, store, &mut limiter, conversation_id, root).await?;
        }
        None => {
            threads::sync_threads(client, store, &mut limiter, conversation_id, &opts).await?;
        }
    }
    Ok(())
}
