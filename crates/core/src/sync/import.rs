//! Import existing archives so no history is lost. Formats are described in
//! `docs/import-formats.md`:
//! - simple: `<channel>.json` holding an array of `{ts, author, text, links, files, thread_ts}`
//!   (a single file or a directory of them);
//! - Slack export (also written by slackdump): `channels.json`, `users.json`, …
//!   and `<channel>/<YYYY-MM-DD>.json` with raw Slack messages.
//!
//! Archived messages only fill gaps (see `store::messages::import_messages`),
//! and importing the same archive twice changes nothing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::Value;

use crate::api::{Message, User};
use crate::store::directory;
use crate::store::messages::{self, MessageRow};
use crate::sync::backfill::to_row;
use crate::{Error, Result};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ImportStats {
    pub format: &'static str,
    /// Messages read from the archive.
    pub read: usize,
    /// Messages that were new or replaced a hidden one.
    pub written: usize,
    /// Channels in the archive that match no known conversation.
    pub unknown_channels: Vec<String>,
}

/// Imports `path` into `team_id`. `channel` forces every message into one
/// conversation (simple format only).
pub fn import_path(
    conn: &mut Connection,
    team_id: &str,
    path: &Path,
    channel: Option<&str>,
) -> Result<ImportStats> {
    if path.is_dir() && (path.join("channels.json").is_file() || path.join("users.json").is_file())
    {
        import_slack_export(conn, team_id, path)
    } else {
        import_simple(conn, team_id, path, channel)
    }
}

// ---- simple format ----

#[derive(Deserialize)]
struct SimpleMessage {
    ts: Value,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    text: String,
    #[serde(default)]
    links: Vec<String>,
    #[serde(default)]
    files: Vec<Value>,
    #[serde(default)]
    thread_ts: Option<Value>,
}

fn import_simple(
    conn: &mut Connection,
    team_id: &str,
    path: &Path,
    channel: Option<&str>,
) -> Result<ImportStats> {
    let files: Vec<PathBuf> = if path.is_dir() {
        let mut v: Vec<PathBuf> = fs::read_dir(path)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        v.sort();
        v
    } else {
        vec![path.to_owned()]
    };

    let authors = author_index(conn)?;
    let mut stats = ImportStats {
        format: "simple",
        ..Default::default()
    };
    for file in files {
        let name = channel
            .map(str::to_owned)
            .or_else(|| file.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let Some(conv) = directory::find_conversation(conn, team_id, &name)? else {
            tracing::warn!(channel = %name, file = %file.display(), "no such conversation; run `flek sync` first or pass --channel");
            stats.unknown_channels.push(name);
            continue;
        };
        let items: Vec<Value> = serde_json::from_slice(&fs::read(&file)?).map_err(|e| {
            Error::DesktopData(format!(
                "{}: not a JSON array of messages: {e}",
                file.display()
            ))
        })?;
        let mut rows = Vec::with_capacity(items.len());
        for raw in items {
            let m: SimpleMessage = serde_json::from_value(raw.clone())
                .map_err(|e| Error::DesktopData(format!("{}: bad message: {e}", file.display())))?;
            rows.push(simple_row(m, raw, &authors)?);
        }
        stats.read += rows.len();
        stats.written += messages::import_messages(conn, &conv.id, &rows)?;
    }
    Ok(stats)
}

fn simple_row(
    m: SimpleMessage,
    raw: Value,
    authors: &HashMap<String, String>,
) -> Result<MessageRow> {
    let ts = normalize_ts(&m.ts).ok_or_else(|| Error::DesktopData(format!("bad ts {}", m.ts)))?;
    let mut text = m.text;
    for link in &m.links {
        if text.contains(link.as_str()) {
            continue;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(link);
    }
    for f in &m.files {
        let (name, url) = match f {
            Value::String(s) => (s.as_str(), ""),
            v => (
                v.get("name").and_then(Value::as_str).unwrap_or("file"),
                v.get("url").and_then(Value::as_str).unwrap_or(""),
            ),
        };
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(format!("[file] {name} {url}").trim_end());
    }
    let user_id = m
        .author
        .as_deref()
        .and_then(|a| authors.get(&a.to_lowercase()))
        .cloned();
    Ok(MessageRow {
        ts,
        thread_ts: m.thread_ts.as_ref().and_then(normalize_ts),
        author_name: if user_id.is_none() { m.author } else { None },
        user_id,
        text,
        raw_json: raw.to_string(),
        ..Default::default()
    })
}

/// Lowercased display name, real name and handle → user id.
fn author_index(conn: &Connection) -> Result<HashMap<String, String>> {
    let mut stmt = conn.prepare(
        "
            SELECT id, name, real_name, display_name
            FROM users
            ORDER BY deleted DESC
        ",
    )?;
    let mut map = HashMap::new();
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            [r.get::<_, Option<String>>(1)?, r.get(2)?, r.get(3)?],
        ))
    })?;
    // Active users are read last, so they win name clashes.
    for row in rows {
        let (id, names) = row?;
        for n in names.into_iter().flatten().filter(|n| !n.trim().is_empty()) {
            map.insert(n.to_lowercase(), id.clone());
        }
    }
    Ok(map)
}

/// Accepts `"1700000000.123456"` or a number; returns the canonical string.
fn normalize_ts(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => {
            let (secs, frac) = s.split_once('.').unwrap_or((s, ""));
            if secs.is_empty()
                || !secs.bytes().all(|b| b.is_ascii_digit())
                || !frac.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            Some(format!("{secs}.{:0<6}", &frac[..frac.len().min(6)]))
        }
        Value::Number(n) => n.as_f64().map(|f| format!("{f:.6}")),
        _ => None,
    }
}

