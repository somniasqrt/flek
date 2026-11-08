//! Thread replies via `conversations.replies`.
//!
//! Roots come from backfill (`reply_count > 0`). A thread is fetched when it
//! was never fetched, its root's `latest_reply` moved past what we have, or
//! it was active inside the refresh window (an old root does not show up in
//! catch-up, so its new replies are only found this way). A fetched thread
//! is complete, so stored replies missing from it are marked deleted.
//! Broadcast replies (`thread_broadcast`) appear in both history and the
//! thread with the same ts, so the primary key dedupes them.

use std::collections::HashSet;

use crate::api::Client;
use crate::store::messages;
use crate::store::Store;
use crate::sync::backfill::{save_page, RateLimiter};
use crate::sync::SyncOptions;
use crate::Result;

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
        let mut cursor: Option<String> = None;
        let mut latest = root.clone();
        let mut seen = HashSet::new();
        loop {
            limiter.wait().await;
            let page = client
                .conversations_replies(conversation_id, &root, cursor.as_deref())
                .await?;
            if let Some(max) = page.messages.iter().map(|m| m.ts.as_str()).max() {
                latest = latest.max(max.to_owned());
            }
            stats.replies += page.messages.iter().filter(|m| m.ts != root).count();
            seen.extend(page.messages.iter().map(|m| m.ts.clone()));
            save_page(store, conversation_id, &page.messages, None).await?;
            match page.next_cursor {
                Some(next) if page.has_more => cursor = Some(next),
                _ => break,
            }
        }
        let (id, root_ts) = (conversation_id.to_owned(), root.clone());
        stats.deleted += store
            .call(move |c| {
                let gone: Vec<String> = messages::thread_replies(c, &id, &root_ts)?
                    .into_iter()
                    .filter(|ts| !seen.contains(ts))
                    .collect();
                messages::mark_thread_synced(c, &id, &root_ts, &latest)?;
                messages::mark_deleted(c, &id, &gone)
            })
            .await?;
        stats.threads += 1;
    }
    Ok(stats)
}
