//! `flekfile://localhost/<file id>[/preview]` and `…/link/<hash>`: cached
//! files and link-preview images for the UI.
//!
//! The UI names files by id, never by path. The handler looks the file up
//! in the database and serves it only from inside the download cache, and
//! only images and PDFs (for the viewer) or a preview (a PNG Slack rendered).

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
        let response = match serve(&app, &path).await {
            Ok((bytes, mime)) => Response::builder()
                .header(header::CONTENT_TYPE, mime)
                .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
                .body(bytes),
            Err(status) => Response::builder().status(status).body(Vec::new()),
        };
        responder.respond(response.unwrap_or_else(|_| Response::new(Vec::new())));
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
        if !(mime.starts_with("image/") || mime == "application/pdf") {
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
fn inside_cache(path: &Path) -> Option<PathBuf> {
    let cache = flek_core::files::default_cache_dir()
        .ok()?
        .canonicalize()
        .ok()?;
    let path = path.canonicalize().ok()?;
    path.starts_with(&cache).then_some(path)
}
