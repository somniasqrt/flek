//! Keep a live connection: connect, catch up, apply events, survive drops.
//!
//! ```text
//! loop:
//!   rtm.connect + hello ──fail──► backoff (1 s … 60 s, ±50 % jitter) ─┐
//!   │ ok                                                              │
//!   ├─ catch-up (caller's closure) ┐ concurrently, so pings and       │
//!   ├─ read events → apply → bus   ┘ events are handled meanwhile     │
//!   ├─ every 30 s: ping; nothing heard for 75 s → dead                │
//!   └─ goodbye / close / dead / error ────────────────────────────────┘
//! ```
//! Only an auth error (session revoked) or shutdown ends the loop.

use std::collections::VecDeque;
use std::pin::pin;
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::{interval, Instant, MissedTickBehavior};

use crate::api::{AuthTest, Client};
use crate::rtm::{Connection, Event};
use crate::store::Store;
use crate::sync::directory::DirectoryRefresh;
use crate::sync::live::{apply, Applied, Bus, Change, ConnectionState};
use crate::Result;

#[derive(Debug, Clone)]
pub struct FollowConfig {
    /// UI-selected people. None keeps CLI follow free of presence requests.
    pub presence: Option<watch::Receiver<Vec<String>>>,
    pub ping_every: Duration,
    /// No frame at all for this long → the connection is dead.
    pub dead_after: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
}

impl Default for FollowConfig {
    fn default() -> Self {
        Self {
            presence: None,
            ping_every: Duration::from_secs(30),
            dead_after: Duration::from_secs(75),
            backoff_min: Duration::from_secs(1),
            backoff_max: Duration::from_secs(60),
        }
    }
}

/// Exponential backoff with ±50 % jitter.
#[derive(Debug, Clone)]
pub struct Backoff {
    min: Duration,
    max: Duration,
    next: Duration,
}

impl Backoff {
    pub fn new(min: Duration, max: Duration) -> Self {
        Self {
            min,
            max,
            next: min,
        }
    }

    pub fn next_delay(&mut self) -> Duration {
        let base = self.next;
        self.next = (self.next * 2).min(self.max);
        base.mul_f64(0.5 + fastrand::f64()).min(self.max)
    }

    pub fn reset(&mut self) {
        self.next = self.min;
    }
}

