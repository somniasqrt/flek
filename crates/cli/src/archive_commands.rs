//! Offline archive and conversation commands.

use crate::default_team;
use crate::{open_db, output::emit, Global};
use anyhow::{Context, Result};
use flek_core::views;
use serde_json::json;

pub(super) fn db(g: &Global) -> Result<()> {
    let db = open_db(g)?;
    let path = db
        .path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let version = db.schema_version()?;
    if g.json {
        emit(json!({"path":path,"db_schema_version":version}));
    } else {
        println!("path    {path}");
        println!("schema  v{version}");
    }
    Ok(())
}

pub(super) fn import_archive(
    g: &Global,
    path: &std::path::Path,
    channel: Option<&str>,
) -> Result<()> {
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let stats = db
        .with_conn(|c| flek_core::sync::import::import_path(c, &team, path, channel))
        .with_context(|| format!("importing {}", path.display()))?;
    if g.json {
        emit(json!({"summary":stats}));
        return Ok(());
    }
    println!(
        "{} archive: {} messages read, {} added or recovered",
        stats.format, stats.read, stats.written
    );
    if !stats.unknown_channels.is_empty() {
        println!(
            "skipped (no such conversation, run `flek sync` or pass --channel): {}",
            stats.unknown_channels.join(", ")
        );
    }
    Ok(())
}

pub(super) fn channels(g: &Global, archived: bool, non_member: bool) -> Result<()> {
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let rows: Vec<_> = db
        .with_read(|c| {
            flek_core::store::directory::list_conversations_including(c, &team, non_member)
        })?
        .into_iter()
        .filter(|r| archived || !r.is_archived)
        .collect();
    if g.json {
        let conversations =
            db.with_read(|c| views::conversations_including(c, &team, archived, non_member))?;
        emit(json!({"conversations":conversations}));
        return Ok(());
    }
    if rows.is_empty() {
        println!("no conversations stored; run `flek sync`");
    }
    for r in rows {
        let prefix = match r.kind.as_str() {
            "public_channel" => "#",
            "private_channel" => "🔒",
            "mpim" => "👥",
            _ => "@",
        };
        let archived = if r.is_archived { "  (archived)" } else { "" };
        let unread = match r.unread {
            Some(0) | None => String::new(),
            Some(n) => format!("  ({n} unread)"),
        };
        println!(
            "{prefix} {}{archived}  {} msgs{unread}",
            r.label, r.messages
        );
    }
    Ok(())
}
