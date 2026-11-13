//! Thread replies via `conversations.replies`.
//!
//! Roots come from backfill (`reply_count > 0`). A thread is fetched when it
//! was never fetched, its root's `latest_reply` moved past what we have, or
//! it was active inside the refresh window (an old root does not show up in
//! catch-up, so its new replies are only found this way). A fetched thread
//! can establish deletion only for recent replies, with a complete response
//! containing the root. Older replies may be omitted by Slack Free.
//! Broadcast replies (`thread_broadcast`) appear in both history and the
//! thread with the same ts, so the primary key dedupes them.

use std::collections::HashSet;

use crate::api::Client;
use crate::store::messages;
use crate::store::Store;
use crate::sync::backfill::{save_page, RateLimiter};
use crate::sync::{deletion_cutoff, SyncOptions};
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct ThreadStats {
    pub threads: usize,
    pub replies: usize,
    pub deleted: usize,
}

/// Fetches every stale thread of one conversation.
pub async fn sync_threads(
    client: &Client,
    store: &Store,
    limiter: &mut RateLimiter,
    conversation_id: &str,
    opts: &SyncOptions,
) -> Result<ThreadStats> {
    let (id, since, all) = (
        conversation_id.to_owned(),
        opts.refresh_since.clone(),
        opts.full,
    );
    let roots = store
        .call(move |c| messages::threads_to_fetch(c, &id, since.as_deref(), all))
        .await?;
    let mut stats = ThreadStats::default();

    for root in roots {
        let fetched = match sync_thread(client, store, limiter, conversation_id, &root).await {
            Ok(stats) => stats,
            Err(error) if error.is_auth_error() => return Err(error),
            Err(
                Error::Slack { .. }
                | Error::Http(_)
                | Error::HttpStatus(_)
                | Error::Json(_)
                | Error::IncompleteSync(_),
            ) => {
                let id = conversation_id.to_owned();
                store
                    .call(move |c| messages::mark_thread_failed(c, &id, &root))
                    .await?;
                continue;
            }
            Err(error) => return Err(error),
        };
        stats.threads += fetched.threads;
        stats.replies += fetched.replies;
        stats.deleted += fetched.deleted;
    }
    Ok(stats)
}

/// Fetch one requested thread, including every page, even if its root is absent locally.
pub async fn sync_thread(
    client: &Client,
    store: &Store,
    limiter: &mut RateLimiter,
    conversation_id: &str,
    root: &str,
) -> Result<ThreadStats> {
    let mut stats = ThreadStats::default();
    let mut cursor: Option<String> = None;
    let mut latest = root.to_owned();
    let mut seen = HashSet::new();
    loop {
        limiter.wait().await;
        let page = client
            .conversations_replies(conversation_id, root, cursor.as_deref())
            .await?;
        if let Some(max) = page.messages.iter().map(|m| m.ts.as_str()).max() {
            latest = latest.max(max.to_owned());
        }
        stats.replies += page.messages.iter().filter(|m| m.ts != root).count();
        seen.extend(page.messages.iter().map(|m| m.ts.clone()));
        save_page(store, conversation_id, &page.messages, None).await?;
        if !page.has_more {
            break;
        }
        let Some(next) = page.next_cursor.filter(|next| !next.trim().is_empty()) else {
            return Err(Error::IncompleteSync("conversations.replies"));
        };
        cursor = Some(next);
    }
    let (id, root_ts) = (conversation_id.to_owned(), root.to_owned());
    let cutoff = deletion_cutoff();
    stats.deleted += store
        .call(move |c| {
            let gone: Vec<String> = messages::thread_replies(c, &id, &root_ts)?
                .into_iter()
                .filter(|ts| seen.contains(&root_ts) && *ts > cutoff && !seen.contains(ts))
                .collect();
            messages::mark_thread_synced(c, &id, &root_ts, &latest)?;
            messages::mark_deleted(c, &id, &gone)
        })
        .await?;
    stats.threads = 1;
    Ok(stats)
}
