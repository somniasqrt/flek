//! Agent event/reply subscriptions over committed state. The durable cursor
//! survives broadcast lag and observes writes from another WAL process.

use crate::store::{events, Store};
use crate::{references, views, Error, Result};
use rusqlite::{Connection, OptionalExtension};
use std::time::Duration;

pub use events::{Event, Filter};

pub struct Stream {
    store: Store,
    team: String,
    filter: Filter,
    cursor: i64,
    replay_end: i64,
}

impl Stream {
    /// Capture the boundary before starting network work. With --since,
    /// replay that prefix first; subsequent commits are consumed by sequence.
    pub async fn open(store: Store, team: String, filter: Filter) -> Result<Self> {
        let t = team.clone();
        let replay_end = store.read(move |c| events::head(c, &t)).await?;
        let cursor = if filter.since.is_some() {
            0
        } else {
            replay_end
        };
        Ok(Self {
            store,
            team,
            filter,
            cursor,
            replay_end,
        })
    }

    pub async fn replay(&mut self) -> Result<Option<Vec<Event>>> {
        if self.cursor >= self.replay_end {
            return Ok(None);
        }
        self.batch(self.replay_end).await.map(Some)
    }

    async fn batch(&mut self, through: i64) -> Result<Vec<Event>> {
        let team = self.team.clone();
        let filter = self.filter.clone();
        let cursor = self.cursor;
        let (events, next) = self
            .store
            .read(move |c| events::page(c, &team, cursor, through, &filter, 256))
            .await?;
        self.cursor = next;
        Ok(events)
    }

    /// One non-blocking read of everything committed after the cursor (at most
    /// one page); empty means the journal is drained. Used to flush on shutdown.
    pub async fn poll(&mut self) -> Result<Vec<Event>> {
        self.batch(i64::MAX).await
    }

    pub async fn next(&mut self) -> Result<Vec<Event>> {
        loop {
            let events = self.poll().await?;
            if !events.is_empty() {
                return Ok(events);
            }
            // Local DB polling only: network ingestion remains websocket-driven.
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReplyTarget {
    pub conversation_id: String,
    pub after: String,
    /// None means any subsequent message in a DM/group DM.
    pub root: Option<String>,
    pub self_id: String,
}

impl ReplyTarget {
    pub fn resolve(c: &Connection, team: &str, reference: &str, ts: Option<&str>) -> Result<Self> {
        let r = references::message_reference(c, team, reference, ts)?;
        // Permalinks are parsed without resolving; enforce team membership here.
        let conv = references::resolve(c, team, &r.conversation_id, references::Match::Exact)?
            .required(reference)?;
        let stored_root: Option<String> = c
            .query_row(
                "SELECT thread_ts FROM messages WHERE conversation_id = ?1 AND ts = ?2",
                rusqlite::params![conv.id, r.ts],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let root = if matches!(conv.kind.as_str(), "im" | "mpim") {
            None
        } else {
            Some(stored_root.or(r.thread_ts).unwrap_or_else(|| r.ts.clone()))
        };
        Ok(Self {
            conversation_id: conv.id,
            after: crate::time::parse(&r.ts)?,
            root,
            self_id: events::self_id(c, team)?,
        })
    }

    pub async fn stored_reply(&self, store: &Store) -> Result<Option<views::Message>> {
        let target = self.clone();
        store
            .read(move |c| {
                events::reply(
                    c,
                    &target.conversation_id,
                    &target.after,
                    target.root.as_deref(),
                    &target.self_id,
                )
            })
            .await
    }

    /// Wait for the first reply from someone else: stored replies win, then
    /// live events (and the catch-up after each reconnect) are watched until
    /// `deadline` (`Error::Timeout`) or `shutdown` (`Ok(None)`).
    pub async fn wait(
        &self,
        client: &crate::api::Client,
        store: &Store,
        me: &crate::api::AuthTest,
        config: &super::follow::FollowConfig,
        deadline: Option<tokio::time::Instant>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<views::Message>> {
        if let Some(message) = self.stored_reply(store).await? {
            return Ok(Some(message));
        }
        let bus = super::live::bus();
        let (stop, stop_rx) = tokio::sync::watch::channel(false);
        let following = self.follow(client, store, me, &bus, config, stop_rx);
        tokio::pin!(following);
        let answer = async {
            loop {
                if let Some(message) = self.stored_reply(store).await? {
                    return Ok::<_, Error>(message);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        let timeout = async {
            match deadline {
                Some(deadline) => tokio::time::sleep_until(deadline).await,
                None => std::future::pending::<()>().await,
            }
        };
        let mut finished = false;
        let outcome = tokio::select! {
            biased;
            _ = shutdown.changed() => Ok(None),
            r = &mut following => {
                finished = true;
                r.map(|()| None)
            }
            r = answer => r.map(Some),
            _ = timeout => Err(Error::Timeout),
        };
        if !finished {
            // Stop the supervisor cleanly (no write is abandoned mid-way).
            let _ = stop.send(true);
            let _ = following.await;
        }
        outcome
    }

    /// Reuse the sync supervisor, RTM application, heartbeat and backoff. On
    /// every connection fetch the target thread explicitly (ordinary engine
    /// stale-thread heuristics need not include an old question).
    pub async fn follow(
        &self,
        client: &crate::api::Client,
        store: &Store,
        me: &crate::api::AuthTest,
        bus: &super::live::Bus,
        config: &super::follow::FollowConfig,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        if me.user_id != self.self_id {
            return Err(Error::Usage(
                "stored self identity differs from the active session; sync first".into(),
            ));
        }
        super::follow::follow(
            client,
            store,
            me,
            bus,
            config,
            async || {
                super::fresh_conversation(
                    client,
                    store,
                    &self.conversation_id,
                    self.root.as_deref(),
                )
                .await
            },
            shutdown,
        )
        .await
    }
}
