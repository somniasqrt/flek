//! `flekfile://localhost/<file id>[/preview]` and `…/link/<hash>`: cached
//! files, video byte ranges and link-preview images for the UI.
//!
//! The UI names files by id, never by path. The handler looks the file up
//! in the database and serves it only from inside the download cache, and
//! only raster images, explicitly allowlisted video or a PNG preview.

use std::path::{Path, PathBuf};

use flek_core::store::{messages, unfurls};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Manager, UriSchemeContext, UriSchemeResponder, Wry};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::state::AppState;

pub const SCHEME: &str = "flekfile";

pub fn handle(
    ctx: UriSchemeContext<'_, Wry>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    let path = request.uri().path().to_owned();
    let range = request
        .headers()
        .get(header::RANGE)
        .map(|v| v.to_str().unwrap_or_default().to_owned());
    let head = request.method() == tauri::http::Method::HEAD;
    tauri::async_runtime::spawn(async move {
        responder.respond(
            serve(&app, &path, range.as_deref(), head)
                .await
                .unwrap_or_else(|status| file_response(Err(status))),
        );
    });
}

async fn serve(
    app: &tauri::AppHandle,
    path: &str,
    range: Option<&str>,
    head: bool,
) -> Result<Response<Vec<u8>>, StatusCode> {
    let mut parts = path.trim_start_matches('/').split('/');
    let id = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if id == "link" {
        let hash = parts.next().ok_or(StatusCode::BAD_REQUEST)?.to_owned();
        return serve_link_image(app, hash)
            .await
            .map(|data| file_response(Ok(data)));
    }
    let preview = match parts.next() {
        None => false,
        Some("preview") => true,
        Some(_) => return Err(StatusCode::BAD_REQUEST),
    };
    let id = id.to_owned();
    let state = app.state::<AppState>();
    let file = state
        .store
        .read(move |c| messages::cached_file(c, &id))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let (path, mime) = if preview {
        (file.preview_path, "image/png".to_owned())
    } else {
        let mime = file
            .mimetype
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if !raster_mime(&mime) && !video_mime(&mime) {
            return Err(StatusCode::FORBIDDEN);
        }
        (file.local_path, mime)
    };
    let path = path.ok_or(StatusCode::NOT_FOUND)?;
    let path = inside_cache(Path::new(&path)).ok_or(StatusCode::FORBIDDEN)?;
    if !preview && video_mime(&mime) {
        if !flek_core::files::can_open(&path, Some(&mime)) {
            return Err(StatusCode::FORBIDDEN);
        }
        return serve_video(&path, &mime, range, head).await;
    }
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let mime = if preview {
        flek_core::files::preview_type(&bytes)
            .ok_or(StatusCode::FORBIDDEN)?
            .to_owned()
    } else {
        mime
    };
    Ok(file_response(Ok((bytes, mime))))
}

/// Media types the webview can decode. The resolved filename must also pass
/// core's existing extension/MIME allowlist; arbitrary video/* is never served.
fn video_mime(mime: &str) -> bool {
    matches!(
        mime,
        "video/mp4" | "video/x-m4v" | "video/quicktime" | "video/webm" | "video/ogg"
    )
}

