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
    files::prepare_cache(cache_dir)?;
    if max_size == Some(0) {
        return Ok(FileStats::default());
    }
    let (files_list, previews, retrying) = store
        .read(move |c| {
            Ok((
                file_store::downloadable(c, max_size)?,
                file_store::previews_to_fetch(c)?,
                file_store::retrying(c)?,
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
        let target = if preview {
            files::preview_path(cache_dir, &f.id)
        } else {
            files::cache_path(cache_dir, &f.id, f.name.as_deref())
        };
        if !target.is_file() {
            bucket.acquire().await;
        }
        let result = if preview {
            files::fetch_preview(client, cache_dir, &f, max_size).await
        } else {
            files::fetch_bounded(client, cache_dir, &f, max_size).await
        };
        match result {
            Ok((path, fresh)) => {
                match (fresh, preview) {
                    (true, true) => stats.previews += 1,
                    (true, false) => stats.downloaded += 1,
                    (false, false) => stats.cached += 1,
                    (false, true) => {}
                }
                let path = path.to_string_lossy().into_owned();
                if fresh
                    || f.local_path.as_deref() != Some(path.as_str())
                    || retrying.contains(&(f.id.clone(), preview))
                {
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
                let id = f.id.clone();
                let permanent = files::permanent_file_failure(&e);
                store
                    .call(move |c| file_store::record_failure(c, &id, preview, permanent))
                    .await?;
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
/// Definitive refusals are remembered; transient failures back off. Returns the fetched count.
pub async fn download_link_images(
    client: &Client,
    store: &Store,
    cache_dir: &Path,
    proxy: &str,
) -> Result<usize> {
    prepare_images(store, cache_dir).await?;
    refresh_media(store).await?;
    let urls = store
        .read(|c| crate::store::unfurls::images_to_fetch(c, LINK_IMAGES_PER_PASS))
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
    prepare_images(store, cache_dir).await?;
    let team = team_id.to_owned();
    let urls = store
        .read(move |c| crate::store::directory::avatars_to_fetch(c, &team, LINK_IMAGES_PER_PASS))
        .await?;
    fetch_images(client, store, cache_dir, None, urls).await
}

/// Custom emoji images to fetch in one pass at most.
const EMOJI_PER_PASS: usize = 100;

/// Downloads images of the custom emoji stored messages and reactions use
/// (never the whole list), from Slack's emoji CDN: public URLs, no session.
pub async fn download_custom_emoji(
    client: &Client,
    store: &Store,
    cache_dir: &Path,
    team_id: &str,
) -> Result<usize> {
    prepare_images(store, cache_dir).await?;
    refresh_media(store).await?;
    let team = team_id.to_owned();
    let urls = store
        .read(move |c| crate::store::emoji::to_fetch(c, &team, EMOJI_PER_PASS))
        .await?;
    fetch_images(client, store, cache_dir, None, urls).await
}

/// The large photo of one person, for the profile panel: fetched on demand
/// straight from Slack's CDN (public URL, no session, like avatars) unless
/// cached. Returns its cache hash, or `None` when the user has no photo.
/// A failure is recorded but retried the next time the panel opens.
pub async fn fetch_profile_photo(
    user_agent: &str,
    store: &Store,
    cache_dir: &Path,
    user_id: &str,
) -> Result<Option<String>> {
    let id = user_id.to_owned();
    let Some(url) = store
        .read(move |c| crate::store::directory::profile_photo_url(c, &id))
        .await?
    else {
        return Ok(None);
    };
    let hash = crate::store::unfurls::url_hash(&url);
    let known = url.clone();
    if store
        .read(move |c| Ok(crate::store::unfurls::is_cached(c, &known)))
        .await?
    {
        return Ok(Some(hash));
    }
    let http = files::ImageClient::new(user_agent)?;
    match files::fetch_link_image(&http, None, cache_dir, &url).await {
        Ok(path) => {
            let path = path.to_string_lossy().into_owned();
            store
                .call(move |c| crate::store::unfurls::set_image(c, &url, Some(&path)))
                .await?;
            Ok(Some(hash))
        }
        Err(error) => {
            let permanent = files::permanent_image_failure(&error);
            store
                .call(move |c| crate::store::unfurls::record_image_failure(c, &url, permanent))
                .await?;
            Err(error)
        }
    }
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
    files::prepare_cache(cache_dir)?;
    let http = files::ImageClient::new(&client.profile().user_agent)?;
    let mut bucket = TokenBucket::for_files();
    let mut fetched = 0;
    for url in urls {
        bucket.acquire().await;
        match files::fetch_link_image(&http, proxy, cache_dir, &url).await {
            Ok(path) => {
                fetched += 1;
                let path = path.to_string_lossy().into_owned();
                store
                    .call(move |c| crate::store::unfurls::set_image(c, &url, Some(&path)))
                    .await?;
            }
            Err(error) => {
                tracing::debug!(error = %error, "image download failed");
                let permanent = files::permanent_image_failure(&error);
                store
                    .call(move |c| crate::store::unfurls::record_image_failure(c, &url, permanent))
                    .await?;
            }
        }
    }
    Ok(fetched)
}

/// Workspace icons share the public, session-free avatar image cache.
pub async fn download_team_icon(
    client: &Client,
    store: &Store,
    cache: &Path,
    team: &str,
) -> Result<usize> {
    prepare_images(store, cache).await?;
    let team = team.to_owned();
    let urls = store
        .read(
            move |c| match crate::store::directory::team_icon_url(c, &team)? {
                Some(url) if crate::store::unfurls::image_pending(c, &url)? => Ok(vec![url]),
                _ => Ok(Vec::new()),
            },
        )
        .await?;
    fetch_images(client, store, cache, None, urls).await
}

async fn refresh_media(store: &Store) -> Result<()> {
    let through = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT COALESCE(MAX(position), 0) FROM media_message_changes",
                [],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .await?;
    loop {
        let scanned = store
            .read(move |c| crate::store::unfurls::scan_media_through(c, through))
            .await?;
        if scanned.is_empty() {
            break;
        }
        store
            .call(move |c| crate::store::unfurls::save_media_scan(c, scanned))
            .await?;
        // Each batch releases the writer; newly changed positions are left for
        // the next refresh so an active conversation cannot cause a livelock.
        tokio::task::yield_now().await;
    }
    Ok(())
}

/// Reconcile all public paths once per store/cache per process; afterwards only
/// exact missing-path hints from the display/scheme read paths are examined.
async fn prepare_images(store: &Store, cache: &Path) -> Result<()> {
    files::prepare_cache(cache)?;
    type CacheKey = (Option<std::path::PathBuf>, std::path::PathBuf);
    static CHECKED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<CacheKey>>> =
        std::sync::OnceLock::new();
    let key = (store.path().map(Path::to_owned), cache.canonicalize()?);
    let checked = CHECKED.get_or_init(Default::default);
    let first = !checked
        .lock()
        .map_err(|_| crate::Error::File("image cache lock poisoned".into()))?
        .contains(&key);
    let cache = cache.to_owned();
    let missing = store
        .read(move |c| {
            let mut missing = crate::store::unfurls::missing_image_hints(c);
            if first {
                let mut stmt = c.prepare(
                    "SELECT hash, local_path FROM link_images WHERE local_path IS NOT NULL",
                )?;
                let paths =
                    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                for path in paths {
                    let (hash, path) = path?;
                    if files::inside_cache(&cache, Path::new(&path)).is_none() {
                        missing.push((hash, path));
                    }
                }
            }
            Ok(missing)
        })
        .await?;
    if !missing.is_empty() {
        store
            .call(move |c| crate::store::unfurls::invalidate_missing(c, &missing))
            .await?;
    }
    checked
        .lock()
        .map_err(|_| crate::Error::File("image cache lock poisoned".into()))?
        .insert(key);
    Ok(())
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
