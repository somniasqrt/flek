//! Versioned, secret-free machine views built exclusively from stored data.
use crate::{references, Result};
use chrono::{DateTime, SecondsFormat};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct Conversation {
    pub id: String,
    pub name: Option<String>,
    pub label: String,
    pub kind: String,
    pub is_archived: bool,
    pub is_member: bool,
    pub topic: Option<String>,
    pub purpose: Option<String>,
    pub unread_count: Option<i64>,
    pub last_read: Option<String>,
    pub latest_ts: Option<String>,
    pub synced_at: Option<String>,
    pub history_complete: bool,
}

fn datetime(ts: &str) -> Option<String> {
    let (secs, micros) = ts.split_once('.').unwrap_or((ts, "0"));
    let nanos = format!("{micros:0<9}").get(..9)?.parse().ok()?;
    Some(
        DateTime::from_timestamp(secs.parse().ok()?, nanos)?
            .to_rfc3339_opts(SecondsFormat::AutoSi, true),
    )
}

pub fn conversation(conn: &Connection, id: &str) -> Result<Option<Conversation>> {
    Ok(conn.query_row(
        "SELECT c.id, c.name, COALESCE(c.label,c.name,c.id), c.kind, c.is_archived, c.is_member,
         c.topic, c.purpose, c.unread_count, c.last_read, c.latest_ts, s.updated_at, COALESCE(s.complete,0)
         FROM conversations c LEFT JOIN sync_state s ON s.conversation_id=c.id WHERE c.id=?1", [id], |r| {
            let updated: Option<i64> = r.get(11)?;
            Ok(Conversation {
                id: r.get(0)?,
                name: r.get(1)?,
                label: r.get(2)?,
                kind: references::kind(&r.get::<_, String>(3)?).into(),
                is_archived: r.get(4)?,
                is_member: r.get(5)?,
                topic: r.get(6)?,
                purpose: r.get(7)?,
                unread_count: r.get(8)?,
                last_read: r.get(9)?,
                latest_ts: r.get(10)?,
                synced_at: updated.and_then(|s| datetime(&s.to_string())),
                history_complete: r.get(12)?,
            })
        }).optional()?)
}

pub fn conversations(conn: &Connection, team: &str, archived: bool) -> Result<Vec<Conversation>> {
    crate::store::directory::list_conversations(conn, team)?
        .into_iter()
        .filter(|c| archived || !c.is_archived)
        .map(|c| {
            conversation(conn, &c.id)?.ok_or_else(|| {
                crate::Error::NotFound(format!("stored conversation {} was not found", c.id))
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub real_name: Option<String>,
    pub is_bot: bool,
    pub deleted: bool,
    pub tz: Option<String>,
}
pub fn user(conn: &Connection, id: &str) -> Result<Option<User>> {
    Ok(
        crate::store::directory::user_profile(conn, id)?.map(|p| User {
            id: p.id,
            name: p.name,
            display_name: p.display_name,
            real_name: p.real_name,
            is_bot: p.is_bot,
            deleted: p.deleted,
            tz: p.tz,
        }),
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct File {
    pub id: String,
    pub name: Option<String>,
    pub mimetype: Option<String>,
    pub size: Option<i64>,
    pub local_path: Option<String>,
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
    let row = conn.query_row(
        "SELECT m.thread_ts,m.reply_count,m.user_id,COALESCE(NULLIF(u.display_name,''),u.name,m.author_name),
         COALESCE(m.text,''),m.edited_ts IS NOT NULL,m.deleted,m.hidden_by_limit,
         COALESCE(c.label,c.name,c.id),t.url
         FROM messages m JOIN conversations c ON c.id=m.conversation_id JOIN teams t ON t.id=c.team_id
         LEFT JOIN users u ON u.id=m.user_id WHERE m.conversation_id=?1 AND m.ts=?2",
        params![conversation_id, ts],
        |r| {
            let thread_ts = r.get::<_, Option<String>>(0)?.filter(|t| t != ts);
            let url: Option<String> = r.get(9)?;
            let permalink = url.filter(|u| !u.is_empty()).map(|url| {
                let mut link = format!("{}/archives/{}/p{}", url.trim_end_matches('/'), conversation_id, ts.replace('.', ""));
                if let Some(root) = &thread_ts {
                    link.push_str(&format!("?thread_ts={root}&cid={conversation_id}"));
                }
                link
            });
            Ok(Message {
                conversation_id: conversation_id.into(),
                conversation: r.get(8)?,
                ts: ts.into(),
                datetime: datetime(ts).unwrap_or_default(),
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
        },
    ).optional()?;
    let Some(mut message) = row else {
        return Ok(None);
    };
    let mut stmt = conn.prepare("SELECT f.id,f.name,f.mimetype,f.size,f.local_path FROM files f JOIN message_files mf ON mf.file_id=f.id WHERE mf.conversation_id=?1 AND mf.ts=?2 ORDER BY f.id")?;
    let files = stmt
        .query_map(params![conversation_id, ts], |r| {
            Ok(File {
                id: r.get(0)?,
                name: r.get(1)?,
                mimetype: r.get(2)?,
                size: r.get(3)?,
                local_path: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut stmt = conn.prepare("SELECT name,user_id FROM reactions WHERE conversation_id=?1 AND ts=?2 ORDER BY name,user_id")?;
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pair in stmt.query_map(params![conversation_id, ts], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (name, user) = pair?;
        groups.entry(name).or_default().push(user);
    }
    let reactions = groups
        .into_iter()
        .map(|(name, users)| Reaction {
            name,
            count: users.len(),
            users,
        })
        .collect();
    message.files = files;
    message.reactions = reactions;
    message.text_resolved = resolve_text(conn, &message.text)?;
    Ok(Some(message))
}

/// Resolve Slack angle-bracket references before decoding escaped text.
/// Unknown identities retain their id (or Slack's supplied label).
pub fn resolve_text(conn: &Connection, text: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>').map(|i| start + i) else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let inner = &rest[start + 1..end];
        let (target, label) = inner
            .split_once('|')
            .map_or((inner, None), |(a, b)| (a, Some(b)));
        if let Some(id) = target.strip_prefix('@') {
            out.push('@');
            out.push_str(
                &crate::store::directory::user_label(conn, id)?
                    .unwrap_or_else(|| label.unwrap_or(id).into()),
            );
        } else if let Some(id) = target.strip_prefix('#') {
            let name: Option<String> = conn
                .query_row(
                    "SELECT COALESCE(name,label,id) FROM conversations WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            out.push('#');
            out.push_str(&name.unwrap_or_else(|| label.unwrap_or(id).into()));
        } else if ["https://", "http://", "mailto:"]
            .iter()
            .any(|p| target.starts_with(p))
        {
            if let Some(label) = label {
                out.push_str(label);
                out.push_str(" (");
                out.push_str(target);
                out.push(')');
            } else {
                out.push_str(target);
            }
        } else {
            out.push_str(&rest[start..=end]);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(decode_entities(&out))
}

fn decode_entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';') else {
            break;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            other => other
                .strip_prefix("#x")
                .and_then(|s| u32::from_str_radix(s, 16).ok())
                .or_else(|| other.strip_prefix('#').and_then(|s| s.parse().ok()))
                .and_then(char::from_u32),
        };
        if let Some(ch) = decoded {
            out.push(ch);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
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
    rows.iter()
        .map(|r| {
            message(conn, id, &r.ts)?.ok_or_else(|| {
                crate::Error::NotFound(format!("no stored message {} in conversation {id}", r.ts))
            })
        })
        .collect()
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
