//! Files IPC adapters.

use super::{text, CmdResult};
use crate::state::AppState;
use flek_core::store::messages;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

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
        let path = crate::files::inside_cache(&path).ok_or("the file is outside the cache")?;
        flek_core::files::quarantine(&path).map_err(text)?;
        app.opener().reveal_item_in_dir(path).map_err(text)?;
    }
    Ok(())
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
