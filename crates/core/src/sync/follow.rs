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

use std::pin::pin;
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::{interval, Instant, MissedTickBehavior};

use crate::api::{AuthTest, Client};
use crate::rtm::{Connection, Event};
use crate::store::Store;
use crate::sync::directory::sync_directory;
use crate::sync::live::{apply, Applied, Bus, Change, ConnectionState};
use crate::Result;

#[derive(Debug, Clone)]
pub struct FollowConfig {
    pub ping_every: Duration,
    /// No frame at all for this long → the connection is dead.
    pub dead_after: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
}

impl Default for FollowConfig {
    fn default() -> Self {
        Self {
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
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
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
                    &mut catch_up,
                    &mut shutdown,
                )
                .await;
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
        let _ = bus.send(Change::Connection(ConnectionState::Reconnecting {
            in_secs: delay.as_secs(),
        }));
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
    catch_up: &mut impl AsyncFnMut() -> Result<()>,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<End> {
    let mut catching_up = pin!(catch_up());
    let mut caught_up = false;
    let mut ticker = interval(cfg.ping_every);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await;
    let mut last_heard = Instant::now();
    let mut directory_stale = false;

    loop {
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
                    // The stream still works; the next reconnect retries.
                    Err(e) => tracing::warn!(error = %e, "catch-up failed"),
                }
            }
            frame = conn.next_event() => {
                let Some(v) = frame? else { return Ok(End::Reconnect("closed by server")) };
                last_heard = Instant::now();
                let event = Event::parse(v);
                if matches!(event, Event::Goodbye) {
                    return Ok(End::Reconnect("goodbye"));
                }
                match apply(store, event.clone()).await? {
                    Applied::Changed(change) => { let _ = bus.send(change); }
                    Applied::NeedsDirectory => {
                        sync_directory(client, store, me).await?;
                        let _ = bus.send(Change::Directory);
                        if let Applied::Changed(change) = apply(store, event).await? {
                            let _ = bus.send(change);
                        }
                    }
                    Applied::DirectoryStale => directory_stale = true,
                    Applied::Ignored => {}
                }
            }
            _ = ticker.tick() => {
                if last_heard.elapsed() > cfg.dead_after {
                    return Ok(End::Reconnect("no frames, connection is dead"));
                }
                conn.ping().await?;
                if directory_stale {
                    directory_stale = false;
                    sync_directory(client, store, me).await?;
                    let _ = bus.send(Change::Directory);
                }
            }
        }
    }
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