const VIDEO_CHUNK: u64 = 8 * 1024 * 1024;
fn byte_range(range: Option<&str>, size: u64) -> Result<(u64, u64), StatusCode> {
    if size == 0 {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    let Some(range) = range else {
        return Ok((0, size - 1));
    };
    let range = range
        .strip_prefix("bytes=")
        .ok_or(StatusCode::RANGE_NOT_SATISFIABLE)?;
    let (start, end) = range
        .split_once('-')
        .ok_or(StatusCode::RANGE_NOT_SATISFIABLE)?;
    let parse = |s: &str| {
        s.parse::<u64>()
            .map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)
    };
    let (start, end) = if start.is_empty() {
        let suffix = parse(end)?;
        if suffix == 0 {
            return Err(StatusCode::RANGE_NOT_SATISFIABLE);
        }
        (size.saturating_sub(suffix), size - 1)
    } else {
        (
            parse(start)?,
            if end.is_empty() {
                size - 1
            } else {
                parse(end)?.min(size - 1)
            },
        )
    };
    if start >= size || end < start {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    Ok((start, end.min(start.saturating_add(VIDEO_CHUNK - 1))))
}

async fn serve_video(
    path: &Path,
    mime: &str,
    range: Option<&str>,
    head: bool,
) -> Result<Response<Vec<u8>>, StatusCode> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let size = file
        .metadata()
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?
        .len();
    let mut response = Response::builder()
        .header("X-Content-Type-Options", "nosniff")
        .header("Content-Security-Policy", "sandbox; default-src 'none'")
        .header(header::ACCEPT_RANGES, "bytes");
    let (start, end) = match byte_range(range, size) {
        Ok(value) => value,
        Err(status) => {
            return response
                .status(status)
                .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                .body(Vec::new())
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
    };
    let length = end - start + 1;
    // The responder owns a Vec rather than a stream. Browsers use Range for
    // large media; fail back to the file card if a client requests an entire
    // large video, rather than allocating an unbounded buffer.
    if !head && range.is_none() && length > VIDEO_CHUNK {
        return response
            .status(StatusCode::PAYLOAD_TOO_LARGE)
            .body(Vec::new())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }
    if range.is_some() {
        response = response
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    let bytes = if head {
        Vec::new()
    } else {
        file.seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;
        let mut bytes = Vec::new();
        file.take(length)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;
        bytes
    };
    response
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, length)
        .body(bytes)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn serve_link_image(
    app: &tauri::AppHandle,
    hash: String,
) -> Result<(Vec<u8>, String), StatusCode> {
    let state = app.state::<AppState>();
    let path = state
        .store
        .read(move |c| unfurls::image_path(c, &hash))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let path = inside_cache(Path::new(&path)).ok_or(StatusCode::FORBIDDEN)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let mime = sniff_image(&bytes).ok_or(StatusCode::FORBIDDEN)?;
    Ok((bytes, mime.to_owned()))
}

/// Image type from its first bytes; anything else is not served.
fn sniff_image(b: &[u8]) -> Option<&'static str> {
    Some(match b {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xff, 0xd8, 0xff, ..] => "image/jpeg",
        [b'G', b'I', b'F', b'8', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        [0, 0, 1, 0, ..] => "image/x-icon",
        _ => return None,
    })
}

/// `path` resolved, if it lies inside the download cache.
pub(crate) fn inside_cache(path: &Path) -> Option<PathBuf> {
    let cache = flek_core::files::default_cache_dir().ok()?;
    flek_core::files::inside_cache(&cache, path)
}

/// Raster branch only. Video uses a separate ranged response after the core
/// filename/MIME gate; PDFs use IPC and pdf.js canvas.
fn raster_mime(mime: &str) -> bool {
    matches!(
        mime.to_ascii_lowercase().as_str(),
        "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/avif"
            | "image/bmp"
            | "image/x-ms-bmp"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
    )
}

/// Pure response boundary: every path (including errors) gets document guards.
/// Reject active content even if another serving branch is added in the future.
fn file_response(result: Result<(Vec<u8>, String), StatusCode>) -> Response<Vec<u8>> {
    let result = result.and_then(|(bytes, mime)| {
        if raster_mime(&mime) || mime == "application/pdf" {
            Ok((bytes, mime))
        } else {
            Err(StatusCode::FORBIDDEN)
        }
    });
    let mut response = Response::builder()
        .header("X-Content-Type-Options", "nosniff")
        .header("Content-Security-Policy", "sandbox; default-src 'none'");
    let bytes = match result {
        Ok((bytes, mime)) => {
            response = response
                .header(header::CONTENT_TYPE, mime)
                .header(header::CACHE_CONTROL, "max-age=31536000, immutable");
            bytes
        }
        Err(status) => {
            response = response
                .status(status)
                .header(header::CONTENT_TYPE, "text/plain");
            Vec::new()
        }
    };
    response
        .body(bytes)
        .expect("fixed headers and validated content type")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_never_serves_active_documents_and_guards_errors_too() {
        for mime in [
            "image/svg+xml",
            "IMAGE/SVG+XML",
            "image/svg+xml; charset=utf-8",
            "text/html",
            "application/xhtml+xml",
            "text/xml",
            "application/xml",
            "image/unknown",
        ] {
            let response = file_response(Ok((
                b"<svg><script>alert(1)</script></svg>".to_vec(),
                mime.into(),
            )));
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{mime}");
            assert_eq!(response.headers()[header::CONTENT_TYPE], "text/plain");
            assert!(response.body().is_empty());
            assert_guards(&response);
        }
        assert_guards(&file_response(Err(StatusCode::NOT_FOUND)));
    }
    #[test]
    fn raster_and_pdf_response_types_and_bytes_are_unchanged() {
        for mime in ["image/png", "image/jpeg", "image/webp", "application/pdf"] {
            let response = file_response(Ok((vec![1, 2, 3], mime.into())));
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_TYPE], mime);
            assert_eq!(response.body(), &[1, 2, 3]);
            assert_guards(&response);
        }
        assert_eq!(sniff_image(b"<svg><script/></svg>"), None);
    }
    #[test]
    fn video_ranges_are_bounded_and_reject_invalid_or_multiple_ranges() {
        assert_eq!(byte_range(Some("bytes=2-5"), 10).unwrap(), (2, 5));
        assert_eq!(byte_range(Some("bytes=-3"), 10).unwrap(), (7, 9));
        assert_eq!(byte_range(Some("bytes=5-"), 10).unwrap(), (5, 9));
        assert_eq!(
            byte_range(Some("bytes=0-"), VIDEO_CHUNK * 3).unwrap(),
            (0, VIDEO_CHUNK - 1)
        );
        for range in [
            "bytes=11-",
            "bytes=8-3",
            "bytes=0-2,4-6",
            "bytes=-0",
            "other=1-2",
        ] {
            assert!(byte_range(Some(range), 10).is_err());
        }
        assert!(!video_mime("video/unknown"));
        assert!(!video_mime("text/html"));
    }
    #[tokio::test]
    async fn cached_video_supports_seek_head_and_protected_range_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.mp4");
        tokio::fs::write(&path, b"0123456789").await.unwrap();
        let response = serve_video(&path, "video/mp4", Some("bytes=3-6"), false)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 3-6/10");
        assert_eq!(response.body(), b"3456");
        assert_guards(&response);
        let head = serve_video(&path, "video/mp4", None, true).await.unwrap();
        assert_eq!(head.headers()[header::CONTENT_LENGTH], "10");
        assert!(head.body().is_empty());
        let error = serve_video(&path, "video/mp4", Some("bytes=10-"), false)
            .await
            .unwrap();
        assert_eq!(error.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(error.headers()[header::CONTENT_RANGE], "bytes */10");
        assert_guards(&error);
        let large = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .await
            .unwrap();
        large.set_len(VIDEO_CHUNK * 2).await.unwrap();
        let no_range = serve_video(&path, "video/mp4", None, false).await.unwrap();
        assert_eq!(no_range.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(no_range.body().is_empty());
        assert_guards(&no_range);
    }
    fn assert_guards(response: &Response<Vec<u8>>) {
        assert_eq!(response.headers()["X-Content-Type-Options"], "nosniff");
        assert_eq!(
            response.headers()["Content-Security-Policy"],
            "sandbox; default-src 'none'"
        );
    }
}