enum End {
    Shutdown,
    Reconnect(&'static str),
}

/// Runs until `shutdown` turns true or the session is revoked.
/// `catch_up` runs after every (re)connect, alongside the event stream.
pub async fn follow(
    client: &Client,
    store: &Store,
    me: &AuthTest,
    bus: &Bus,
    cfg: &FollowConfig,
    mut catch_up: impl AsyncFnMut() -> Result<()>,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    follow_with_directory(
        client,
        store,
        me,
        bus,
        cfg,
        &DirectoryRefresh::default(),
        &mut catch_up,
        shutdown,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn follow_with_directory(
    client: &Client,
    store: &Store,
    me: &AuthTest,
    bus: &Bus,
    cfg: &FollowConfig,
    directory: &DirectoryRefresh,
    mut catch_up: impl AsyncFnMut() -> Result<()>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let mut restart = Backoff::new(cfg.backoff_min, cfg.backoff_max);
    loop {
        if *shutdown.borrow() || shutdown.has_changed().is_err() {
            return Ok(());
        }
        let follow_shutdown = shutdown.clone();
        let result = tokio::select! {
            biased;
            _ = shutdown.changed() => return Ok(()),
            result = follow_once(client, store, me, bus, cfg, directory, &mut catch_up, follow_shutdown) => result,
        };
        match result {
            Ok(()) => return Ok(()),
            Err(e) if e.is_auth_error() => return Err(e),
            Err(e) => tracing::warn!(error = %e, "sync engine failed; restarting"),
        }
        let delay = restart.next_delay();
        reconnecting(bus, delay);
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn follow_once(
    client: &Client,
    store: &Store,
    me: &AuthTest,
    bus: &Bus,
    cfg: &FollowConfig,
    directory: &DirectoryRefresh,
    catch_up: &mut impl AsyncFnMut() -> Result<()>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    super::presence::stale(store, bus).await?;
    let mut backoff = Backoff::new(cfg.backoff_min, cfg.backoff_max);
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        match Connection::open(client).await {
            Ok(mut conn) => {
                let _ = bus.send(Change::Connection(ConnectionState::Connected));
                backoff.reset();
                let end = session(
                    &mut conn,
                    client,
                    store,
                    me,
                    bus,
                    cfg,
                    directory,
                    catch_up,
                    &mut shutdown,
                )
                .await;
                if end.as_ref().is_err_and(|e| e.is_auth_error()) {
                    return end.map(|_| ());
                }
                super::presence::stale(store, bus).await?;
                match end {
                    Ok(End::Shutdown) => {
                        conn.close().await;
                        return Ok(());
                    }
                    Ok(End::Reconnect(why)) => tracing::info!(why, "reconnecting"),
                    Err(e) if e.is_auth_error() => return Err(e),
                    Err(e) => tracing::warn!(error = %e, "live connection failed"),
                }
            }
            Err(e) if e.is_auth_error() => return Err(e),
            Err(e) => tracing::warn!(error = %e, "cannot connect"),
        }
        let delay = backoff.next_delay();
        reconnecting(bus, delay);
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = shutdown.changed() => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn session(
    conn: &mut Connection,
    client: &Client,
    store: &Store,
    me: &AuthTest,
    bus: &Bus,
    cfg: &FollowConfig,
    directory: &DirectoryRefresh,
    catch_up: &mut impl AsyncFnMut() -> Result<()>,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<End> {
    let mut presence_rx = cfg.presence.clone();
    let mut ids = presence_rx
        .as_mut()
        .map(|rx| rx.borrow_and_update().clone())
        .unwrap_or_default();
    if presence_rx.is_some() {
        if !ids.contains(&me.user_id) {
            ids.push(me.user_id.clone());
        }
        conn.subscribe_presence(&ids).await?;
    }
    let mut snapshot = Box::pin(super::presence::snapshot(
        client,
        store,
        bus,
        &me.user_id,
        ids.clone(),
    ));
    let mut snapshot_pending = presence_rx.is_some();
    let mut catching_up = pin!(catch_up_with_retry(catch_up, bus, cfg));
    let mut caught_up = false;
    let mut ticker = interval(cfg.ping_every);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await;
    let mut last_heard = Instant::now();
    let mut directory_refresh = Box::pin(directory.refresh(client, store, me, true));
    let mut directory_pending = false;
    let mut deferred = VecDeque::new();
    let mut replay_directory = false;
    let mut replay = None;

    loop {
        if replay_directory && replay.is_none() {
            replay = deferred
                .pop_front()
                .map(|event| Box::pin(apply(store, event)));
        }
        // Biased: shutdown first, then catch-up starts before any event is
        // handled, so even a short-lived connection begins catching up.
        tokio::select! {
            biased;
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return Ok(End::Shutdown);
                }
            }
            r = &mut catching_up, if !caught_up => {
                caught_up = true;
                match r {
                    Ok(()) => tracing::info!("caught up"),
                    Err(e) if e.is_auth_error() => return Err(e),
                    Err(e) => return Err(e),
                }
            }
            result = &mut directory_refresh, if directory_pending => {
                directory_pending = false;
                match result {
                    Ok(_) => {
                        let _ = bus.send(Change::Directory);
                        replay_directory = true;
                    }
                    Err(e) if e.is_auth_error() => return Err(e),
                    Err(e) => {
                        tracing::warn!(error = %e, "directory refresh failed; retrying");
                        directory_refresh = Box::pin(directory.refresh(client, store, me, true));
                        directory_pending = true;
                    }
                }
            }
            result = &mut snapshot, if snapshot_pending => {
                snapshot_pending = false;
                if let Err(e) = result {
                    if e.is_auth_error() {
                        return Err(e);
                    }
                    tracing::debug!(error = %e, "presence snapshot unavailable");
                }
            }
            changed = async {
                match &mut presence_rx {
                    Some(rx) => rx.changed().await,
                    None => std::future::pending().await,
                }
            } => {
                if changed.is_err() {
                    presence_rx = None;
                    continue;
                }
                let next = presence_rx.as_mut().unwrap().borrow_and_update().clone();
                ids = update_presence_subscription(conn, store, bus, &me.user_id, &ids, next).await?;
                snapshot = Box::pin(super::presence::snapshot(
                    client,
                    store,
                    bus,
                    &me.user_id,
                    ids.clone(),
                ));
                snapshot_pending = true;
            }
            frame = conn.next_event() => {
                let Some(v) = frame? else {
                    return Ok(End::Reconnect("closed by server"));
                };
                last_heard = Instant::now();
                let mut event = Event::parse(v);
                match &mut event {
                    Event::ManualPresenceChanged { user, .. }
                    | Event::DndUpdated { user, .. } if user.is_none() => {
                        *user = Some(me.user_id.clone());
                    }
                    _ => {}
                }
                if matches!(event, Event::Goodbye) {
                    return Ok(End::Reconnect("goodbye"));
                }
                match apply(store, event.clone()).await? {
                    Applied::Changed(change) => {
                        let _ = bus.send(change);
                    }
                    Applied::NeedsDirectory => {
                        deferred.push_back(event);
                        if !directory_pending {
                            replay_directory = false;
                            directory_refresh = Box::pin(directory.refresh(client, store, me, true));
                            directory_pending = true;
                        }
                    }
                    Applied::DirectoryStale => {
                        if !directory_pending {
                            replay_directory = false;
                            directory_refresh = Box::pin(directory.refresh(client, store, me, true));
                            directory_pending = true;
                        }
                    }
                    Applied::Ignored => {}
                }
            }
            _ = ticker.tick() => {
                if last_heard.elapsed() > cfg.dead_after {
                    return Ok(End::Reconnect("no frames, connection is dead"));
                }
                conn.ping().await?;
            }
            result = async { replay.as_mut().expect("pending replay").await }, if replay.is_some() => {
                replay = None;
                if let Applied::Changed(change) = result? {
                    let _ = bus.send(change);
                }
            }
        }
    }
}

fn reconnecting(bus: &Bus, delay: Duration) {
    let _ = bus.send(Change::Connection(ConnectionState::Reconnecting {
        in_secs: delay.as_secs(),
    }));
}

/// Kept inside the session's select so events and heartbeats continue during
/// a failed pass and its backoff. Dropping the session cancels the retry.
async fn catch_up_with_retry(
    catch_up: &mut impl AsyncFnMut() -> Result<()>,
    bus: &Bus,
    cfg: &FollowConfig,
) -> Result<()> {
    // A conversation may keep failing for hours; each retry is a whole pass
    // (directory included), so it backs off far beyond the reconnect delays.
    let mut retry = Backoff::new(cfg.backoff_max, cfg.backoff_max * 30);
    let mut failed = false;
    loop {
        match catch_up().await {
            Ok(()) => {
                if failed {
                    let _ = bus.send(Change::Connection(ConnectionState::Connected));
                }
                return Ok(());
            }
            Err(e) if e.is_auth_error() => return Err(e),
            Err(e) => tracing::warn!(error = %e, "catch-up failed; retrying"),
        }
        failed = true;
        let delay = retry.next_delay();
        let _ = bus.send(Change::Connection(ConnectionState::SyncIncomplete {
            in_secs: delay.as_secs(),
        }));
        tokio::time::sleep(delay).await;
    }
}

async fn update_presence_subscription(
    conn: &mut Connection,
    store: &Store,
    bus: &Bus,
    me: &str,
    ids: &[String],
    mut next: Vec<String>,
) -> Result<Vec<String>> {
    if !next.iter().any(|id| id == me) {
        next.push(me.to_owned());
    }
    let added: Vec<_> = next
        .iter()
        .filter(|id| !ids.contains(id))
        .cloned()
        .collect();
    store
        .call(move |c| crate::store::presence::stale_users(c, &added))
        .await?;
    super::presence::notify(bus);
    conn.subscribe_presence(&next).await?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_with_jitter_and_resets() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(60));
        let first = b.next_delay();
        assert!(first >= Duration::from_millis(500) && first <= Duration::from_millis(1500));
        for _ in 0..10 {
            assert!(b.next_delay() <= Duration::from_secs(60));
        }
        b.reset();
        assert!(b.next_delay() <= Duration::from_millis(1500));
    }
}
