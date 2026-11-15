//! Snapshot queries and bounded hydration of stored archive data.
use crate::{store::unfurls::url_hash, views, Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Conversation {
    pub id: String,
    pub label: String,
    pub kind: String,
}
impl Conversation {
    pub fn note(&self) -> String {
        format!(
            "conversations/{}",
            crate::files::download_name(&format!("conversation-{}", url_hash(&self.id)))
        )
    }
}

pub(super) struct File {
    pub id: String,
    pub name: Option<String>,
    pub local_path: Option<String>,
    pub permalink: Option<String>,
    pub hidden: bool,
}

pub(super) struct Record {
    pub message: views::Message,
    pub links: BTreeSet<String>,
    pub files: Vec<File>,
}

pub(super) fn team_name(conn: &Connection, team: &str) -> Result<String> {
    conn.query_row("SELECT name FROM teams WHERE id = ?1", [team], |r| r.get(0))
        .optional()?
        .ok_or_else(|| Error::NotFound("no such stored team".into()))
}

pub(super) fn conversations(conn: &Connection, team: &str) -> Result<Vec<Conversation>> {
    let mut stmt = conn.prepare(
        "
        SELECT id, COALESCE(NULLIF(label, ''), name, id), kind
        FROM conversations
        WHERE team_id = ?1
        ORDER BY id
    ",
    )?;
    let rows = stmt
        .query_map([team], |r| {
            Ok(Conversation {
                id: r.get(0)?,
                label: r.get(1)?,
                kind: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

pub(super) fn emoji_aliases(conn: &Connection, team: &str) -> Result<BTreeMap<String, String>> {
    let mut stmt = conn.prepare(
        "
        SELECT name, alias_of
        FROM custom_emoji
        WHERE team_id = ?1 AND alias_of IS NOT NULL
        ORDER BY name
    ",
    )?;
    let rows = stmt
        .query_map([team], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

pub(super) fn messages(
    conn: &Connection,
    conversation: &str,
    since: Option<&str>,
    until: Option<&str>,
) -> Result<Vec<Record>> {
    let mut out = Vec::new();
    let mut cursor = String::new();
    loop {
        let mut stmt = conn.prepare(
            "
            SELECT ts, raw_json
            FROM messages
            WHERE conversation_id = ?1 AND ts > ?2 AND deleted = 0
                AND (?3 IS NULL OR ts >= ?3) AND (?4 IS NULL OR ts < ?4)
            ORDER BY ts
            LIMIT 500
        ",
        )?;
        let page = stmt
            .query_map(params![conversation, cursor, since, until], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if page.is_empty() {
            break;
        }
        cursor = page.last().unwrap().0.clone();
        let keys: Vec<_> = page
            .iter()
            .map(|(ts, _)| (conversation.to_owned(), ts.clone()))
            .collect();
        let key_json = serde_json::to_string(&keys)?;
        let mut files_stmt = conn.prepare(
            "
            SELECT mf.ts, f.id, f.name, f.local_path, f.permalink, f.hidden_by_limit
            FROM json_each(?1) k
            JOIN message_files mf ON mf.conversation_id = json_extract(k.value, '$[0]')
                AND mf.ts = json_extract(k.value, '$[1]')
            JOIN files f ON f.id = mf.file_id
            ORDER BY mf.ts, f.id
        ",
        )?;
        let mut files: BTreeMap<String, Vec<File>> = BTreeMap::new();
        let mut rows = files_stmt.query([key_json])?;
        while let Some(row) = rows.next()? {
            files.entry(row.get(0)?).or_default().push(File {
                id: row.get(1)?,
                name: row.get(2)?,
                local_path: row.get(3)?,
                permalink: row.get(4)?,
                hidden: row.get(5)?,
            });
        }
        for (mut message, (_, raw)) in views::messages(conn, &keys)?.into_iter().zip(page) {
            let mut links = super::render::links_in(&message.text);
            if let Some(raw) = raw.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            {
                // Explicit content fields only: never dump raw payloads or session metadata.
                for field in ["blocks", "attachments", "links"] {
                    if let Some(content) = raw.get(field) {
                        content_links(content, &mut links, 0);
                    }
                }
                if message.text.is_empty() {
                    if let Some(blocks) = raw.get("blocks") {
                        message.text = block_fallback(blocks, 0);
                    }
                }
            }
            out.push(Record {
                files: files.remove(&message.ts).unwrap_or_default(),
                message,
                links,
            });
        }
    }
    Ok(out)
}

fn content_links(value: &serde_json::Value, links: &mut BTreeSet<String>, depth: usize) {
    if depth > 32 {
        return;
    }
    match value {
        serde_json::Value::String(s) => links.extend(super::render::links_in(s)),
        serde_json::Value::Array(items) => {
            for item in items {
                content_links(item, links, depth + 1);
            }
        }
        serde_json::Value::Object(fields) => {
            for (key, child) in fields {
                if matches!(
                    key.as_str(),
                    "url"
                        | "original_url"
                        | "from_url"
                        | "title_link"
                        | "image_url"
                        | "text"
                        | "fallback"
                        | "elements"
                        | "fields"
                        | "accessory"
                        | "title"
                        | "blocks"
                ) {
                    content_links(child, links, depth + 1);
                }
            }
        }
        _ => {}
    }
}

fn block_fallback(value: &serde_json::Value, depth: usize) -> String {
    if depth > 32 {
        return String::new();
    }
    if let Some(items) = value.as_array() {
        return items
            .iter()
            .map(|v| block_fallback(v, depth + 1))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    let text = |key| value.get(key).and_then(serde_json::Value::as_str);
    match text("type") {
        Some("user") => text("user_id").map(|id| format!("<@{id}>")),
        Some("channel") => text("channel_id").map(|id| format!("<#{id}>")),
        Some("emoji") => text("name").map(|name| format!(":{name}:")),
        Some("link") => text("url").map(|u| format!("<{u}|{}>", text("text").unwrap_or(u))),
        _ => text("text").map(str::to_owned),
    }
    .unwrap_or_else(|| {
        ["text", "elements", "fields", "accessory", "title"]
            .iter()
            .filter_map(|key| value.get(key))
            .map(|v| block_fallback(v, depth + 1))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    })
}
