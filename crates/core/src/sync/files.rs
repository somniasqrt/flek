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
    /// Preview images fetched (PDF first pages, video frames).
    pub previews: usize,
}

/// Downloads every Slack-hosted, visible file up to `max_size` bytes that is
/// not in the cache yet, then the previews Slack rendered for files (always
/// small), one at a time and paced by [`TokenBucket::for_files`]. One failure
/// does not stop the rest, except an expired session. `progress(done, total)`
/// is called after each item.
pub async fn download_files(
    client: &Client,
    store: &Store,
    cache_dir: &Path,
    max_size: Option<i64>,
    mut progress: impl FnMut(usize, usize),
) -> Result<FileStats> {
    let (files_list, previews) = store
        .call(move |c| {
            Ok((
                file_store::downloadable(c, max_size)?,
                file_store::previews_to_fetch(c)?,
            ))
        })
        .await?;
    let items = files_list
        .into_iter()
        .map(|f| (f, false))
        .chain(previews.into_iter().map(|f| (f, true)));
    let total = items.size_hint().0;
    let mut stats = FileStats::default();
    let mut bucket = TokenBucket::for_files();
    for (i, (f, preview)) in items.enumerate() {
        if !files::cache_path(cache_dir, &f.id, f.name.as_deref()).is_file() {
            bucket.acquire().await;
        }
        match files::fetch(client, cache_dir, &f).await {
            Ok((path, fresh)) => {
                match (fresh, preview) {
                    (true, true) => stats.previews += 1,
                    (true, false) => stats.downloaded += 1,
                    (false, false) => stats.cached += 1,
                    (false, true) => {}
                }
                let path = path.to_string_lossy().into_owned();
                if f.local_path.as_deref() != Some(path.as_str()) {
                    let id = f.id.clone();
                    store
                        .call(move |c| {
                            if preview {
                                file_store::set_preview_path(c, &id, &path)
                            } else {
                                file_store::set_local_path(c, &id, &path)
                            }
                        })
                        .await?;
                }
            }
            Err(e) if e.is_auth_error() => return Err(e),
            Err(e) => {
                stats.failed += 1;
                tracing::warn!(file = %f.id, preview, error = %e, "file download failed");
            }
        }
        progress(i + 1, total);
    }
    Ok(stats)
}

/// Link-preview images to fetch in one pass at most.
const LINK_IMAGES_PER_PASS: usize = 300;

/// Caches the images of link previews through Slack's image proxy, paced
/// like files. Uses a client without the session: these are public images.
/// A failed image is recorded and not retried. Returns how many were fetched.
pub async fn download_link_images(
    client: &Client,
    store: &Store,
    cache_dir: &Path,
    proxy: &str,
) -> Result<usize> {
    let urls = store
        .call(|c| crate::store::unfurls::images_to_fetch(c, LINK_IMAGES_PER_PASS))
        .await?;
    fetch_images(client, store, cache_dir, Some(proxy), urls).await
}

/// Downloads avatars of the people the timeline shows, straight from Slack's
/// CDN (public URLs, no session), like the Slack client does.
pub async fn download_avatars(
    client: &Client,
    store: &Store,
    cache_dir: &Path,
    team_id: &str,
) -> Result<usize> {
    let team = team_id.to_owned();
    let urls = store
        .call(move |c| crate::store::directory::avatars_to_fetch(c, &team, LINK_IMAGES_PER_PASS))
        .await?;
    fetch_images(client, store, cache_dir, None, urls).await
}

async fn fetch_images(
    client: &Client,
    store: &Store,
    cache_dir: &Path,
    proxy: Option<&str>,
    urls: Vec<String>,
) -> Result<usize> {
    if urls.is_empty() {
        return Ok(0);
    }
    let http = reqwest::Client::builder()
        .user_agent(client.profile().user_agent.clone())
        .timeout(Duration::from_secs(30))
        .build()?;
    let mut bucket = TokenBucket::for_files();
    let mut fetched = 0;
    for url in urls {
        bucket.acquire().await;
        let path = match files::fetch_link_image(&http, proxy, cache_dir, &url).await {
            Ok(p) => {
                fetched += 1;
                Some(p.to_string_lossy().into_owned())
            }
            Err(e) => {
                tracing::debug!(error = %e, "image download failed");
                None
            }
        };
        store
            .call(move |c| crate::store::unfurls::set_image(c, &url, path.as_deref()))
            .await?;
    }
    Ok(fetched)
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
