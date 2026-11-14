//! Authenticated download of Slack-hosted files into the local cache.

use std::path::{Path, PathBuf};

mod safety;
pub use safety::{
    can_open, can_read_document, display_name, inside_cache, quarantine, save_copy, viewer_kind,
    ViewerKind,
};

use crate::api::Client;
use crate::store::files::PendingDownload;
use crate::{Error, Result};

/// `<cache dir>/flek/files`, where the cache dir is `~/Library/Caches` (macOS),
/// `%LOCALAPPDATA%` (Windows) or `$XDG_CACHE_HOME` / `~/.cache` (Linux).
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
    let name = if name.eq_ignore_ascii_case(PREVIEW_NAME) {
        format!("attachment{name}")
    } else {
        name
    };
    cache_dir.join(dir).join(name)
}

/// The per-id directory name. Slack ids are plain (`F0…`); anything that
/// sanitizes to nothing (`""`, `.`, `..`) gets a hex name, so it can neither
/// leave the cache nor share a directory with another file.
fn id_component(id: &str) -> String {
    let clean = sanitize(id);
    if !clean.is_empty()
        && !clean.eq_ignore_ascii_case("_links")
        && !clean.to_ascii_lowercase().starts_with("_id_")
    {
        return clean;
    }
    let hex: String = id.bytes().map(|b| format!("{b:02x}")).collect();
    format!("_id_{hex}")
}

/// Reserved preview slot; attachment names never resolve here.
pub fn preview_path(cache_dir: &Path, id: &str) -> PathBuf {
    cache_dir.join(id_component(id)).join(PREVIEW_NAME)
}

fn sanitize(name: &str) -> String {
    let cleaned: String = display_name(name)
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
    fetch_bounded(
        client,
        cache_dir,
        f,
        crate::settings::Settings::default().max_file_bytes,
    )
    .await
}

pub async fn fetch_bounded(
    client: &Client,
    cache_dir: &Path,
    f: &PendingDownload,
    max_bytes: Option<i64>,
) -> Result<(PathBuf, bool)> {
    let dest = cache_path(cache_dir, &f.id, f.name.as_deref());
    fetch_to(client, cache_dir, f, max_bytes, dest).await
}

pub(crate) async fn fetch_preview(
    client: &Client,
    cache_dir: &Path,
    f: &PendingDownload,
    max_bytes: Option<i64>,
) -> Result<(PathBuf, bool)> {
    let (path, fresh) = fetch_to(
        client,
        cache_dir,
        f,
        max_bytes,
        preview_path(cache_dir, &f.id),
    )
    .await?;
    // Slack's thumbnails are PNG or JPEG; anything else is never kept or served.
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(&path).await?;
    let mut signature = [0; 4];
    let image = file.read_exact(&mut signature).await.is_ok() && preview_type(&signature).is_some();
    drop(file);
    if !image {
        tokio::fs::remove_file(&path).await?;
        return Err(Error::File(NOT_A_PREVIEW.into()));
    }
    Ok((path, fresh))
}

async fn fetch_to(
    client: &Client,
    cache_dir: &Path,
    f: &PendingDownload,
    max_bytes: Option<i64>,
    dest: PathBuf,
) -> Result<(PathBuf, bool)> {
    crate::private_fs::dir(cache_dir)?;
    crate::private_fs::dir(dest.parent().expect("cache file has parent"))?;
    if dest.is_file() {
        return Ok((dest, false));
    }
    if max_bytes == Some(0) {
        return Err(Error::File(
            "download byte limit is zero (downloads disabled)".into(),
        ));
    }
    let html_ok = f
        .mimetype
        .as_deref()
        .is_some_and(|m| m.starts_with("text/html"));
    let declared = f
        .size
        .and_then(|n| u64::try_from(n).ok())
        .map(|n| n.saturating_add(64 * 1024));
    let configured = max_bytes.and_then(|n| u64::try_from(n).ok());
    let limit = match (declared, configured) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    client
        .download_bounded(&f.url, &dest, html_ok, limit)
        .await?;
    Ok((dest, true))
}

