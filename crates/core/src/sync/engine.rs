//! The background sync engine the app (and `flek sync`) runs; nobody has to
//! type commands.
//!
//! A pass:
//! 1. directory (users, conversations, read markers);
//! 2. fast pass: only what is new, priority conversations first; ones whose
//!    stored history already reaches Slack's newest message (`client.counts`)
//!    are skipped without a request;
//! 3. files into the cache;
//! 4. deep pass: older history and stale threads, slowly, in the background;
//! 5. on the first pass after start, at most every `refresh_every`: the
//!    refresh window (edits, reactions, deletions made while we were
//!    offline), quietly, without progress events.
//!
//! With `follow`, a pass runs after every (re)connect of the live stream and
//! events are applied in between (see [`super::follow`]). Progress goes to
//! the bus as [`Progress`].

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;

use crate::api::{AuthTest, Client};
use crate::store::{directory, messages, Store};
use crate::sync::backfill::{backfill_conversation, RateLimiter, HISTORY_INTERVAL};
use crate::sync::directory::{sync_custom_emoji, sync_directory, EMOJI_MAX_AGE};
use crate::sync::files::download_files;
use crate::sync::follow::{follow, FollowConfig};
use crate::sync::live::{Bus, Change, Progress};
use crate::sync::threads::sync_threads;
use crate::sync::SyncOptions;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct EngineOptions {
    /// Refresh window on the first pass (days, 0 = off).
    pub refresh_days: u32,
    /// Re-read the refresh window no more often than this (zero = every start).
    pub refresh_every: std::time::Duration,
    pub full: bool,
    /// Download files: cache directory and size cap in bytes.
    pub files: Option<(PathBuf, Option<i64>)>,
    /// Limit to these conversation ids (CLI `--channel`).
    pub only: Option<Vec<String>>,
    /// Keep a live connection after the first pass.
    pub follow: bool,
    pub follow_config: FollowConfig,
    /// Spacing of history/replies requests (tests set zero).
    pub history_interval: std::time::Duration,
    /// Image proxy for link previews (tests point it at a mock server).
    pub image_proxy: Option<String>,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            refresh_days: 7,
            refresh_every: std::time::Duration::ZERO,
            full: false,
            files: None,
            only: None,
            follow: false,
            follow_config: FollowConfig::default(),
            history_interval: HISTORY_INTERVAL,
            image_proxy: None,
        }
    }
}

/// Which walk over the conversations: decides the progress shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Walk {
    Fast,
    Deep,
    /// The refresh window: no progress events, so nothing looks busy.
    Quiet,
}

/// Totals of one pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct PassStats {
    pub conversations: usize,
    pub messages: usize,
    pub deleted: usize,
    pub failed: usize,
    pub files_downloaded: usize,
}

/// Shared handle to push conversations to the front of the queue (the one
/// the user opened).
#[derive(Clone, Default)]
pub struct Priority(Arc<Mutex<Vec<String>>>);

impl Priority {
    pub fn push(&self, conversation_id: &str) {
        if let Ok(mut q) = self.0.lock() {
            q.retain(|c| c != conversation_id);
            q.push(conversation_id.to_owned());
        }
    }

    /// Most recently pushed conversation still pending. Pushes are kept, so
    /// an opened conversation also leads the deep pass (where its older
    /// history comes from).
    fn take_next(&self, pending: &HashSet<String>) -> Option<String> {
        let q = self.0.lock().ok()?;
        q.iter().rev().find(|id| pending.contains(*id)).cloned()
    }
}

pub struct Engine {
    pub client: Client,
    pub store: Store,
    pub me: AuthTest,
    pub bus: Bus,
    pub priority: Priority,
    pub options: EngineOptions,
}

impl Engine {
    /// One pass, or with `follow` a live loop until `shutdown` or revoked
    /// session. Returns the totals of the first pass.
    pub async fn run(&self, shutdown: watch::Receiver<bool>) -> Result<PassStats> {
        let mut limiter = RateLimiter::new(self.options.history_interval);
        if !self.options.follow {
            return self.pass(&mut limiter, true).await;
        }
        let first = Mutex::new(None);
        let mut passes = 0u32;
        follow(
            &self.client,
            &self.store,
            &self.me,
            &self.bus,
            &self.options.follow_config,
            async || {
                passes += 1;
                let stats = self.pass(&mut limiter, passes == 1).await?;
                if let Ok(mut f) = first.lock() {
                    f.get_or_insert(stats);
                }
                Ok(())
            },
            shutdown,
        )
        .await?;
        Ok(first.into_inner().ok().flatten().unwrap_or_default())
    }

