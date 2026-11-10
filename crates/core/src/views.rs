//! Versioned, secret-free machine views built exclusively from stored data.
use crate::{references, Result};
use chrono::{DateTime, SecondsFormat};
use rusqlite::{Connection, OptionalExtension};
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

const CONVERSATION_SELECT: &str =
    "SELECT c.id,c.name,COALESCE(c.label,c.name,c.id),c.kind,c.is_archived,c.is_member,
    c.topic,c.purpose,c.unread_count,c.last_read,c.latest_ts,s.updated_at,COALESCE(s.complete,0)
    FROM conversations c LEFT JOIN sync_state s ON s.conversation_id=c.id";
fn conversation_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Conversation> {
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
}
pub fn conversation(conn: &Connection, id: &str) -> Result<Option<Conversation>> {
    Ok(conn
        .query_row(
            &format!("{CONVERSATION_SELECT} WHERE c.id=?1"),
            [id],
            conversation_row,
        )
        .optional()?)
}
pub fn conversations(conn: &Connection, team: &str, archived: bool) -> Result<Vec<Conversation>> {
    conversations_including(conn, team, archived, false)
}

pub fn conversations_including(
    conn: &Connection,
    team: &str,
    archived: bool,
    non_member: bool,
) -> Result<Vec<Conversation>> {
    let mut stmt = conn.prepare(&format!(
        "{CONVERSATION_SELECT}
         WHERE c.team_id = ?1 AND (?3 OR c.is_member = 1)
           AND (?2 OR NOT c.is_archived)
         ORDER BY CASE c.kind
             WHEN 'public_channel' THEN 0 WHEN 'private_channel' THEN 0
             WHEN 'mpim' THEN 1 ELSE 2 END,
             c.is_archived, lower(COALESCE(c.label, c.name, c.id))"
    ))?;
    let rows = stmt
        .query_map(
            rusqlite::params![team, archived, non_member],
            conversation_row,
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
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
    pub title: Option<String>,
    pub status_text: Option<String>,
    pub status_emoji: Option<String>,
    pub status_expiration: Option<i64>,
    pub presence: String,
    pub dnd_until: Option<i64>,
    pub tz_label: Option<String>,
    pub email: Option<String>,
    pub is_admin: Option<bool>,
    pub is_owner: Option<bool>,
    pub dm_conversation_id: Option<String>,
}
pub fn user(conn: &Connection, id: &str) -> Result<Option<User>> {
    Ok(conn.query_row("SELECT u.id,u.name,u.display_name,u.real_name,u.is_bot,u.deleted,u.raw_json,
        (SELECT id FROM conversations WHERE dm_user_id=u.id AND team_id=u.team_id ORDER BY id LIMIT 1),
         p.user_id, p.presence, p.dnd_enabled, p.dnd_start, p.dnd_end,
         p.snooze_enabled, p.snooze_end, p.manual_away, p.auto_away, p.online,
         p.connection_count, p.presence_fresh, p.dnd_fresh, p.updated_at
        FROM users u LEFT JOIN presence p ON p.user_id=u.id WHERE u.id=?1", [id], user_row).optional()?)
}
pub(crate) fn user_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    let raw: Option<String> = r.get(6)?;
    let v: serde_json::Value = raw
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let p = &v["profile"];
    let text = |v: &serde_json::Value| v.as_str().map(str::to_owned);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let presence = if let Some(user_id) = r.get::<_, Option<String>>(8)? {
        crate::store::presence::Presence {
            user_id,
            presence: r.get(9)?,
            dnd_enabled: r.get(10)?,
            dnd_start: r.get(11)?,
            dnd_end: r.get(12)?,
            snooze_enabled: r.get(13)?,
            snooze_end: r.get(14)?,
            manual_away: r.get(15)?,
            auto_away: r.get(16)?,
            online: r.get(17)?,
            connection_count: r.get(18)?,
            presence_fresh: r.get(19)?,
            dnd_fresh: r.get(20)?,
            updated_at: r.get(21)?,
        }
    } else {
        Default::default()
    };
    Ok(User {
        presence: presence.indicator(now).to_owned(),
        dnd_until: presence.dnd_until(now),
        id: r.get(0)?,
        name: r.get(1)?,
        display_name: r.get(2)?,
        real_name: r.get(3)?,
        is_bot: r.get(4)?,
        deleted: r.get(5)?,
        tz: text(&v["tz"]),
        title: text(&p["title"]),
        status_text: text(&p["status_text"]),
        status_emoji: text(&p["status_emoji"]),
        status_expiration: p["status_expiration"].as_i64(),
        tz_label: text(&v["tz_label"]),
        email: text(&p["email"]),
        is_admin: v["is_admin"].as_bool(),
        is_owner: v["is_owner"].as_bool(),
        dm_conversation_id: r.get(7)?,
    })
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
    Ok(messages(conn, &[(conversation_id.to_owned(), ts.to_owned())])?.pop())
}

/// Hydrate a page with fixed joined queries for messages, files, reactions and labels.
pub fn messages(conn: &Connection, keys: &[(String, String)]) -> Result<Vec<Message>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let keys = serde_json::to_string(keys)?;
    let mut stmt = conn.prepare("SELECT m.thread_ts,m.reply_count,m.user_id,COALESCE(NULLIF(u.display_name,''),u.name,m.author_name),
         COALESCE(m.text,''),m.edited_ts IS NOT NULL,m.deleted,m.hidden_by_limit,
         COALESCE(c.label,c.name,c.id),t.url,m.conversation_id,m.ts
         FROM json_each(?1) k JOIN messages m ON m.conversation_id=json_extract(k.value,'$[0]') AND m.ts=json_extract(k.value,'$[1]')
         JOIN conversations c ON c.id=m.conversation_id JOIN teams t ON t.id=c.team_id
         LEFT JOIN users u ON u.id=m.user_id ORDER BY CAST(k.key AS INTEGER)")?;
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
    let mut stmt = conn.prepare("SELECT mf.conversation_id,mf.ts,f.id,f.name,f.mimetype,f.size,f.local_path
        FROM json_each(?1) k JOIN message_files mf ON mf.conversation_id=json_extract(k.value,'$[0]') AND mf.ts=json_extract(k.value,'$[1]')
        JOIN files f ON f.id=mf.file_id ORDER BY f.id")?;
    let mut cursor = stmt.query([&keys])?;
    while let Some(r) = cursor.next()? {
        let key = (r.get::<_, String>(0)?, r.get::<_, String>(1)?);
        if let Some(&i) = positions.get(&key) {
            rows[i].files.push(File {
                id: r.get(2)?,
                name: r.get(3)?,
                mimetype: r.get(4)?,
                size: r.get(5)?,
                local_path: r.get(6)?,
            });
        }
    }
    let mut stmt=conn.prepare("SELECT r.conversation_id,r.ts,r.name,r.user_id FROM json_each(?1) k JOIN reactions r
        ON r.conversation_id=json_extract(k.value,'$[0]') AND r.ts=json_extract(k.value,'$[1]') ORDER BY r.name,r.user_id")?;
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
    let mut stmt=conn.prepare("SELECT '@'||u.id,COALESCE(u.display_name,u.name) FROM json_each(?1) k JOIN users u ON u.id=substr(k.value,2) WHERE substr(k.value,1,1)='@'
        UNION ALL SELECT '#'||c.id,COALESCE(c.name,c.label,c.id) FROM json_each(?1) k JOIN conversations c ON c.id=substr(k.value,2) WHERE substr(k.value,1,1)='#'")?;
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

/// Resolve Slack angle-bracket references before decoding escaped text.
/// Unknown identities retain their id (or Slack's supplied label).
pub fn resolve_text(conn: &Connection, text: &str) -> Result<String> {
    resolve_with(text, |sigil, id| {
        if sigil == '@' {
            crate::store::directory::user_label(conn, id)
        } else {
            Ok(conn
                .query_row(
                    "SELECT COALESCE(name,label,id) FROM conversations WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?)
        }
    })
}
fn resolve_with(
    text: &str,
    mut lookup: impl FnMut(char, &str) -> Result<Option<String>>,
) -> Result<String> {
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
            out.push_str(&lookup('@', id)?.unwrap_or_else(|| label.unwrap_or(id).into()));
        } else if let Some(id) = target.strip_prefix('#') {
            let name = lookup('#', id)?;
            out.push('#');
            out.push_str(&name.unwrap_or_else(|| label.unwrap_or(id).into()));
        } else if let Some(special) = target.strip_prefix('!') {
            // `<!here>`, `<!channel>`, `<!subteam^S…|@team>`, `<!date^…|fallback>`.
            match label {
                Some(label) => out.push_str(label),
                None => {
                    out.push('@');
                    out.push_str(special.split('^').next().unwrap_or(special));
                }
            }
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
