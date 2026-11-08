//! Authenticated download of Slack-hosted files into the local cache.

use std::path::{Path, PathBuf};

use crate::api::Client;
use crate::store::files::PendingDownload;
use crate::{Error, Result};

/// `~/Library/Caches/flek/files` on macOS.
pub fn default_cache_dir() -> Result<PathBuf> {
    let dir = dirs::cache_dir().ok_or_else(|| Error::File("cache directory is unknown".into()))?;
    Ok(dir.join("flek").join("files"))
}

/// Cache name of a file's preview image, next to the file itself.
pub const PREVIEW_NAME: &str = "_flek_preview.png";

/// Where a file lives in the cache: `<cache>/<id>/<name>`. The id directory
/// dedupes; the original name lets the OS pick the right app to open it.
pub fn cache_path(cache_dir: &Path, id: &str, name: Option<&str>) -> PathBuf {
    let name = name
        .map(sanitize)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| id.to_owned());
    cache_dir.join(sanitize(id)).join(name)
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0' | ':') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_start_matches('.').trim();
    cleaned.chars().take(200).collect()
}

/// Downloads one file unless it is already cached. Returns the cache path.
pub async fn fetch(
    client: &Client,
    cache_dir: &Path,
    f: &PendingDownload,
) -> Result<(PathBuf, bool)> {
    let dest = cache_path(cache_dir, &f.id, f.name.as_deref());
    if dest.is_file() {
        return Ok((dest, false));
    }
    let html_ok = f
        .mimetype
        .as_deref()
        .is_some_and(|m| m.starts_with("text/html"));
    client.download(&f.url, &dest, html_ok).await?;
    Ok((dest, true))
}
