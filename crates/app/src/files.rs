//! `flekfile://localhost/<file id>[/preview]` and `…/link/<hash>`: cached
//! files and link-preview images for the UI.
//!
//! The UI names files by id, never by path. The handler looks the file up
//! in the database and serves it only from inside the download cache, and
//! only images (a file itself) or its preview (a PNG Slack rendered).

use std::path::{Path, PathBuf};

use flek_core::store::{messages, unfurls};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Manager, UriSchemeContext, UriSchemeResponder, Wry};

use crate::state::AppState;

pub const SCHEME: &str = "flekfile";

pub fn handle(
    ctx: UriSchemeContext<'_, Wry>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    let path = request.uri().path().to_owned();
    tauri::async_runtime::spawn(async move {
        responder.respond(file_response(serve(&app, &path).await));
    });
}

async fn serve(app: &tauri::AppHandle, path: &str) -> Result<(Vec<u8>, String), StatusCode> {
    let mut parts = path.trim_start_matches('/').split('/');
    let id = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if id == "link" {
        let hash = parts.next().ok_or(StatusCode::BAD_REQUEST)?.to_owned();
        return serve_link_image(app, hash).await;
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
        let mime = file.mimetype.unwrap_or_default();
        if !raster_mime(&mime) {
            return Err(StatusCode::FORBIDDEN);
        }
        (file.local_path, mime)
    };
    let path = path.ok_or(StatusCode::NOT_FOUND)?;
    let path = inside_cache(Path::new(&path)).ok_or(StatusCode::FORBIDDEN)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok((bytes, mime))
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

/// Only known raster image types are exposed by the image protocol. PDFs use
/// pdf_data IPC and pdf.js canvas; video/PDF previews are PNGs.
fn raster_mime(mime: &str) -> bool {
    matches!(
        mime.to_ascii_lowercase().as_str(),
        "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/avif"
            | "image/bmp"
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
    fn assert_guards(response: &Response<Vec<u8>>) {
        assert_eq!(response.headers()["X-Content-Type-Options"], "nosniff");
        assert_eq!(
            response.headers()["Content-Security-Policy"],
            "sandbox; default-src 'none'"
        );
    }
}
