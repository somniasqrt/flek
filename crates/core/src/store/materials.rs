//! Derived local material catalogue; never reads credentials or the network.

use std::collections::BTreeMap;

use rusqlite::{params, Connection};
use serde_json::Value;

use crate::Result;

/// Extract HTTP(S) materials, not image-cache URLs. One URL per message;
/// unfurl titles improve the text/Block Kit label without adding duplicates.
pub fn links(text: &str, blocks: Option<&str>, raw: Option<&str>) -> BTreeMap<String, String> {
    fn decode(s: &str) -> String {
        s.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
    }
    fn add(out: &mut BTreeMap<String, String>, url: &str, title: Option<&str>) {
        let url = decode(url);
        let Ok(parsed) = reqwest::Url::parse(&url) else {
            return;
        };
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return;
        }
        let title = title.filter(|s| !s.trim().is_empty()).map(decode);
        out.entry(url.clone())
            .and_modify(|old| {
                if let Some(title) = &title {
                    *old = title.clone();
                }
            })
            .or_insert_with(|| title.unwrap_or(url));
    }
    fn scan(out: &mut BTreeMap<String, String>, text: &str) {
        let mut rest = text;
        while let Some(start) = rest.as_bytes().windows(7).position(|prefix| {
            prefix.eq_ignore_ascii_case(b"http://") || prefix.eq_ignore_ascii_case(b"https:/")
        }) {
            let angle = rest[..start].ends_with('<');
            rest = &rest[start..];
            let end = rest
                .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '|' | '"'))
                .unwrap_or(rest.len());
            let url = if angle {
                &rest[..end]
            } else {
                let mut url = rest[..end].trim_end_matches(['.', ',', ';', '!']);
                for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
                    while url.ends_with(close)
                        && url.matches(close).count() > url.matches(open).count()
                    {
                        url = &url[..url.len() - close.len_utf8()];
                    }
                }
                url
            };
            let label = rest[end..]
                .strip_prefix('|')
                .and_then(|s| s.split_once('>'))
                .map(|(s, _)| s);
            add(out, url, label);
            rest = &rest[end..];
        }
    }
    fn block(out: &mut BTreeMap<String, String>, v: &Value, depth: usize) {
        if depth > 32 {
            return;
        }
        match v.get("type").and_then(Value::as_str) {
            Some("link") => {
                if let Some(url) = v.get("url").and_then(Value::as_str) {
                    add(out, url, v.get("text").and_then(Value::as_str));
                }
            }
            Some("mrkdwn" | "plain_text" | "text") => {
                if let Some(text) = v.get("text").and_then(Value::as_str) {
                    scan(out, text);
                }
            }
            _ => {}
        }
        for key in ["elements", "fields", "text"] {
            if let Some(child) = v.get(key) {
                if let Some(list) = child.as_array() {
                    for child in list {
                        block(out, child, depth + 1);
                    }
                } else if child.is_object() {
                    block(out, child, depth + 1);
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    scan(&mut out, text);
    let raw = raw.and_then(|s| serde_json::from_str::<Value>(s).ok());
    let blocks = blocks.and_then(|s| serde_json::from_str::<Value>(s).ok());
    if let Some(blocks) = blocks.as_ref().or_else(|| raw.as_ref()?.get("blocks")) {
        for value in blocks.as_array().into_iter().flatten() {
            block(&mut out, value, 0);
        }
    }
    if let Some(raw) = raw {
        for a in raw
            .get("attachments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for key in ["text", "pretext"] {
                if let Some(text) = a.get(key).and_then(Value::as_str) {
                    scan(&mut out, text);
                }
            }
            for value in a
                .get("blocks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                block(&mut out, value, 0);
            }
            // These are the same documented link fields as store::unfurls.
            if let Some(url) = ["original_url", "from_url", "title_link"]
                .into_iter()
                .find_map(|key| a.get(key).and_then(Value::as_str))
            {
                add(&mut out, url, a.get("title").and_then(Value::as_str));
            }
        }
    }
    out
}

/// Re-read the winning stored content, never the incoming (possibly stale or
/// hidden) payload. Called inside the message/files transaction.
pub(crate) fn refresh(conn: &Connection, conversation: &str, ts: &str) -> Result<()> {
    let (id, team, text, blocks, raw, deleted): (
        i64,
        String,
        String,
        Option<String>,
        Option<String>,
        bool,
    ) = conn.query_row(
        "SELECT m.id, c.team_id, COALESCE(m.text, ''), m.blocks_json, m.raw_json, m.deleted
         FROM messages m JOIN conversations c ON c.id = m.conversation_id
         WHERE m.conversation_id = ?1 AND m.ts = ?2",
        params![conversation, ts],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        },
    )?;
    if deleted {
        conn.execute("DELETE FROM search_materials WHERE message_id = ?1", [id])?;
        return Ok(());
    }
    let links = links(&text, blocks.as_deref(), raw.as_deref());
    let keys = serde_json::to_string(&links.keys().collect::<Vec<_>>())?;
    conn.execute(
        "DELETE FROM search_materials
         WHERE message_id = ?1 AND (
            (kind = 'link' AND material_key NOT IN (SELECT value FROM json_each(?2)))
            OR (kind = 'file' AND NOT EXISTS (
                SELECT 1 FROM message_files mf WHERE mf.conversation_id = ?3
                AND mf.ts = ?4 AND mf.file_id = material_key)))",
        params![id, keys, conversation, ts],
    )?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO search_materials(message_id, team_id, conversation_id, ts, kind,
            material_key, title, title_key, url, context, user_id, author_name)
         SELECT ?1, ?2, ?3, ?4, 'link', ?5, ?6, lower(?6), ?5, ?7, user_id, author_name
         FROM messages WHERE id = ?1
         ON CONFLICT (message_id, kind, material_key) DO UPDATE
         SET title = excluded.title, title_key = excluded.title_key,
             context = excluded.context, user_id = excluded.user_id,
             author_name = excluded.author_name
         WHERE title IS NOT excluded.title OR context IS NOT excluded.context
             OR user_id IS NOT excluded.user_id OR author_name IS NOT excluded.author_name",
    )?;
    for (url, title) in links {
        insert.execute(params![id, team, conversation, ts, url, title, text])?;
    }
    // Reuse the Files browser's message_files share relation and file metadata.
    conn.execute(
        "INSERT INTO search_materials(message_id, team_id, conversation_id, ts, kind,
            material_key, title, title_key, url, context, user_id, author_name)
         SELECT ?1, ?2, ?3, ?4, 'file', f.id, COALESCE(f.name, f.title, f.id),
            lower(COALESCE(f.name, f.title, f.id)), f.permalink, ?5 || ' ' || COALESCE(f.title, ''), m.user_id, m.author_name
         FROM message_files mf JOIN files f ON f.id = mf.file_id
         JOIN messages m ON m.id = ?1
         WHERE mf.conversation_id = ?3 AND mf.ts = ?4
         ON CONFLICT (message_id, kind, material_key) DO UPDATE
         SET title = excluded.title, title_key = excluded.title_key, url = excluded.url,
             context = excluded.context, user_id = excluded.user_id,
             author_name = excluded.author_name
         WHERE title IS NOT excluded.title OR url IS NOT excluded.url
             OR context IS NOT excluded.context OR user_id IS NOT excluded.user_id
             OR author_name IS NOT excluded.author_name",
        params![id, team, conversation, ts, text],
    )?;
    Ok(())
}

/// Bounded-memory backfill, atomic with schema/version changes. Old archives
/// must be searchable even if sync never visits their messages again.
pub(crate) fn backfill(conn: &Connection) -> Result<()> {
    let mut after = 0;
    loop {
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, ts FROM messages WHERE id > ?1 ORDER BY id LIMIT 500",
        )?;
        let rows = stmt
            .query_map([after], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            break;
        }
        for (id, conversation, ts) in rows {
            refresh(conn, &conversation, &ts)?;
            after = id;
        }
    }
    Ok(())
}