    /// Directory, fast pass, files, deep pass; `first` may add the refresh window.
    pub async fn pass(&self, limiter: &mut RateLimiter, first: bool) -> Result<PassStats> {
        let o = &self.options;
        self.progress(Progress::Directory);
        sync_directory(&self.client, &self.store, &self.me).await?;
        // Custom emoji change rarely: once per start, then daily.
        let max_age = if first {
            std::time::Duration::ZERO
        } else {
            EMOJI_MAX_AGE
        };
        sync_custom_emoji(&self.client, &self.store, &self.me.team_id, max_age).await?;
        let _ = self.bus.send(Change::Directory);

        let team = self.me.team_id.clone();
        let (mut convs, unchanged) = self
            .store
            .call(move |c| {
                Ok((
                    directory::conversation_activity(c, &team)?,
                    directory::unchanged_conversations(c, &team)?,
                ))
            })
            .await?;
        if let Some(only) = &o.only {
            convs.retain(|(id, _)| only.contains(id));
        }
        let mut stats = PassStats {
            conversations: convs.len(),
            ..Default::default()
        };

        // A full re-read walks everything; otherwise nothing new means no request.
        let fresh: Vec<_> = convs
            .iter()
            .filter(|(id, _)| (o.full && first) || !unchanged.contains(id))
            .cloned()
            .collect();
        let fast = SyncOptions {
            catch_up_only: true,
            ..Default::default()
        };
        self.walk(limiter, &fresh, &fast, Walk::Fast, &mut stats)
            .await?;

        if let Some((cache, max)) = &o.files {
            let bus = self.bus.clone();
            let f = download_files(&self.client, &self.store, cache, *max, |done, total| {
                let _ = bus.send(Change::Progress(Progress::Files { done, total }));
            })
            .await?;
            stats.files_downloaded = f.downloaded;
            let proxy = o
                .image_proxy
                .as_deref()
                .unwrap_or(crate::files::IMAGE_PROXY);
            if let Err(e) =
                crate::sync::files::download_link_images(&self.client, &self.store, cache, proxy)
                    .await
            {
                tracing::warn!(error = %e, "link preview images failed");
            }
            let team = &self.me.team_id;
            let avatars =
                crate::sync::files::download_avatars(&self.client, &self.store, cache, team)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::warn!(error = %e, "avatars failed");
                        0
                    });
            let emoji =
                crate::sync::files::download_custom_emoji(&self.client, &self.store, cache, team)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::warn!(error = %e, "custom emoji images failed");
                        0
                    });
            // Avatars and emoji are part of the directory as the UI sees it.
            if avatars + emoji > 0 {
                let _ = self.bus.send(Change::Directory);
            }
        }

        // Older history and stale threads; complete conversations cost nothing.
        let deep = SyncOptions {
            full: o.full && first,
            skip_catch_up: true,
            ..Default::default()
        };
        self.walk(limiter, &convs, &deep, Walk::Deep, &mut stats)
            .await?;
        self.progress(Progress::Idle);

        if first && o.refresh_days > 0 && self.refresh_due().await? {
            let refresh = SyncOptions {
                catch_up_only: true,
                ..SyncOptions::refresh_days(o.refresh_days)
            };
            let mut quiet = PassStats::default();
            self.walk(limiter, &convs, &refresh, Walk::Quiet, &mut quiet)
                .await?;
            stats.messages += quiet.messages;
            stats.deleted += quiet.deleted;
            stats.failed += quiet.failed;
            // A partial refresh is retried on the next start.
            if quiet.failed == 0 {
                let team = self.me.team_id.clone();
                self.store
                    .call(move |c| messages::set_refreshed(c, &team))
                    .await?;
            }
        }
        Ok(stats)
    }

    /// Whether the refresh window was last re-read `refresh_every` or longer ago.
    async fn refresh_due(&self) -> Result<bool> {
        let every = self.options.refresh_every;
        if every.is_zero() {
            return Ok(true);
        }
        let team = self.me.team_id.clone();
        let last = self
            .store
            .call(move |c| messages::refreshed_at(c, &team))
            .await?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        Ok(last.is_none_or(|t| now - t >= every.as_secs() as i64))
    }

    async fn walk(
        &self,
        limiter: &mut RateLimiter,
        convs: &[(String, String)],
        opts: &SyncOptions,
        kind: Walk,
        stats: &mut PassStats,
    ) -> Result<()> {
        let labels: std::collections::HashMap<&str, &str> = convs
            .iter()
            .map(|(i, l)| (i.as_str(), l.as_str()))
            .collect();
        let mut pending: HashSet<String> = convs.iter().map(|(id, _)| id.clone()).collect();
        let mut order = convs.iter().map(|(id, _)| id.clone());
        let total = convs.len();
        while !pending.is_empty() {
            let id = match self.priority.take_next(&pending) {
                Some(id) => id,
                None => match order.by_ref().find(|id| pending.contains(id)) {
                    Some(id) => id,
                    None => break,
                },
            };
            pending.remove(&id);
            if kind != Walk::Quiet {
                self.progress(Progress::Conversations {
                    done: total - pending.len() - 1,
                    total,
                    current: labels.get(id.as_str()).copied().unwrap_or(&id).to_owned(),
                    deep: kind == Walk::Deep,
                });
            }
            let result = async {
                let b =
                    backfill_conversation(&self.client, &self.store, limiter, &id, opts).await?;
                // Stale threads, or active in the refresh window; on a first
                // sync only roots from the pages fetched so far are known.
                let t = sync_threads(&self.client, &self.store, limiter, &id, opts).await?;
                Ok::<_, Error>((b, t))
            }
            .await;
            match result {
                Ok((b, t)) => {
                    stats.messages += b.messages + t.replies;
                    stats.deleted += b.deleted + t.deleted;
                    if b.messages + t.replies + b.deleted + t.deleted > 0 {
                        let _ = self.bus.send(Change::History {
                            conversation_id: id.clone(),
                        });
                    }
                }
                Err(e) if e.is_auth_error() => return Err(e),
                Err(e) => {
                    stats.failed += 1;
                    tracing::warn!(conversation = %id, error = %e, "sync failed");
                }
            }
        }
        Ok(())
    }

    fn progress(&self, p: Progress) {
        let _ = self.bus.send(Change::Progress(p));
    }
}
