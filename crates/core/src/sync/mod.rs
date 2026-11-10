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
    fresh_conversation_with_limiter(client, store, conversation_id, thread, &mut limiter).await
}

async fn fresh_conversation_with_limiter(
    client: &crate::api::Client,
    store: &crate::store::Store,
    conversation_id: &str,
    thread: Option<&str>,
    limiter: &mut backfill::RateLimiter,
) -> crate::Result<()> {
    let opts = SyncOptions {
        catch_up_only: true,
        ..Default::default()
    };
    backfill::backfill_conversation(client, store, limiter, conversation_id, &opts).await?;
    match thread {
        Some(root) => {
            threads::sync_thread(client, store, limiter, conversation_id, root).await?;
        }
        None => {
            threads::sync_threads(client, store, limiter, conversation_id, &opts).await?;
        }
    }
    Ok(())
}

pub mod channel_details;

/// Outcome of a directory refresh followed by conversation catch-up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct FreshUnreadStats {
    pub caught_up: usize,
    pub failed: usize,
}

/// Refresh the directory and catch up conversations with new or unknown remote activity.
/// Authentication failures abort immediately; other conversation failures are counted.
pub async fn fresh_unread(
    client: &crate::api::Client,
    store: &crate::store::Store,
    me: &crate::api::AuthTest,
) -> crate::Result<FreshUnreadStats> {
    directory::sync_directory(client, store, me).await?;
    let team = me.team_id.clone();
    let ids = store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT c.id FROM conversations c
                 LEFT JOIN sync_state s ON s.conversation_id = c.id
                 WHERE c.team_id = ?1 AND c.is_member = 1
                   AND (c.remote_latest IS NULL OR s.newest_ts IS NULL
                        OR c.remote_latest > s.newest_ts)
                 ORDER BY c.latest_ts DESC, c.id",
            )?;
            let ids = stmt
                .query_map([team], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(ids)
        })
        .await?;
    let mut stats = FreshUnreadStats::default();
    let mut limiter = backfill::RateLimiter::new(backfill::HISTORY_INTERVAL);
    for id in ids {
        match fresh_conversation_with_limiter(client, store, &id, None, &mut limiter).await {
            Ok(()) => stats.caught_up += 1,
            Err(error) if error.is_auth_error() => return Err(error),
            Err(error) => {
                tracing::warn!(conversation_id = %id, %error, "conversation catch-up failed");
                stats.failed += 1;
            }
        }
    }
    Ok(stats)
}

/// Refresh a referenced thread directly, or catch up a message's conversation
/// and then its thread. Resolve roots from SQLite again after catch-up, because
/// a previously absent reply/root may now be available.
pub async fn fresh_message_reference(
    client: &crate::api::Client,
    store: &crate::store::Store,
    reference: &crate::references::MessageReference,
    thread_only: bool,
) -> crate::Result<()> {
    let mut limiter = backfill::RateLimiter::new(backfill::HISTORY_INTERVAL);
    if !thread_only {
        fresh_conversation_with_limiter(
            client,
            store,
            &reference.conversation_id,
            None,
            &mut limiter,
        )
        .await?;
    }
    let (id, ts) = (reference.conversation_id.clone(), reference.ts.clone());
    let root = store
        .read(move |c| crate::reads::thread_root(c, &id, &ts))
        .await?;
    let root = if root == reference.ts {
        reference.thread_ts.as_deref().unwrap_or(&root).to_owned()
    } else {
        root
    };
    if thread_only || root != reference.ts {
        threads::sync_thread(
            client,
            store,
            &mut limiter,
            &reference.conversation_id,
            &root,
        )
        .await?;
    }
    Ok(())
}

pub mod presence;

pub mod conversation;
pub mod read;

pub mod channel_browser;
