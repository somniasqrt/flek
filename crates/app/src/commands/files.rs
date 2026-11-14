//! Files IPC adapters.

use super::{text, CmdResult};
use crate::state::AppState;
use flek_core::store::messages;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use tokio::io::AsyncReadExt;

/// Opens an allowlisted cached file, or reveals other types. Takes the file id,
/// so the UI cannot open arbitrary files.
#[tauri::command]
pub async fn open_file(
    app: AppHandle,
    state: State<'_, AppState>,
    file_id: String,
) -> CmdResult<()> {
    let cache = flek_core::files::default_cache_dir().map_err(text)?;
    let (path, allowed) = flek_core::files::cached_open_target(&state.store, &cache, &file_id)
        .await
        .map_err(text)?;
    flek_core::files::quarantine(&path).map_err(text)?;
    if allowed {
        app.opener()
            .open_path(path.to_string_lossy().into_owned(), None::<&str>)
            .map_err(text)
    } else {
        app.opener().reveal_item_in_dir(path).map_err(text)
    }
}

/// Maximum compressed/cached document bytes sent to the webview.
const MAX_DOCUMENT_BYTES: u64 = 50 * 1024 * 1024;

/// Only these documents travel over IPC; flekfile remains media-only.
fn document_path(cache: &Path, file: &messages::CachedFile) -> CmdResult<PathBuf> {
    let path = file
        .local_path
        .as_deref()
        .ok_or("the file is not downloaded yet")?;
    let path = flek_core::files::inside_cache(cache, Path::new(path))
        .ok_or("the file is outside the cache")?;
    if !flek_core::files::can_read_document(&path, file.mimetype.as_deref()) {
        return Err("unsupported document type or mismatched filename/MIME".into());
    }
    Ok(path)
}

async fn read_document(path: &Path) -> CmdResult<Vec<u8>> {
    let file = tokio::fs::File::open(path).await.map_err(text)?;
    if file.metadata().await.map_err(text)?.len() > MAX_DOCUMENT_BYTES {
        return Err("document_too_large: viewer limit is 50 MiB; open in the default app".into());
    }
    // Bound the read too, in case the file grows after metadata was inspected.
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(text)?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("document_too_large: viewer limit is 50 MiB; open in the default app".into());
    }
    Ok(bytes)
}

/// Cached PDF/XLSX/DOCX/SVG bytes. WebKit refuses fetch of cross-origin custom
/// scheme responses, so documents use IPC and are parsed only in the webview.
#[tauri::command]
pub async fn document_data(
    state: State<'_, AppState>,
    file_id: String,
    inline_preview: Option<bool>,
) -> CmdResult<tauri::ipc::Response> {
    let file = state
        .store
        .read(move |c| messages::cached_file(c, &file_id))
        .await
        .map_err(text)?
        .ok_or("unknown file")?;
    let cache = flek_core::files::default_cache_dir().map_err(text)?;
    let path = document_path(&cache, &file)?;
    // Keep the app boundary's canonical cache gate as well.
    let path = crate::files::inside_cache(&path).ok_or("the file is outside the cache")?;
    let bytes = if inline_preview.unwrap_or(false) {
        flek_core::files::read_inline_svg(&path, file.mimetype.as_deref())
            .await
            .map_err(text)?
    } else {
        read_document(&path).await?
    };
    Ok(tauri::ipc::Response::new(bytes))
}

/// Opens a link from a message in the default browser (never in Flek's
/// own webview).
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    let parsed = tauri::Url::parse(&url).map_err(text)?;
    if !external_scheme_allowed(&parsed) {
        return Err(format!("not opening {} links", parsed.scheme()));
    }
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(text)
}

