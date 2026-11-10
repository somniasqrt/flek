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
    let dir = id_component(id);
    let name = name
        .map(sanitize)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| dir.clone());
    cache_dir.join(dir).join(name)
}

/// The per-id directory name. Slack ids are plain (`F0…`); anything that
/// sanitizes to nothing (`""`, `.`, `..`) gets a hex name, so it can neither
/// leave the cache nor share a directory with another file.
fn id_component(id: &str) -> String {
    let clean = sanitize(id);
    if !clean.is_empty() {
        return clean;
    }
    let hex: String = id.bytes().map(|b| format!("{b:02x}")).collect();
    format!("_{hex}")
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

/// Find a stored file in either its recorded path or the standard cache layout.
/// Reconcile paths before callers decide whether a session is needed.
pub async fn stored_cached_path(
    store: &crate::store::Store,
    cache_dir: &Path,
    id: &str,
) -> Result<Option<PathBuf>> {
    let file_id = id.to_owned();
    let (name, recorded): (Option<String>, Option<String>) = store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT name,local_path FROM files WHERE id=?1",
                [file_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .await?;
    if let Some(path) = recorded.map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(Some(path));
    }
    let path = cache_path(cache_dir, id, name.as_deref());
    if !path.is_file() {
        return Ok(None);
    }
    let (file_id, local_path) = (id.to_owned(), path.to_string_lossy().into_owned());
    store
        .call(move |c| crate::store::files::set_local_path(c, &file_id, &local_path))
        .await?;
    Ok(Some(path))
}

/// Download one stored file and persist the cache path before returning it.
pub async fn fetch_stored(
    client: &Client,
    store: &crate::store::Store,
    cache_dir: &Path,
    id: &str,
) -> Result<PathBuf> {
    let file_id = id.to_owned();
    let pending = store
        .read(move |c| crate::reads::pending_file(c, &file_id))
        .await?;
    let (path, _) = fetch(client, cache_dir, &pending).await?;
    let (file_id, local_path) = (id.to_owned(), path.to_string_lossy().into_owned());
    store
        .call(move |c| crate::store::files::set_local_path(c, &file_id, &local_path))
        .await?;
    Ok(path)
}

pub mod storage;
