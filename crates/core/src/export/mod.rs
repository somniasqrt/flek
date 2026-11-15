//! Offline, incremental Markdown export. No API clients or credentials enter this module.
mod disk;
mod read;
mod render;

use crate::{references, store::unfurls::url_hash, Error, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// Time strings use the shared CLI parser; the interval is [since, until).
#[derive(Debug, Default)]
pub struct Options {
    pub channel: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub directory: PathBuf,
    pub conversations: usize,
    pub messages: usize,
    /// Unique URL occurrences per message, including Block Kit and unfurls.
    pub links: usize,
    /// Unique attachments; shares appear separately in the catalog.
    pub files: usize,
    /// Includes the index and links/files catalog.
    pub notes_written: usize,
    pub notes_unchanged: usize,
    pub attachments_copied: usize,
    pub attachments_unchanged: usize,
    pub attachments_unavailable: usize,
}

/// Export a consistent read snapshot. `cache` is the trusted Flek cache root;
/// recorded paths outside it are never read. Only the vault is written.
pub fn obsidian(
    conn: &Connection,
    team: &str,
    vault: &Path,
    cache: &Path,
    options: &Options,
) -> Result<Summary> {
    let now = chrono::Utc::now();
    let since = options
        .since
        .as_deref()
        .map(|s| crate::time::parse_at(s, now))
        .transpose()?;
    let until = options
        .until
        .as_deref()
        .map(|s| crate::time::parse_at(s, now))
        .transpose()?;
    if matches!((&since, &until), (Some(s), Some(u)) if s >= u) {
        return Err(Error::Usage("--since must be earlier than --until".into()));
    }
    let snapshot = conn.unchecked_transaction()?;
    let team_name = read::team_name(&snapshot, team)?;
    let conversations = read::conversations(&snapshot, team)?;
    let selected = options
        .channel
        .as_deref()
        .map(|query| {
            references::resolve(&snapshot, team, query, references::Match::Exact)?
                .required(query)
                .map(|c| c.id)
        })
        .transpose()?;
    let selected_ids: BTreeSet<_> = conversations
        .iter()
        .filter(|c| selected.as_ref().is_none_or(|id| id == &c.id))
        .map(|c| c.id.clone())
        .collect();
    let vault = disk::vault(vault)?;
    let mut directory = disk::directory(&vault, "Flek")?;
    directory = disk::directory(&directory, &format!("team-{}", url_hash(team)))?;
    // A filtered date projection can never replace an existing full archive.
    if since.is_some() || until.is_some() {
        directory = disk::directory(&directory, "ranges")?;
        directory = disk::directory(
            &directory,
            &url_hash(&serde_json::to_string(&(&since, &until))?),
        )?;
    }
    disk::directory(&directory, "conversations")?;
    disk::directory(&directory, "attachments")?;
    let suffix = selected
        .as_ref()
        .map_or(String::new(), |id| format!("-{}", url_hash(id)));
    let index_name = format!("index{suffix}");
    let catalog_name = format!("links-and-files{suffix}");
    let aliases = read::emoji_aliases(&snapshot, team)?;
    let context = render::Context {
        conn: &snapshot,
        conversations: &conversations,
        selected: &selected_ids,
        directory: &directory,
        vault: &vault,
        aliases: &aliases,
        since: since.as_deref(),
        until: until.as_deref(),
    };
    let mut summary = Summary {
        directory: directory.clone(),
        ..Summary::default()
    };
    let mut index = format!(
        "# {}\n\nOffline Flek archive. Times and day headings are UTC.\n\n{}\n\n",
        render::literal(&team_name),
        context.note(&catalog_name, "Links and files")
    );
    index.push_str(&format!(
        "Since (inclusive): {}. Until (exclusive): {}.\n\n",
        since.as_deref().unwrap_or("unbounded"),
        until.as_deref().unwrap_or("unbounded")
    ));
    let mut links = String::from("## Links\n\n| Message | Link |\n| --- | --- |\n");
    let mut files = String::from("## Files\n\n| Message | File | Cached source | Vault copy | Status |\n| --- | --- | --- | --- | --- |\n");
    let mut attachments = BTreeMap::new();
    for conversation in conversations
        .iter()
        .filter(|c| selected_ids.contains(&c.id))
    {
        let messages = read::messages(
            &snapshot,
            &conversation.id,
            since.as_deref(),
            until.as_deref(),
        )?;
        let mut note = format!(
            "# {}\n\nConversation: {} ({})\n\n{} · {}\n\n",
            render::literal(&conversation.label),
            render::literal(&conversation.id),
            render::literal(&conversation.kind),
            context.note(&index_name, "Index"),
            context.note(&catalog_name, "Links and files")
        );
        let present: BTreeSet<_> = messages.iter().map(|m| m.message.ts.as_str()).collect();
        let mut day = String::new();
        for record in &messages {
            let message = &record.message;
            let date = message.datetime.get(..10).unwrap_or("Unknown date");
            if date != day {
                day = date.to_owned();
                note.push_str(&format!("## {date}\n\n"));
            }
            context.message(&mut note, record, &present)?;
            let source_link = table_cell(&context.message_link(conversation, &message.ts));
            for url in &record.links {
                links.push_str(&format!(
                    "| {source_link} | {} |\n",
                    table_cell(&context.url(url, url))
                ));
                summary.links += 1;
            }
            for file in &record.files {
                if !attachments.contains_key(&file.id) {
                    let copy = disk::attachment(&directory, cache, file)?;
                    match copy.state {
                        disk::CopyState::Copied => summary.attachments_copied += 1,
                        disk::CopyState::Unchanged => summary.attachments_unchanged += 1,
                        disk::CopyState::Unavailable => summary.attachments_unavailable += 1,
                    }
                    attachments.insert(file.id.clone(), copy);
                }
                let copy = &attachments[&file.id];
                let name = file.name.as_deref().unwrap_or(&file.id);
                let remote = file
                    .permalink
                    .as_deref()
                    .map_or_else(|| render::literal(name), |u| context.url(u, name));
                let local = copy
                    .source
                    .as_ref()
                    .map_or_else(|| "—".into(), |p| render::literal(&p.to_string_lossy()));
                let target = copy
                    .relative
                    .as_deref()
                    .map_or_else(|| "—".into(), |p| context.file_link(p, name));
                let status = if file.hidden {
                    "hidden by limit"
                } else if copy.relative.is_some() {
                    "cached copy"
                } else {
                    "not cached / unavailable"
                };
                files.push_str(&format!(
                    "| {source_link} | {} | {local} | {} | {status} |\n",
                    table_cell(&remote),
                    table_cell(&target)
                ));
                note.push_str(&format!(
                    "File at {}: {}{}\n\n",
                    render::literal(&message.ts),
                    if copy.relative.is_some() {
                        target
                    } else {
                        String::new()
                    },
                    if copy.relative.is_none() {
                        format!("{remote} ({status})")
                    } else {
                        String::new()
                    }
                ));
            }
        }
        let path = conversation.note();
        index.push_str(&format!(
            "- {} — {} messages\n",
            context.note(&path, &conversation.label),
            messages.len()
        ));
        summary.conversations += 1;
        summary.messages += messages.len();
        write_note(&directory.join(format!("{path}.md")), &note, &mut summary)?;
    }
    summary.files = attachments.len();
    let catalog = format!(
        "# Links and files\n\n{}\n\n{links}\n{files}\n",
        context.note(&index_name, "Index")
    );
    write_note(
        &directory.join(format!("{catalog_name}.md")),
        &catalog,
        &mut summary,
    )?;
    write_note(
        &directory.join(format!("{index_name}.md")),
        &index,
        &mut summary,
    )?;
    snapshot.commit()?;
    Ok(summary)
}

fn write_note(path: &Path, content: &str, summary: &mut Summary) -> Result<()> {
    if disk::write_changed(path, render::redact(content).as_bytes())? {
        summary.notes_written += 1;
    } else {
        summary.notes_unchanged += 1;
    }
    Ok(())
}

fn table_cell(text: &str) -> String {
    text.replace('|', "\\|")
}