#[tauri::command]
pub async fn browse_files(
    state: State<'_, AppState>,
    query: String,
    kind: Option<String>,
    sort: String,
    mine: bool,
    offset: usize,
) -> CmdResult<Vec<flek_core::reads::File>> {
    let Some(team) = state.team_id() else {
        return Ok(Vec::new());
    };
    state
        .store
        .read(move |c| {
            flek_core::reads::files(
                c,
                &team,
                &flek_core::reads::FileFilter {
                    query: Some(query),
                    kind,
                    sort: Some(sort),
                    mine,
                    offset,
                    limit: 100,
                    other_includes_media: true,
                    ..Default::default()
                },
            )
        })
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn fetch_file(
    app: AppHandle,
    state: State<'_, AppState>,
    file_id: String,
    reveal: bool,
) -> CmdResult<flek_core::reads::File> {
    let team = state.team_id().ok_or("no active workspace")?;
    let cache = flek_core::files::default_cache_dir().map_err(text)?;
    let read_id = file_id.clone();
    let path = match flek_core::files::stored_cached_path(&state.store, &cache, &file_id)
        .await
        .map_err(text)?
    {
        Some(path) => path,
        None => {
            let client = state.client().ok_or("not signed in to Slack")?;
            flek_core::files::fetch_stored(&client, &state.store, &cache, &file_id)
                .await
                .map_err(text)?
        }
    };
    let shares = state
        .store
        .read(move |c| {
            let mut stmt = c.prepare(
                "
                    SELECT conversation_id, ts
                    FROM message_files
                    WHERE file_id = ?1
                ",
            )?;
            let rows = stmt.query_map([file_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await
        .map_err(text)?;
    for (conversation_id, ts) in shares {
        let _ = state.bus.send(flek_core::sync::live::Change::Message {
            conversation_id,
            ts,
        });
    }
    if reveal {
        let path = crate::files::inside_cache(&path).ok_or("the file is outside the cache")?;
        flek_core::files::quarantine(&path).map_err(text)?;
        app.opener().reveal_item_in_dir(path).map_err(text)?;
    }
    state
        .store
        .read(move |c| flek_core::reads::file(c, &team, &read_id))
        .await
        .map_err(text)
}

/// Return an opaque receipt, never a path supplied by the webview.
#[tauri::command]
pub async fn download_file(
    app: AppHandle,
    state: State<'_, AppState>,
    file_id: String,
) -> CmdResult<String> {
    let directory = dirs::download_dir()
        .ok_or("downloads_unavailable: system Downloads directory is unknown")?;
    let file = fetch_file(app, state.clone(), file_id.clone(), false).await?;
    let cache = flek_core::files::default_cache_dir().map_err(text)?;
    let source = flek_core::files::stored_cached_path(&state.store, &cache, &file_id)
        .await
        .map_err(text)?
        .ok_or("the file is not downloaded yet")?;
    let name = file.file.name.unwrap_or_else(|| "file".into());
    let path = tokio::task::spawn_blocking(move || {
        flek_core::files::save_to_downloads(&source, &directory, &name)
    })
    .await
    .map_err(text)?
    .map_err(text)?;
    let mut saved = state.saved_downloads.lock().map_err(text)?;
    saved.next = saved
        .next
        .checked_add(1)
        .ok_or("download receipt limit reached")?;
    let token = saved.next.to_string();
    saved.paths.push_back((token.clone(), path));
    // Bound session memory; old receipts fail closed.
    if saved.paths.len() > 100 {
        saved.paths.pop_front();
    }
    Ok(token)
}

#[tauri::command]
pub fn reveal_download(app: AppHandle, state: State<'_, AppState>, token: String) -> CmdResult<()> {
    let path = state
        .saved_downloads
        .lock()
        .map_err(text)?
        .paths
        .iter()
        .find(|(saved, _)| saved == &token)
        .map(|(_, path)| path.clone())
        .ok_or("unknown download receipt")?;
    app.opener().reveal_item_in_dir(path).map_err(text)
}

fn external_scheme_allowed(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "mailto") || slack_channel_link(url)
}

/// A narrow OS-only deep link, never an arbitrary Slack command.
fn slack_channel_link(url: &tauri::Url) -> bool {
    if url.scheme() != "slack"
        || url.host_str() != Some("channel")
        || !url.path().is_empty()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    let pairs: Vec<_> = url.query_pairs().collect();
    if !(2..=3).contains(&pairs.len()) {
        return false;
    }
    let id = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    };
    let exactly_one = |key: &str| pairs.iter().filter(|(k, _)| k == key).count() == 1;
    exactly_one("team")
        && exactly_one("id")
        && pairs.iter().all(|(key, value)| match key.as_ref() {
            "team" | "id" => id(value),
            "message" => {
                let Some((seconds, fraction)) = value.split_once('.') else {
                    return false;
                };
                exactly_one("message")
                    && !seconds.is_empty()
                    && !fraction.is_empty()
                    && seconds.bytes().all(|b| b.is_ascii_digit())
                    && fraction.bytes().all(|b| b.is_ascii_digit())
            }
            _ => false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn document_gate_checks_cache_type_and_size() {
        let cache = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let make = |path: PathBuf, mime: &str| messages::CachedFile {
            local_path: Some(path.to_string_lossy().into_owned()),
            preview_path: None,
            mimetype: Some(mime.into()),
        };
        for (ext, mime) in [
            ("pdf", "application/pdf"),
            (
                "xlsx",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            (
                "docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            ("svg", "image/svg+xml"),
        ] {
            let path = cache.path().join(format!("sample.{ext}"));
            std::fs::write(&path, b"synthetic").unwrap();
            assert_eq!(
                document_path(cache.path(), &make(path.clone(), mime)).unwrap(),
                path.canonicalize().unwrap()
            );
            assert_eq!(read_document(&path).await.unwrap(), b"synthetic");
            assert!(document_path(cache.path(), &make(path, "image/png")).is_err());
        }
        for (ext, mime) in [
            ("xls", "application/vnd.ms-excel"),
            ("doc", "application/msword"),
            (
                "pptx",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
            ("docx", "application/pdf"),
            ("svg", "text/html"),
            ("html", "image/svg+xml"),
            ("png", "image/png"),
            ("mp4", "video/mp4"),
        ] {
            let path = cache.path().join(format!("refused.{ext}"));
            std::fs::write(&path, b"synthetic").unwrap();
            assert!(document_path(cache.path(), &make(path, mime)).is_err());
        }
        let path = outside.path().join("outside.pdf");
        std::fs::write(&path, b"synthetic").unwrap();
        assert!(document_path(cache.path(), &make(path, "application/pdf")).is_err());
        let mut missing = make(cache.path().join("missing.pdf"), "application/pdf");
        assert!(document_path(cache.path(), &missing).is_err());
        missing.local_path = None;
        assert!(document_path(cache.path(), &missing).is_err());
        let path = cache.path().join("large.pdf");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_DOCUMENT_BYTES).unwrap();
        assert_eq!(
            read_document(&path).await.unwrap().len() as u64,
            MAX_DOCUMENT_BYTES
        );
        file.set_len(MAX_DOCUMENT_BYTES + 1).unwrap();
        assert!(read_document(&path)
            .await
            .unwrap_err()
            .contains("document_too_large"));
    }

    #[test]
    fn document_ipc_permission_is_registered_for_main_only() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../../capabilities/main.json")).unwrap();
        assert_eq!(capability["windows"], serde_json::json!(["main"]));
        let permissions = capability["permissions"].as_array().unwrap();
        assert!(permissions.iter().any(|p| p == "allow-document-data"));
        assert!(permissions.iter().any(|p| p == "allow-platform"));
        assert!(include_str!("../../build.rs").contains("\"platform\""));
        assert!(!permissions.iter().any(|p| p == "allow-pdf-data"));
        let core: Vec<_> = permissions
            .iter()
            .filter_map(|p| p.as_str())
            .filter(|p| p.starts_with("core:"))
            .collect();
        assert_eq!(
            core,
            [
                "core:event:allow-listen",
                "core:event:allow-unlisten",
                "core:window:allow-start-dragging",
                "core:window:allow-internal-toggle-maximize"
            ]
        );
    }

    #[test]
    fn native_slack_opener_only_accepts_exact_channel_deep_links() {
        for value in [
            "slack://channel?team=T1&id=C1",
            "slack://channel?team=T1&id=D1&message=1700000000.000001",
        ] {
            assert!(
                external_scheme_allowed(&tauri::Url::parse(value).unwrap()),
                "{value}"
            );
        }
        for value in [
            "slack://channel?team=T1&id=C1&command=delete",
            "slack://channel?team=T1&id=C1&message=bad",
            "slack://channel?team=T1&id=C1&id=C2",
            "slack://channel?team=T1&id=C1#other",
            "slack://channel/path?team=T1&id=C1",
            "slack://user@channel?team=T1&id=C1",
            "slack://channel:123?team=T1&id=C1",
            "slack://channel?team=T1%26other&id=C1",
            "slack://open?team=T1&id=C1",
        ] {
            assert!(
                !external_scheme_allowed(&tauri::Url::parse(value).unwrap()),
                "{value}"
            );
        }
    }
    #[test]
    fn external_opener_rejects_non_web_and_non_mail_schemes() {
        for value in [
            "https://example.test",
            "http://example.test",
            "mailto:a@example.test",
        ] {
            assert!(external_scheme_allowed(&tauri::Url::parse(value).unwrap()));
        }
        for value in [
            "file:///tmp/a",
            "javascript:alert(1)",
            "data:text/html,hi",
            "flekfile://localhost/F",
            "slack://channel?id=C",
        ] {
            assert!(!external_scheme_allowed(&tauri::Url::parse(value).unwrap()));
        }
    }
}
