//! Messages storage views.

use super::text::{datetime, resolve_with};
use crate::Result;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct File {
    pub id: String,
    pub name: Option<String>,
    pub mimetype: Option<String>,
    pub size: Option<i64>,
    pub local_path: Option<String>,
    pub can_open: bool,
    pub viewer_kind: Option<crate::files::ViewerKind>,
}

impl File {
    pub(crate) fn new(
        id: String,
        name: Option<String>,
        mimetype: Option<String>,
        size: Option<i64>,
        local_path: Option<String>,
    ) -> Self {
        let path = local_path
            .as_deref()
            .or(name.as_deref())
            .unwrap_or_default();
        let can_open = crate::files::can_open(std::path::Path::new(path), mimetype.as_deref());
        let viewer_kind =
            crate::files::viewer_kind(std::path::Path::new(path), mimetype.as_deref());
        Self {
            id,
            name: name.as_deref().map(crate::files::display_name),
            mimetype,
            size,
            local_path,
            can_open,
            viewer_kind,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Reaction {
    pub name: String,
    pub count: usize,
    pub users: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub conversation_id: String,
    pub conversation: String,
    pub ts: String,
    pub datetime: String,
    pub thread_ts: Option<String>,
    pub reply_count: i64,
    pub user_id: Option<String>,
    pub user: Option<String>,
    pub text: String,
    pub text_resolved: String,
    pub edited: bool,
    pub deleted: bool,
    pub hidden_by_limit: bool,
    pub files: Vec<File>,
    pub reactions: Vec<Reaction>,
    pub permalink: Option<String>,
}

pub fn message(conn: &Connection, conversation_id: &str, ts: &str) -> Result<Option<Message>> {
    Ok(messages(conn, &[(conversation_id.to_owned(), ts.to_owned())])?.pop())
}

/// Hydrate a page with fixed joined queries for messages, files, reactions and labels.
pub fn messages(conn: &Connection, keys: &[(String, String)]) -> Result<Vec<Message>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let keys = serde_json::to_string(keys)?;
    let mut stmt = conn.prepare(
        "
            SELECT m.thread_ts, m.reply_count, m.user_id, COALESCE(NULLIF(u.display_name, ''), u.name,
                    m.author_name), COALESCE(m.text, ''), m.edited_ts IS NOT NULL, m.deleted, m.hidden_by_limit,
                COALESCE(c.label, c.name, c.id), t.url, m.conversation_id, m.ts
            FROM json_each(?1) k
            JOIN messages m ON m.conversation_id = json_extract(k.value, '$[0]')
            AND m.ts = json_extract(k.value, '$[1]')
            JOIN conversations c ON c.id = m.conversation_id
            JOIN teams t ON t.id = c.team_id
            LEFT JOIN users u ON u.id = m.user_id
            ORDER BY CAST (k.KEY AS INTEGER)
        ",
    )?;
    let mut rows = stmt
        .query_map([&keys], |r| {
            let conversation_id: String = r.get(10)?;
            let ts: String = r.get(11)?;
            let thread_ts = r.get::<_, Option<String>>(0)?.filter(|t| t != &ts);
            let url: Option<String> = r.get(9)?;
            let permalink = url.filter(|u| !u.is_empty()).map(|url| {
                let mut link = format!(
                    "{}/archives/{}/p{}",
                    url.trim_end_matches('/'),
                    conversation_id,
                    ts.replace('.', "")
                );
                if let Some(root) = &thread_ts {
                    link.push_str(&format!("?thread_ts={root}&cid={conversation_id}"));
                }
                link
            });
            Ok(Message {
                conversation_id,
                conversation: r.get(8)?,
                ts: ts.clone(),
                datetime: datetime(&ts).unwrap_or_default(),
                thread_ts,
                reply_count: r.get(1)?,
                user_id: r.get(2)?,
                user: r.get(3)?,
                text: r.get(4)?,
                text_resolved: String::new(),
                edited: r.get(5)?,
                deleted: r.get(6)?,
                hidden_by_limit: r.get(7)?,
                files: Vec::new(),
                reactions: Vec::new(),
                permalink,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let positions: BTreeMap<_, _> = rows
        .iter()
        .enumerate()
        .map(|(i, m)| ((m.conversation_id.clone(), m.ts.clone()), i))
        .collect();
    let mut stmt = conn.prepare(
        "
            SELECT mf.conversation_id, mf.ts, f.id, f.name, f.mimetype, f.size, f.local_path
            FROM json_each(?1) k
            JOIN message_files mf ON mf.conversation_id = json_extract(k.value, '$[0]')
            AND mf.ts = json_extract(k.value, '$[1]')
            JOIN files f ON f.id = mf.file_id
            ORDER BY f.id
        ",
    )?;
    let mut cursor = stmt.query([&keys])?;
    while let Some(r) = cursor.next()? {
        let key = (r.get::<_, String>(0)?, r.get::<_, String>(1)?);
        if let Some(&i) = positions.get(&key) {
            rows[i].files.push(File::new(
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ));
        }
    }
    let mut stmt = conn.prepare(
        "
            SELECT r.conversation_id, r.ts, r.name, r.user_id
            FROM json_each(?1) k
            JOIN reactions r ON r.conversation_id = json_extract(k.value, '$[0]')
            AND r.ts = json_extract(k.value, '$[1]')
            ORDER BY r.name, r.user_id
        ",
    )?;
    let mut cursor = stmt.query([&keys])?;
    while let Some(r) = cursor.next()? {
        let key = (r.get::<_, String>(0)?, r.get::<_, String>(1)?);
        if let Some(&i) = positions.get(&key) {
            let name: String = r.get(2)?;
            let user: String = r.get(3)?;
            if let Some(reaction) = rows[i].reactions.iter_mut().find(|x| x.name == name) {
                reaction.count += 1;
                reaction.users.push(user);
            } else {
                rows[i].reactions.push(Reaction {
                    name,
                    count: 1,
                    users: vec![user],
                });
            }
        }
    }
    // All referenced labels are joined once, even when every row mentions a different person.
    let ids: std::collections::BTreeSet<_> = rows
        .iter()
        .flat_map(|m| m.text.split('<').skip(1))
        .filter_map(|s| {
            s.split_once('>')
                .map(|(t, _)| t.split('|').next().unwrap_or(t))
        })
        .filter(|t| t.starts_with(['@', '#']))
        .map(str::to_owned)
        .collect();
    let ids = serde_json::to_string(&ids)?;
    let mut stmt = conn.prepare(
        "
            SELECT '@' || u.id, COALESCE(u.display_name, u.name)
            FROM json_each(?1) k
            JOIN users u ON u.id = substr(k.value, 2)
            WHERE substr(k.value, 1, 1) = '@'
            UNION ALL
            SELECT '#' || c.id, COALESCE(c.name, c.label, c.id)
            FROM json_each(?1) k
            JOIN conversations c ON c.id = substr(k.value, 2)
            WHERE substr(k.value, 1, 1) = '#'
        ",
    )?;
    let labels = stmt
        .query_map([ids], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
    for m in &mut rows {
        m.text_resolved = resolve_with(&m.text, |sigil, id| {
            Ok(labels.get(&format!("{sigil}{id}")).cloned())
        })?;
    }
    Ok(rows)
}

/// Stored timeline or one thread, oldest first, using the same visibility
/// rules as the human timeline. Entity hydration stays in core.
pub fn history(
    conn: &Connection,
    id: &str,
    limit: usize,
    thread: Option<&str>,
) -> Result<Vec<Message>> {
    let rows = match thread {
        Some(root) => crate::store::messages::thread(conn, id, root)?,
        None => crate::store::messages::recent(conn, id, limit)?,
    };
    messages(
        conn,
        &rows
            .into_iter()
            .map(|r| (id.to_owned(), r.ts))
            .collect::<Vec<_>>(),
    )
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    #[serde(flatten)]
    pub message: Message,
    pub snippet: String,
}

pub fn search_results(
    conn: &Connection,
    hits: &[crate::store::search::Hit],
) -> Result<Vec<SearchResult>> {
    hits.iter()
        .map(|h| {
            Ok(SearchResult {
                message: message(conn, &h.conversation_id, &h.ts)?.ok_or_else(|| {
                    crate::Error::NotFound(format!(
                        "no stored message {} in conversation {}",
                        h.ts, h.conversation_id
                    ))
                })?,
                snippet: h.snippet.clone(),
            })
        })
        .collect()
}
