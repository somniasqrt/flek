//! Files IPC adapters.

use super::{text, CmdResult};
use crate::state::AppState;
use flek_core::store::messages;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

/// Opens a cached file with its default app. Takes the file id, not a path,
/// so the UI cannot open arbitrary files.
#[tauri::command]
pub async fn open_file(
    app: AppHandle,
    state: State<'_, AppState>,
    file_id: String,
) -> CmdResult<()> {
    let path = state
        .store
        .read(move |c| messages::file_local_path(c, &file_id))
        .await
        .map_err(text)?
        .ok_or("the file is not downloaded yet")?;
    app.opener().open_path(path, None::<&str>).map_err(text)
}

/// Bytes of a cached PDF, for the in-app viewer. Goes over IPC rather than
/// `fetch(flekfile://…)`: reading a custom-scheme response from the page is
/// a cross-origin request WebKit refuses (images only display, so they can
/// keep using the scheme).
#[tauri::command]
pub async fn pdf_data(
    state: State<'_, AppState>,
    file_id: String,
) -> CmdResult<tauri::ipc::Response> {
    let file = state
        .store
        .read(move |c| messages::cached_file(c, &file_id))
        .await
        .map_err(text)?
        .ok_or("unknown file")?;
    if file.mimetype.as_deref() != Some("application/pdf") {
        return Err("not a PDF".into());
    }
    let path = file.local_path.ok_or("the file is not downloaded yet")?;
    let path = crate::files::inside_cache(std::path::Path::new(&path))
        .ok_or("the file is outside the cache")?;
    let bytes = tokio::fs::read(path).await.map_err(text)?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Opens a link from a message in the default browser (never in Flek's
/// own webview).
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    let parsed = tauri::Url::parse(&url).map_err(text)?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
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
) -> CmdResult<()> {
    let cache = flek_core::files::default_cache_dir().map_err(text)?;
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
        app.opener().reveal_item_in_dir(path).map_err(text)?;
    }
    Ok(())
}
