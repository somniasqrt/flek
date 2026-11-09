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

/// Image proxy Slack's own clients load link-preview images through, so the
/// linked sites never see the user's address.
pub const IMAGE_PROXY: &str = "https://slack-imgs.com/";
/// Largest link-preview image to keep.
const MAX_LINK_IMAGE: usize = 5 * 1024 * 1024;

/// Where a link-preview image lives: `<cache>/_links/<sha1 of the url>`.
pub fn link_image_path(cache_dir: &Path, url: &str) -> PathBuf {
    cache_dir
        .join("_links")
        .join(crate::store::unfurls::url_hash(url))
}

/// Fetches a link-preview image through [`IMAGE_PROXY`] with `http`, a
/// client that carries no session (no cookie, no token). Only images are kept.
pub async fn fetch_link_image(
    http: &reqwest::Client,
    proxy: Option<&str>,
    cache_dir: &Path,
    url: &str,
) -> Result<PathBuf> {
    let dest = link_image_path(cache_dir, url);
    if dest.is_file() {
        return Ok(dest);
    }
    let target = match proxy {
        Some(proxy) => {
            let mut proxied =
                reqwest::Url::parse(proxy).map_err(|_| Error::File("bad proxy URL".into()))?;
            proxied
                .query_pairs_mut()
                .append_pair("c", "1")
                .append_pair("o1", "ro")
                .append_pair("url", url);
            proxied
        }
        None => reqwest::Url::parse(url).map_err(|_| Error::File("bad image URL".into()))?,
    };
    let resp = http.get(target).send().await?;
    if !resp.status().is_success() {
        return Err(Error::HttpStatus(resp.status().as_u16()));
    }
    let is_image = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("image/"));
    if !is_image {
        return Err(Error::File("link preview is not an image".into()));
    }
    let bytes = resp.bytes().await?;
    if bytes.len() > MAX_LINK_IMAGE {
        return Err(Error::File("link preview image too large".into()));
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = dest.with_extension("part");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}
