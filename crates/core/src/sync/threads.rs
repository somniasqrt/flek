//! Thread replies via `conversations.replies`.
//!
//! Roots come from backfill (`reply_count > 0`). A thread is fetched when it
//! was never fetched or its root's `latest_reply` moved past what we have.
//! Broadcast replies (`thread_broadcast`) appear in both history and the
//! thread with the same ts, so the primary key dedupes them.

use crate::api::Client;
use crate::store::messages;
use crate::store::Store;
use crate::sync::backfill::{save_page, RateLimiter};
use crate::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct ThreadStats {
    pub threads: usize,
    pub replies: usize,
}

/// Fetches every stale thread of one conversation.
pub async fn sync_threads(
    client: &Client,
    store: &Store,
    limiter: &mut RateLimiter,
    conversation_id: &str,
) -> Result<ThreadStats> {
    let id = conversation_id.to_owned();
    let roots = store.call(move |c| messages::stale_threads(c, &id)).await?;
    let mut stats = ThreadStats::default();

    for root in roots {
        let mut cursor: Option<String> = None;
        let mut latest = root.clone();
        loop {
            limiter.wait().await;
            let page = client
                .conversations_replies(conversation_id, &root, cursor.as_deref())
                .await?;
            if let Some(max) = page.messages.iter().map(|m| m.ts.as_str()).max() {
                latest = latest.max(max.to_owned());
            }
            stats.replies += page.messages.iter().filter(|m| m.ts != root).count();
            save_page(store, conversation_id, &page.messages, None).await?;
            match page.next_cursor {
                Some(next) if page.has_more => cursor = Some(next),
                _ => break,
            }
        }
        let (id, root_ts) = (conversation_id.to_owned(), root.clone());
        store
            .call(move |c| messages::mark_thread_synced(c, &id, &root_ts, &latest))
            .await?;
        stats.threads += 1;
    }
    Ok(stats)
}
