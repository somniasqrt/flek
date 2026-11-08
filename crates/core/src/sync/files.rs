//! Fill the local file cache.

use std::path::Path;
use std::time::Duration;

use tokio::time::Instant;

use crate::api::Client;
use crate::store::{files as file_store, Store};
use crate::{files, Result};

/// Token bucket: up to `capacity` requests at once, then one per `refill`.
/// Like a person opening a few files in a row, never a flood.
pub struct TokenBucket {
    capacity: f64,
    refill: Duration,
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    pub fn new(capacity: u32, refill: Duration) -> Self {
        Self {
            capacity: f64::from(capacity),
            refill,
            tokens: f64::from(capacity),
            last: Instant::now(),
        }
    }

    /// Downloads: bursts of 4, then 2 per second (120/minute at most).
    pub fn for_files() -> Self {
        Self::new(4, Duration::from_millis(500))
    }

    pub async fn acquire(&mut self) {
        let now = Instant::now();
        if !self.refill.is_zero() {
            let gained = now.duration_since(self.last).as_secs_f64() / self.refill.as_secs_f64();
            self.tokens = (self.tokens + gained).min(self.capacity);
        } else {
            self.tokens = self.capacity;
        }
        self.last = now;
        if self.tokens < 1.0 {
            let wait = self.refill.mul_f64(1.0 - self.tokens);
            tokio::time::sleep(wait).await;
            self.tokens = 1.0;
            self.last = Instant::now();
        }
        self.tokens -= 1.0;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct FileStats {
    pub downloaded: usize,
    pub cached: usize,
    pub failed: usize,
}

/// Downloads every Slack-hosted, visible file up to `max_size` bytes that is
/// not in the cache yet, one at a time and paced by [`TokenBucket::for_files`]. One failure does not stop the rest, except an
/// expired session. `progress(done, total)` is called after each file.
pub async fn download_files(
    client: &Client,
    store: &Store,
    cache_dir: &Path,
    max_size: Option<i64>,
    mut progress: impl FnMut(usize, usize),
) -> Result<FileStats> {
    let pending = store
        .call(move |c| file_store::downloadable(c, max_size))
        .await?;
    let total = pending.len();
    let mut stats = FileStats::default();
    let mut bucket = TokenBucket::for_files();
    for (i, f) in pending.into_iter().enumerate() {
        if !files::cache_path(cache_dir, &f.id, f.name.as_deref()).is_file() {
            bucket.acquire().await;
        }
        match files::fetch(client, cache_dir, &f).await {
            Ok((path, fresh)) => {
                if fresh {
                    stats.downloaded += 1;
                } else {
                    stats.cached += 1;
                }
                let path = path.to_string_lossy().into_owned();
                if f.local_path.as_deref() != Some(path.as_str()) {
                    let id = f.id.clone();
                    store
                        .call(move |c| file_store::set_local_path(c, &id, &path))
                        .await?;
                }
            }
            Err(e) if e.is_auth_error() => return Err(e),
            Err(e) => {
                stats.failed += 1;
                tracing::warn!(file = %f.id, error = %e, "file download failed");
            }
        }
        progress(i + 1, total);
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn bucket_allows_a_burst_then_paces() {
        let mut b = TokenBucket::new(4, Duration::from_millis(500));
        let start = Instant::now();
        for _ in 0..4 {
            b.acquire().await;
        }
        assert_eq!(start.elapsed(), Duration::ZERO);
        for _ in 0..4 {
            b.acquire().await;
        }
        assert_eq!(start.elapsed(), Duration::from_secs(2));
        // Idle long enough → full burst again.
        tokio::time::sleep(Duration::from_secs(10)).await;
        let t = Instant::now();
        for _ in 0..4 {
            b.acquire().await;
        }
        assert_eq!(t.elapsed(), Duration::ZERO);
    }
}