// ---- Slack export / slackdump ----

#[derive(Deserialize)]
struct ExportConversation {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    created: Option<i64>,
    #[serde(default)]
    is_archived: bool,
    #[serde(default)]
    topic: Option<crate::api::TextValue>,
    #[serde(default)]
    purpose: Option<crate::api::TextValue>,
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    user: Option<String>,
}

fn import_slack_export(conn: &mut Connection, team_id: &str, root: &Path) -> Result<ImportStats> {
    let mut stats = ImportStats {
        format: "slack-export",
        ..Default::default()
    };
    import_export_users(conn, team_id, &root.join("users.json"))?;

    // Folder name → conversation id. Channels, groups and mpims use the
    // name, DMs the id.
    let mut folders: HashMap<String, String> = HashMap::new();
    for (file, kind) in [
        ("channels.json", "public_channel"),
        ("groups.json", "private_channel"),
        ("mpims.json", "mpim"),
        ("dms.json", "im"),
    ] {
        let path = root.join(file);
        if !path.is_file() {
            continue;
        }
        let convs: Vec<ExportConversation> = serde_json::from_slice(&fs::read(&path)?)
            .map_err(|e| Error::DesktopData(format!("{file}: {e}")))?;
        for c in convs {
            insert_export_conversation(conn, team_id, kind, &c)?;
            folders.insert(c.id.clone(), c.id.clone());
            if let Some(name) = &c.name {
                folders.insert(name.clone(), c.id.clone());
            }
        }
    }

    let mut dirs: Vec<PathBuf> = fs::read_dir(root)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let folder = dir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let conv_id = match folders.get(&folder) {
            Some(id) => id.clone(),
            None => match directory::find_conversation(conn, team_id, &folder)? {
                Some(c) => c.id,
                None => {
                    stats.unknown_channels.push(folder);
                    continue;
                }
            },
        };
        let mut days: Vec<PathBuf> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        days.sort();
        for day in days {
            let items: Vec<Value> = serde_json::from_slice(&fs::read(&day)?)
                .map_err(|e| Error::DesktopData(format!("{}: {e}", day.display())))?;
            let mut rows = Vec::with_capacity(items.len());
            for raw in items {
                let mut m: Message = match serde_json::from_value(raw.clone()) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::debug!(file = %day.display(), error = %e, "skipping unreadable message");
                        continue;
                    }
                };
                m.raw = raw;
                rows.push(to_row(&m));
            }
            stats.read += rows.len();
            stats.written += messages::import_messages(conn, &conv_id, &rows)?;
        }
    }
    Ok(stats)
}

/// Users from the export, only where the API has not given us better data.
fn import_export_users(conn: &mut Connection, team_id: &str, path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let raw: Vec<Value> = serde_json::from_slice(&fs::read(path)?)
        .map_err(|e| Error::DesktopData(format!("users.json: {e}")))?;
    let tx = conn.transaction()?;
    for v in raw {
        let Ok(u) = serde_json::from_value::<User>(v.clone()) else {
            continue;
        };
        let team = u.team_id.as_deref().unwrap_or(team_id);
        tx.execute(
            "
                INSERT OR IGNORE INTO teams(id, name)
                VALUES (?1, '')
            ",
            [team],
        )?;
        tx.execute(
            "
                INSERT OR IGNORE INTO users(id, team_id, name, real_name, display_name, is_bot,
                        deleted, raw_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ",
            params![
                u.id,
                team,
                u.name,
                u.real_name,
                u.label(),
                u.is_bot,
                u.deleted,
                v.to_string()
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Adds a conversation the API did not list (e.g. one we left). Existing
/// rows are not touched.
fn insert_export_conversation(
    conn: &Connection,
    team_id: &str,
    kind: &str,
    c: &ExportConversation,
) -> Result<()> {
    let dm_user = c.user.clone().or_else(|| c.members.first().cloned());
    let label = match kind {
        "im" => dm_user
            .as_deref()
            .and_then(|u| directory::user_label(conn, u).ok().flatten())
            .or_else(|| c.name.clone()),
        _ => c.name.clone(),
    };
    let text = |t: &Option<crate::api::TextValue>| {
        t.as_ref()
            .map(|t| t.value.clone())
            .filter(|v| !v.is_empty())
    };
    conn.execute(
        "
            INSERT OR IGNORE INTO conversations(id, team_id, kind, name, label, dm_user_id,
                    topic, purpose, is_archived, is_member, created)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10)
        ",
        params![
            c.id,
            team_id,
            kind,
            c.name,
            label,
            if kind == "im" { dm_user } else { None },
            text(&c.topic),
            text(&c.purpose),
            c.is_archived,
            c.created
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_ts() {
        assert_eq!(
            normalize_ts(&json!("1700000000.123456")).as_deref(),
            Some("1700000000.123456")
        );
        assert_eq!(
            normalize_ts(&json!("1700000000.1")).as_deref(),
            Some("1700000000.100000")
        );
        assert_eq!(
            normalize_ts(&json!("1700000000")).as_deref(),
            Some("1700000000.000000")
        );
        assert_eq!(
            normalize_ts(&json!(1700000000.5)).as_deref(),
            Some("1700000000.500000")
        );
        assert_eq!(normalize_ts(&json!("abc")), None);
        assert_eq!(normalize_ts(&json!(null)), None);
    }
}