const NOT_A_PREVIEW: &str = "preview is not a PNG or JPEG image";

/// Media type of a cached preview from its first bytes, so a JPEG thumbnail is
/// never labelled PNG. The image decoder validates the rest of the body.
pub fn preview_type(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0xff, 0xd8, 0xff, ..] => Some("image/jpeg"),
        _ => None,
    }
}

/// Inline SVG fallback bytes, never an active document or a network fetch.
/// Both metadata and the stream enforce the tighter inline-preview budget.
pub async fn read_inline_svg(path: &Path, mime: Option<&str>) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    const MAX_INLINE_SVG_BYTES: u64 = 1024 * 1024;
    if !matches!(viewer_kind(path, mime), Some(ViewerKind::Svg)) {
        return Err(Error::File("unsupported inline preview type".into()));
    }
    let file = tokio::fs::File::open(path).await?;
    if file.metadata().await?.len() > MAX_INLINE_SVG_BYTES {
        return Err(Error::File("inline_preview_too_large".into()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_INLINE_SVG_BYTES + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 > MAX_INLINE_SVG_BYTES {
        return Err(Error::File("inline_preview_too_large".into()));
    }
    Ok(bytes)
}

/// Image proxy Slack's own clients load link-preview images through, so the
/// linked sites never see the user's address.
pub const IMAGE_PROXY: &str = "https://slack-imgs.com/";
/// Largest link-preview image to keep.
const MAX_LINK_IMAGE: u64 = 5 * 1024 * 1024;

/// Where a link-preview image lives: `<cache>/_links/<sha1 of the url>`.
pub fn link_image_path(cache_dir: &Path, url: &str) -> PathBuf {
    cache_dir
        .join("_links")
        .join(crate::store::unfurls::url_hash(url))
}

/// A reusable session-free client whose resolver only connects to public IPs.
pub struct ImageClient(reqwest::Client);

impl ImageClient {
    pub fn new(user_agent: &str) -> Result<Self> {
        crate::net_policy::image_http(user_agent).map(Self)
    }
}

/// Fetches an image through the proxy or directly from an allowlisted HTTPS CDN.
pub async fn fetch_link_image(
    http: &ImageClient,
    proxy: Option<&str>,
    cache_dir: &Path,
    url: &str,
) -> Result<PathBuf> {
    crate::private_fs::dir(cache_dir)?;
    let dest = link_image_path(cache_dir, url);
    crate::private_fs::dir(dest.parent().expect("image cache has parent"))?;
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
    crate::net_policy::public_image(&target)?;
    let resp = http.0.get(target).send().await?;
    store_image_response(resp, &dest).await?;
    Ok(dest)
}

async fn store_image_response(resp: reqwest::Response, dest: &Path) -> Result<()> {
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
    write_response(resp, dest, Some(MAX_LINK_IMAGE)).await?;
    Ok(())
}

/// Bounded, atomic, private writes. TempPath removes partial data on errors and cancellation.
pub(crate) async fn write_response(
    mut resp: reqwest::Response,
    dest: &Path,
    limit: Option<u64>,
) -> Result<u64> {
    if resp
        .content_length()
        .zip(limit)
        .is_some_and(|(size, max)| size > max)
    {
        return Err(Error::File(
            "download exceeds byte limit (Content-Length)".into(),
        ));
    }
    let dir = dest
        .parent()
        .ok_or_else(|| Error::File("destination has no parent".into()))?;
    crate::private_fs::dir(dir)?;
    let temp = tempfile::NamedTempFile::new_in(dir)?;
    crate::private_fs::mode(temp.path(), 0o600)?;
    let (file, path) = temp.into_parts();
    let mut out = tokio::fs::File::from_std(file);
    let mut written = 0u64;
    while let Some(chunk) = resp.chunk().await? {
        let next = written.saturating_add(chunk.len() as u64);
        if limit.is_some_and(|max| next > max) {
            return Err(Error::File(
                "download exceeds byte limit while streaming".into(),
            ));
        }
        tokio::io::AsyncWriteExt::write_all(&mut out, &chunk).await?;
        written = next;
    }
    tokio::io::AsyncWriteExt::flush(&mut out).await?;
    drop(out);
    quarantine(&path)?;
    path.persist(dest).map_err(|e| Error::Io(e.error))?;
    Ok(written)
}

/// Repair only this app's cache, including files left by older versions.
pub fn prepare_cache(cache_dir: &Path) -> Result<()> {
    // The standard layout has an app-owned `flek` parent above `files`.
    if cache_dir.file_name().is_some_and(|n| n == "files") {
        if let Some(parent) = cache_dir
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "flek"))
        {
            crate::private_fs::dir(parent)?;
        }
    }
    crate::private_fs::dir(cache_dir)?;
    static REPAIRED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::OnceLock::new();
    let mut repaired = REPAIRED
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| Error::File("cache repair lock poisoned".into()))?;
    let key = cache_dir.canonicalize()?;
    if !repaired.contains(&key) {
        crate::private_fs::tree(cache_dir)?;
        repaired.insert(key);
    }
    Ok(())
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
                "
                    SELECT name, local_path
                    FROM files
                    WHERE id = ?1
                ",
                [file_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .await?;
    if let Some(path) = recorded.map(PathBuf::from).filter(|p| p.is_file()) {
        return inside_cache(cache_dir, &path)
            .map(Some)
            .ok_or_else(|| Error::File("the file is outside the cache".into()));
    }
    let path = cache_path(cache_dir, id, name.as_deref());
    if !path.is_file() {
        return Ok(None);
    }
    let path = inside_cache(cache_dir, &path)
        .ok_or_else(|| Error::File("the file is outside the cache".into()))?;
    let (file_id, local_path) = (id.to_owned(), path.to_string_lossy().into_owned());
    store
        .call(move |c| crate::store::files::set_local_path(c, &file_id, &local_path))
        .await?;
    Ok(Some(path))
}

