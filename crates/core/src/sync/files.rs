//! Fill the local file cache.

use std::path::Path;

use crate::api::Client;
use crate::store::{files as file_store, Store};
use crate::{files, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct FileStats {
    pub downloaded: usize,
    pub cached: usize,
    pub failed: usize,
}

/// Downloads every Slack-hosted, visible file up to `max_size` bytes that is
/// not in the cache yet. One failure does not stop the rest, except an
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
    for (i, f) in pending.into_iter().enumerate() {
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
