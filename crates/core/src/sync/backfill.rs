//! History backfill: catch up on new messages, then walk back to the start.
//!
//! Progress is stored in `sync_state` after every page, so an interrupted
//! run resumes where it stopped:
//! 1. catch-up: everything newer than `newest_ts` (cursor pages); `newest_ts`
//!    moves only when the pass finishes, so a gap is never marked done;
//! 2. backfill: pages below `oldest_ts` via `latest`, until Slack says
//!    there is nothing more, then `complete = 1`.
//!
//! With a refresh window the catch-up starts at `refresh_since` instead, so
//! edits and reactions in that window are re-read, and stored top-level
//! messages Slack no longer returns there are marked deleted.

use std::collections::HashSet;
use std::time::Duration;

use tokio::time::Instant;

use crate::api::{Client, HistoryQuery, Message};
use crate::store::files::FileRow;
use crate::store::messages::{self, MessageRow, SyncState};
use crate::store::Store;
use crate::sync::SyncOptions;
use crate::Result;

/// `conversations.history` / `.replies` are Tier 3 (~50 requests/minute):
/// one request every 1.2 s never goes above it, bursts included.
/// Lower bound for catching up a conversation that has no messages yet.
const EPOCH_TS: &str = "0000000000.000000";

pub const HISTORY_INTERVAL: Duration = Duration::from_millis(1200);

/// Spaces out requests: [`RateLimiter::wait`] returns no sooner than
/// `interval` after the previous call returned.
pub struct RateLimiter {
    interval: Duration,
    last: Option<Instant>,
}

impl RateLimiter {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
        }
    }

    pub async fn wait(&mut self) {
        if let Some(last) = self.last {
            tokio::time::sleep_until(last + self.interval).await;
        }
        self.last = Some(Instant::now());
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct BackfillStats {
    pub pages: usize,
    pub messages: usize,
    /// Stored messages found deleted in the refresh window.
    pub deleted: usize,
    pub complete: bool,
}

/// Brings one conversation up to date and continues its backfill.
pub async fn backfill_conversation(
    client: &Client,
    store: &Store,
    limiter: &mut RateLimiter,
    conversation_id: &str,
    opts: &SyncOptions,
) -> Result<BackfillStats> {
    let id = conversation_id.to_owned();
    let mut state = store.call(move |c| messages::sync_state(c, &id)).await?;
    let mut stats = BackfillStats::default();

    // 1. Catch-up above newest_ts (or above the refresh window).
    // A conversation that was empty when backfill completed has no newest
    // ts; catch up from the beginning so its first messages are not missed.
    let catch_up_from = state
        .newest_ts
        .clone()
        .or_else(|| state.complete.then(|| EPOCH_TS.to_owned()));
    if let Some(newest) = catch_up_from {
        let lower = match &opts.refresh_since {
            Some(since) if *since < newest => since.clone(),
            _ => newest.clone(),
        };
        let mut seen = HashSet::new();
        let mut cursor: Option<String> = None;
        let mut top = newest.clone();
        loop {
            limiter.wait().await;
            let page = client
                .conversations_history(
                    conversation_id,
                    HistoryQuery {
                        oldest: Some(&lower),
                        cursor: cursor.as_deref(),
                        ..Default::default()
                    },
                )
                .await?;
            if let Some(max) = page.messages.iter().map(|m| m.ts.as_str()).max() {
                top = top.max(max.to_owned());
            }
            seen.extend(page.messages.iter().map(|m| m.ts.clone()));
            save_page(store, conversation_id, &page.messages, None).await?;
            stats.pages += 1;
            stats.messages += page.messages.len();
            match page.next_cursor {
                Some(next) if page.has_more => cursor = Some(next),
                _ => break,
            }
        }
        if lower < newest {
            let (id, lo, hi) = (conversation_id.to_owned(), lower.clone(), newest.clone());
            stats.deleted = store
                .call(move |c| {
                    let gone: Vec<String> = messages::top_level_between(c, &id, &lo, &hi)?
                        .into_iter()
                        .filter(|ts| !seen.contains(ts))
                        .collect();
                    messages::mark_deleted(c, &id, &gone)
                })
                .await?;
        }
        state.newest_ts = Some(top);
        save_state(store, conversation_id, &state).await?;
    }

    if opts.full {
        state.oldest_ts = None;
        state.complete = false;
    }

    // 2. Backfill below oldest_ts (on a first sync this is also where the
    //    newest messages come from, so catch_up_only still takes one page).
    let mut pages_left = if opts.catch_up_only && state.newest_ts.is_some() {
        0
    } else if opts.catch_up_only {
        1
    } else {
        usize::MAX
    };
    while !state.complete && pages_left > 0 {
        pages_left -= 1;
        limiter.wait().await;
        let page = client
            .conversations_history(
                conversation_id,
                HistoryQuery {
                    latest: state.oldest_ts.as_deref(),
                    ..Default::default()
                },
            )
            .await?;
        if let Some(min) = page.messages.iter().map(|m| m.ts.as_str()).min() {
            state.oldest_ts = Some(min.to_owned());
        }
        if state.newest_ts.is_none() {
            state.newest_ts = page.messages.iter().map(|m| m.ts.clone()).max();
        }
        // An empty page means nothing older is left, whatever has_more says.
        state.complete = !page.has_more || page.messages.is_empty();
        stats.pages += 1;
        stats.messages += page.messages.len();
        save_page(store, conversation_id, &page.messages, Some(&state)).await?;
    }
    stats.complete = state.complete;
    Ok(stats)
}

pub(crate) async fn save_page(
    store: &Store,
    conversation_id: &str,
    page: &[Message],
    state: Option<&SyncState>,
) -> Result<()> {
    let rows: Vec<MessageRow> = page.iter().map(to_row).collect();
    let id = conversation_id.to_owned();
    let state = state.cloned();
    store
        .call(move |c| {
            messages::upsert_messages(c, &id, &rows)?;
            if let Some(s) = state {
                messages::set_sync_state(c, &id, &s)?;
            }
            Ok(())
        })
        .await
}

async fn save_state(store: &Store, conversation_id: &str, state: &SyncState) -> Result<()> {
    let id = conversation_id.to_owned();
    let state = state.clone();
    store
        .call(move |c| messages::set_sync_state(c, &id, &state))
        .await
}

pub(crate) fn to_row(m: &Message) -> MessageRow {
    MessageRow {
        ts: m.ts.clone(),
        thread_ts: m.thread_ts.clone(),
        user_id: m.user.clone().or_else(|| m.bot_id.clone()),
        subtype: m.subtype.clone(),
        text: m.text.clone(),
        blocks_json: m.blocks.as_ref().map(|b| b.to_string()),
        reply_count: m.reply_count,
        latest_reply: m.latest_reply.clone(),
        edited_ts: m.edited.as_ref().and_then(|e| e.ts.clone()),
        // A deleted thread root that still has replies stays as a tombstone.
        deleted: m.subtype.as_deref() == Some("tombstone"),
        hidden_by_limit: m.is_hidden_by_limit(),
        raw_json: m.raw.to_string(),
        reactions: m
            .reactions
            .iter()
            .flat_map(|r| r.users.iter().map(move |u| (r.name.clone(), u.clone())))
            .collect(),
        files: m.files.iter().filter_map(FileRow::from_slack).collect(),
        author_name: None,
    }
}