/// Resolve a cached attachment for an OS action. DB paths are untrusted and the
/// launch decision uses the resolved file's extension, never its display title.
pub async fn cached_open_target(
    store: &crate::store::Store,
    cache_dir: &Path,
    id: &str,
) -> Result<(PathBuf, bool)> {
    let id = id.to_owned();
    let file = store
        .read(move |c| crate::store::messages::cached_file(c, &id))
        .await?
        .ok_or_else(|| Error::File("unknown file".into()))?;
    let path = file
        .local_path
        .ok_or_else(|| Error::File("the file is not downloaded yet".into()))?;
    let path = inside_cache(cache_dir, Path::new(&path))
        .ok_or_else(|| Error::File("the file is outside the cache".into()))?;
    let allowed = can_open(&path, file.mimetype.as_deref());
    Ok((path, allowed))
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
    // A user action bypasses automatic eligibility/backoff and settings limits.
    // Known sizes retain the declared-size + slack bound in fetch_bounded.
    let max = if pending.size.is_some_and(|n| n >= 0) {
        None
    } else {
        crate::settings::Settings::default().max_file_bytes
    };
    let result = fetch_bounded(client, cache_dir, &pending, max).await;
    let (path, _) = match result {
        Ok(result) => result,
        Err(error) => {
            let file_id = id.to_owned();
            let permanent = permanent_file_failure(&error);
            store
                .call(move |c| crate::store::files::record_failure(c, &file_id, false, permanent))
                .await?;
            return Err(error);
        }
    };
    let path = inside_cache(cache_dir, &path)
        .ok_or_else(|| Error::File("the file is outside the cache".into()))?;
    let (file_id, local_path) = (id.to_owned(), path.to_string_lossy().into_owned());
    store
        .call(move |c| crate::store::files::set_local_path(c, &file_id, &local_path))
        .await?;
    Ok(path)
}

/// Definitive server refusals and unsupported preview formats stop automatic retries.
pub fn permanent_file_failure(error: &Error) -> bool {
    matches!(error, Error::HttpStatus(403 | 404 | 410))
        || matches!(error, Error::File(message) if message == NOT_A_PREVIEW)
}

/// Policy/format/size refusals are definitive; I/O and transport errors are not.
pub fn permanent_image_failure(error: &Error) -> bool {
    match error {
        Error::HttpStatus(300..=399 | 404 | 410) => true,
        Error::File(message) => matches!(
            message.as_str(),
            "bad proxy URL"
                | "bad image URL"
                | "public image requires a public HTTPS address"
                | "link preview is not an image"
                | "download exceeds byte limit (Content-Length)"
                | "download exceeds byte limit while streaming"
        ),
        // The checked resolver refuses mixed/private DNS answers before connecting.
        Error::Http(error) => {
            use std::error::Error as _;
            let mut source = error.source();
            while let Some(cause) = source {
                if cause.to_string() == "CDN resolved to a non-public address" {
                    return true;
                }
                source = cause.source();
            }
            false
        }
        _ => false,
    }
}

pub mod storage;

#[cfg(test)]
mod security_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Serve synthetic framing locally, below the public-image URL/DNS boundary.
    async fn serve(headers: &str, body: Vec<u8>, chunked: bool) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let headers = headers.to_owned();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nConnection: close\r\n{headers}\r\n"
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            for part in body.chunks(16 * 1024) {
                let bytes = if chunked {
                    [format!("{:x}\r\n", part.len()).as_bytes(), part, b"\r\n"].concat()
                } else {
                    part.to_vec()
                };
                if stream.write_all(&bytes).await.is_err() {
                    return;
                }
            }
            if chunked {
                let _ = stream.write_all(b"0\r\n\r\n").await;
            }
        });
        format!("http://{address}/")
    }

    async fn response(headers: &str, body: Vec<u8>, chunked: bool) -> reqwest::Response {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(serve(headers, body, chunked).await)
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn images_are_bounded_before_and_during_streaming() {
        for headers in [
            "Content-Length: 5242881\r\n",
            "",
            "Content-Length: 1\r\nTransfer-Encoding: chunked\r\n",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let dest = dir.path().join("image");
            let response = response(
                headers,
                vec![0; MAX_LINK_IMAGE as usize + 1],
                headers.contains("chunked"),
            )
            .await;
            let error = store_image_response(response, &dest).await.unwrap_err();
            assert!(error.to_string().contains("byte limit"), "{error}");
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        }
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("image");
        store_image_response(
            response("Content-Length: 4\r\n", b"\x89PNG".to_vec(), false).await,
            &dest,
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"\x89PNG");
    }

    #[tokio::test]
    async fn file_stream_counter_and_transport_errors_remove_partial_data() {
        for (headers, body) in [
            ("", vec![0; 70_000]),
            ("Content-Length: 60000\r\n", vec![0; 100]),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let dest = dir.path().join("file");
            let url = serve(headers, body, false).await;
            let client = Client::with_base_url(
                crate::Secret::new("xoxc-fake"),
                crate::Secret::new("xoxd-fake"),
                &url,
            )
            .unwrap();
            assert!(client
                .download_bounded(&url, &dest, false, Some(65_536))
                .await
                .is_err());
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        }
        // The convenience API also has a cap; unlimited requires an explicit None.
        let dir = tempfile::tempdir().unwrap();
        let default_cap = crate::settings::Settings::default().max_file_bytes.unwrap();
        let url = serve(
            &format!("Content-Length: {}\r\n", default_cap + 1),
            Vec::new(),
            false,
        )
        .await;
        let client = Client::with_base_url(
            crate::Secret::new("xoxc-fake"),
            crate::Secret::new("xoxd-fake"),
            &url,
        )
        .unwrap();
        let error = client
            .download(&url, &dir.path().join("file"), false)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("byte limit"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
