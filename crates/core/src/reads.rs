//! Offline read operations shared by agent clients. Every entity comes from SQLite.
use crate::{references, views, Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub fn users(
    c: &Connection,
    team: &str,
    query: Option<&str>,
    bots: bool,
    deleted: bool,
    limit: usize,
) -> Result<Vec<views::User>> {
    let mut stmt = c.prepare(
        "SELECT u.id,u.name,u.display_name,u.real_name,u.is_bot,u.deleted,u.raw_json,
         (SELECT id FROM conversations WHERE dm_user_id=u.id AND team_id=u.team_id ORDER BY id LIMIT 1),
         p.user_id, p.presence, p.dnd_enabled, p.dnd_start, p.dnd_end,
         p.snooze_enabled, p.snooze_end, p.manual_away, p.auto_away, p.online,
         p.connection_count, p.presence_fresh, p.dnd_fresh, p.updated_at
         FROM users u LEFT JOIN presence p ON p.user_id=u.id WHERE u.team_id=?1 AND (?2 OR NOT u.is_bot) AND (?3 OR NOT u.deleted) ORDER BY
         u.name,u.id",
    )?;
    let rows = stmt
        .query_map(params![team, bots, deleted], views::user_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let q = query.unwrap_or("").to_lowercase();
    Ok(rows
        .into_iter()
        .filter(|u| {
            [
                Some(&u.name),
                u.display_name.as_ref(),
                u.real_name.as_ref(),
                u.email.as_ref(),
            ]
            .iter()
            .flatten()
            .any(|s| s.to_lowercase().contains(&q))
        })
        .take(limit)
        .collect())
}

#[derive(Debug, Default, Clone)]
pub struct Window {
    pub since: Option<String>,
    pub until: Option<String>,
    pub around: Option<String>,
    pub cursor: Option<String>,
    pub thread: Option<String>,
    pub limit: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    conversation: String,
    thread: Option<String>,
    since: Option<String>,
    until: Option<String>,
    direction: String,
    ts: String,
}
// Hex-encoded JSON is intentionally opaque and needs no extra dependency.
fn encode(c: &Cursor) -> String {
    serde_json::to_vec(c)
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn decode(s: &str) -> Result<Cursor> {
    let bad = || Error::Usage("invalid history cursor".into());
    if s.len() > 8192 || !s.len().is_multiple_of(2) || !s.is_ascii() {
        return Err(bad());
    }
    let bytes = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| bad()))
        .collect::<Result<Vec<_>>>()?;
    let c: Cursor = serde_json::from_slice(&bytes).map_err(|_| bad())?;
    if c.version != 1
        || !references::valid_ts(&c.ts)
        || !matches!(c.direction.as_str(), "before" | "after")
    {
        return Err(bad());
    }
    Ok(c)
}
#[derive(Serialize)]
pub struct Page {
    pub messages: Vec<views::Message>,
    pub next_cursor: Option<String>,
    pub prev_cursor: Option<String>,
}
const VISIBLE: &str = "deleted = 0 AND (
    (?2 IS NULL AND (thread_ts IS NULL OR thread_ts = ts OR subtype = 'thread_broadcast'))
    OR (?2 IS NOT NULL AND (ts = ?2 OR thread_ts = ?2)))";
fn timestamps(
    c: &Connection,
    id: &str,
    w: &Window,
    bound: Option<&str>,
    newer: bool,
    limit: usize,
) -> Result<Vec<String>> {
    use rusqlite::types::Value;
    let mut values = vec![Value::Text(id.into())];
    let mut filter = String::from("conversation_id=?1 AND deleted=0");
    if let Some(root) = &w.thread {
        values.push(Value::Text(root.clone()));
    }
    for (op, value) in [
        (">=", w.since.as_deref()),
        ("<=", w.until.as_deref()),
        (if newer { ">" } else { "<" }, bound),
    ] {
        if let Some(value) = value {
            values.push(Value::Text(value.into()));
            filter.push_str(&format!(" AND ts {op} ?{}", values.len()));
        }
    }
    let mut sql = if w.thread.is_some() {
        format!("SELECT ts FROM messages WHERE {filter} AND thread_ts=?2 UNION SELECT ts FROM messages WHERE {filter} AND ts=?2")
    } else {
        format!("SELECT ts FROM messages WHERE {filter} AND (thread_ts IS NULL OR thread_ts=ts OR subtype='thread_broadcast')")
    };
    let order = if newer { "ASC" } else { "DESC" };
    values.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
    sql.push_str(&format!(" ORDER BY ts {order} LIMIT ?{}", values.len()));
    let mut stmt = c.prepare(&sql)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(values), |r| r.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}
pub fn history(c: &Connection, id: &str, w: &Window) -> Result<Page> {
    if w.since
        .as_ref()
        .zip(w.until.as_ref())
        .is_some_and(|(a, b)| a > b)
    {
        return Err(Error::Usage("--since must not exceed --until".into()));
    }
    if w.around.is_some() && w.cursor.is_some() {
        return Err(Error::Usage("--around conflicts with --cursor".into()));
    }
    if w.thread
        .as_deref()
        .is_some_and(|s| !references::valid_ts(s))
        || w.around
            .as_deref()
            .is_some_and(|s| !references::valid_ts(s))
    {
        return Err(Error::Usage("invalid Slack timestamp".into()));
    }
    let cursor = w.cursor.as_deref().map(decode).transpose()?;
    if cursor.as_ref().is_some_and(|k| {
        k.conversation != id || k.thread != w.thread || k.since != w.since || k.until != w.until
    }) {
        return Err(Error::Usage("cursor does not match history window".into()));
    }
    let mut times = if let Some(target) = &w.around {
        let mut before = timestamps(c, id, w, Some(target), false, w.limit / 2)?;
        before.reverse();
        let exists: bool = c.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM messages
                 WHERE conversation_id = ?1 AND {VISIBLE} AND ts = ?5
                   AND (?3 IS NULL OR ts >= ?3) AND (?4 IS NULL OR ts <= ?4))"
            ),
            params![id, w.thread, w.since, w.until, target],
            |r| r.get(0),
        )?;
        if exists && before.len() < w.limit {
            before.push(target.clone());
        }
        before.extend(timestamps(
            c,
            id,
            w,
            Some(target),
            true,
            w.limit.saturating_sub(before.len()),
        )?);
        if before.len() < w.limit {
            let mut extra = timestamps(
                c,
                id,
                w,
                before.first().map(String::as_str),
                false,
                w.limit - before.len(),
            )?;
            extra.reverse();
            extra.extend(before);
            before = extra;
        }
        before
    } else {
        timestamps(
            c,
            id,
            w,
            cursor.as_ref().map(|k| k.ts.as_str()),
            cursor.as_ref().is_some_and(|k| k.direction == "after"),
            w.limit,
        )?
    };
    times.sort();
    times.dedup();
    let make = |direction: &str, ts: &str| {
        encode(&Cursor {
            version: 1,
            conversation: id.into(),
            thread: w.thread.clone(),
            since: w.since.clone(),
            until: w.until.clone(),
            direction: direction.into(),
            ts: ts.into(),
        })
    };
    let next_cursor = match times.first() {
        Some(ts) if !timestamps(c, id, w, Some(ts), false, 1)?.is_empty() => {
            Some(make("before", ts))
        }
        _ => None,
    };
    let prev_cursor = match times.last() {
        Some(ts) if !timestamps(c, id, w, Some(ts), true, 1)?.is_empty() => Some(make("after", ts)),
        _ => None,
    };
    let keys = times
        .into_iter()
        .map(|ts| (id.to_owned(), ts))
        .collect::<Vec<_>>();
    Ok(Page {
        messages: views::messages(c, &keys)?,
        next_cursor,
        prev_cursor,
    })
}

pub fn thread_root(c: &Connection, id: &str, ts: &str) -> Result<String> {
    Ok(c.query_row(
        "SELECT COALESCE(thread_ts,ts) FROM messages WHERE conversation_id=?1 AND ts=?2",
        params![id, ts],
        |r| r.get(0),
    )
    .optional()?
    .unwrap_or_else(|| ts.into()))
}
pub fn thread(c: &Connection, id: &str, ts: &str) -> Result<Vec<views::Message>> {
    let root = thread_root(c, id, ts)?;
    let page = history(
        c,
        id,
        &Window {
            thread: Some(root),
            limit: usize::MAX,
            ..Default::default()
        },
    )?;
    if page.messages.is_empty() {
        return Err(Error::NotFound(format!("no stored thread {id} {ts}")));
    }
    Ok(page.messages)
}
#[derive(Serialize)]
pub struct Context {
    pub message: views::Message,
    pub before: Vec<views::Message>,
    pub after: Vec<views::Message>,
    pub conversation: views::Conversation,
}
pub fn message(c: &Connection, id: &str, ts: &str, n: usize) -> Result<Context> {
    let message = views::message(c, id, ts)?
        .filter(|m| !m.deleted)
        .ok_or_else(|| Error::NotFound(format!("no stored message {id} {ts}")))?;
    let w = Window {
        thread: message.thread_ts.clone(),
        limit: n,
        ..Default::default()
    };
    let mut before = timestamps(c, id, &w, Some(ts), false, n)?;
    before.reverse();
    let after = timestamps(c, id, &w, Some(ts), true, n)?;
    let hydrate = |times: Vec<String>| {
        views::messages(
            c,
            &times
                .into_iter()
                .map(|ts| (id.to_owned(), ts))
                .collect::<Vec<_>>(),
        )
    };
    Ok(Context {
        message,
        before: hydrate(before)?,
        after: hydrate(after)?,
        conversation: views::conversation(c, id)?
            .ok_or_else(|| references::conversation_not_found(id))?,
    })
}

#[derive(Serialize)]
pub struct Unread {
    #[serde(flatten)]
    pub conversation: views::Conversation,
    pub mentions: i64,
    pub first_unread_ts: Option<String>,
    pub messages: Vec<views::Message>,
}
pub fn unread(
    c: &Connection,
    team: &str,
    n: usize,
) -> Result<(Vec<Unread>, Vec<views::Conversation>)> {
    let me: Option<String> = c
        .query_row("SELECT self_user_id FROM teams WHERE id=?1", [team], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    let token = format!("\"{}\"", me.unwrap_or_default().replace('"', "\"\""));
    let mut stmt = c.prepare(
        "WITH mention_counts AS MATERIALIZED (
         SELECT m.conversation_id,COUNT(*) AS mentions FROM messages_fts f JOIN messages m ON m.id=f.rowid
         JOIN conversations c ON c.id=m.conversation_id JOIN teams t ON t.id=c.team_id
         WHERE messages_fts MATCH ?2 AND c.team_id=?1 AND c.unread_count>0 AND m.deleted=0 AND m.ts>c.last_read
         AND (m.thread_ts IS NULL OR m.thread_ts=m.ts OR m.subtype IS 'thread_broadcast') AND m.user_id IS
         NOT t.self_user_id
         AND instr(m.text,'<@'||t.self_user_id||'>')>0 GROUP BY m.conversation_id
         ) SELECT c.id,COALESCE(mc.mentions,0),
         (SELECT m.ts FROM messages m WHERE m.conversation_id=c.id AND m.deleted=0 AND m.ts>c.last_read
         AND (m.thread_ts IS NULL OR m.thread_ts=m.ts OR m.subtype IS 'thread_broadcast') AND m.user_id IS
         NOT t.self_user_id ORDER BY m.ts LIMIT 1)
         FROM conversations c JOIN teams t ON t.id=c.team_id LEFT JOIN mention_counts mc ON
         mc.conversation_id=c.id
         WHERE c.team_id=?1 AND c.unread_count>0
         ORDER BY CASE WHEN c.kind IN ('im','mpim') THEN 0 WHEN mc.mentions>0 THEN 1 ELSE 2 END,c.latest_ts
         DESC,c.id",
    )?;
    let rows = stmt
        .query_map(params![team, token], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let all = views::conversations(c, team, true)?;
    let mut attached: std::collections::BTreeMap<String, Vec<views::Message>> =
        std::collections::BTreeMap::new();
    if n > 0 {
        let mut stmt = c.prepare(
            "SELECT c.id,m.ts FROM conversations c JOIN teams t ON t.id=c.team_id
             JOIN messages m ON m.id IN (SELECT m2.id FROM messages m2 WHERE m2.conversation_id=c.id
             AND m2.ts>c.last_read AND m2.deleted=0 AND (m2.thread_ts IS NULL OR m2.thread_ts=m2.ts OR m2.subtype
             IS 'thread_broadcast')
             AND m2.user_id IS NOT t.self_user_id ORDER BY m2.ts LIMIT ?2)
             WHERE c.team_id=?1 AND c.unread_count>0 ORDER BY c.id,m.ts",
        )?;
        let keys = stmt
            .query_map(params![team, i64::try_from(n).unwrap_or(i64::MAX)], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for message in views::messages(c, &keys)? {
            attached
                .entry(message.conversation_id.clone())
                .or_default()
                .push(message);
        }
    }
    let by_id: std::collections::BTreeMap<_, _> = all.iter().map(|c| (c.id.as_str(), c)).collect();
    let known = rows
        .into_iter()
        .map(|(id, mentions, first_unread_ts)| Unread {
            conversation: by_id[id.as_str()].clone(),
            mentions,
            first_unread_ts,
            messages: attached.remove(&id).unwrap_or_default(),
        })
        .collect();
    Ok((
        known,
        all.into_iter()
            .filter(|x| x.unread_count.is_none())
            .collect(),
    ))
}

/// The indexed mention candidate source, shared by mentions and Activity.
/// Arguments are internal SQL expressions, never user input.
pub(crate) fn mention_source(terms: &str, team: &str, me: &str, broadcasts: &str) -> String {
    format!(
        "FROM messages_fts f JOIN messages m ON m.id=f.rowid
         JOIN conversations c ON c.id=m.conversation_id
         WHERE messages_fts MATCH {terms} AND c.team_id={team} AND m.deleted=0
           AND m.user_id IS NOT {me}
           AND (instr(m.text,'<@'||{me}||'>')>0 OR ({broadcasts} AND (
               instr(m.text,'<!here>')>0 OR instr(m.text,'<!channel>')>0
               OR instr(m.text,'<!everyone>')>0)))"
    )
}

pub fn mentions(
    c: &Connection,
    team: &str,
    since: &str,
    until: Option<&str>,
    broadcasts: bool,
    limit: usize,
) -> Result<Vec<views::Message>> {
    let me: Option<String> = c
        .query_row("SELECT self_user_id FROM teams WHERE id=?1", [team], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    let me = me.ok_or_else(|| Error::NotFound("no stored self user id for mentions".into()))?;
    let mut terms = vec![format!("\"{}\"", me.replace('"', "\"\""))];
    if broadcasts {
        terms.extend(["here", "channel", "everyone"].map(|x| format!("\"{x}\"")));
    }
    let source = mention_source("?1", "?2", "?3", "?6");
    let mut stmt = c.prepare(&format!(
        "SELECT m.conversation_id,m.ts {source}
         AND m.ts>=?4 AND (?5 IS NULL OR m.ts<=?5)
         ORDER BY m.ts DESC,m.conversation_id LIMIT ?7"
    ))?;
    let keys = stmt
        .query_map(
            params![
                terms.join(" OR "),
                team,
                me,
                since,
                until,
                broadcasts,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    views::messages(c, &keys)
}

#[derive(Serialize)]
pub struct File {
    #[serde(flatten)]
    pub file: views::File,
    pub conversation_id: Option<String>,
    pub ts: Option<String>,
    pub user_id: Option<String>,
    pub permalink: Option<String>,
    pub hidden_by_limit: bool,
    pub title: Option<String>,
}
#[derive(Default)]
pub struct FileFilter {
    pub id: Option<String>,
    pub channel: Option<String>,
    pub user: Option<String>,
    pub kind: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: usize,
    pub query: Option<String>,
    pub sort: Option<String>,
    pub mine: bool,
    pub offset: usize,
    pub other_includes_media: bool,
}
pub fn files(c: &Connection, team: &str, f: &FileFilter) -> Result<Vec<File>> {
    if f.kind.as_deref().is_some_and(|k| {
        !["image", "pdf", "video", "audio", "doc", "other"].contains(&k)
            && (!k.contains('/') || k.chars().any(char::is_whitespace))
    }) {
        return Err(Error::Usage(
            "--type expects image, pdf, video, audio, doc, other or a mimetype prefix".into(),
        ));
    }

    let mut stmt = c.prepare(
        "SELECT
         f.id,f.name,f.mimetype,f.size,f.local_path,m.conversation_id,
         m.ts,m.user_id,t.url,f.hidden_by_limit,m.thread_ts,f.title
         FROM message_files mf JOIN files f ON f.id=mf.file_id JOIN messages m ON
         m.conversation_id=mf.conversation_id AND m.ts=mf.ts JOIN conversations c ON c.id=m.conversation_id
         JOIN teams t ON t.id=c.team_id
         WHERE c.team_id=?1 AND m.deleted=0 AND (?2 IS NULL OR c.id=?2) AND (?3 IS NULL OR m.user_id=?3) AND
         (?4 IS NULL OR m.ts>=?4) AND (?5 IS NULL OR m.ts<=?5) AND (?6 IS NULL OR f.id=?6)
         AND (?7 IS NULL OR (instr(?7,'/')>0 AND substr(COALESCE(f.mimetype,''),1,length(?7))=?7)
         OR CASE
         WHEN f.mimetype LIKE 'image/%' THEN 'image'
         WHEN f.mimetype='application/pdf' THEN 'pdf'
         WHEN f.mimetype LIKE 'video/%' THEN 'video'
         WHEN f.mimetype LIKE 'audio/%' THEN 'audio'
         WHEN f.mimetype LIKE 'text/%' OR f.mimetype LIKE '%word%' OR f.mimetype LIKE '%document%'
         OR f.mimetype LIKE '%excel%' OR f.mimetype LIKE '%spreadsheet%' OR f.mimetype LIKE '%presentation%'
         OR f.mimetype LIKE '%powerpoint%' THEN 'doc'
         ELSE 'other' END=?7
         OR (?13 AND ?7='other' AND (f.mimetype LIKE 'video/%' OR f.mimetype LIKE 'audio/%')))
         AND (?9 IS NULL OR instr(lower(COALESCE(f.name,'')||' '||COALESCE(f.title,'')),lower(?9))>0)
         AND (NOT ?10 OR m.user_id=t.self_user_id)
         ORDER BY CASE WHEN ?11='name' THEN lower(COALESCE(f.title,f.name)) END,
                  CASE WHEN ?11='size' THEN f.size END DESC,m.ts DESC,f.id
         LIMIT ?8 OFFSET ?12",
    )?;
    let rows = stmt.query_map(
        params![
            team,
            f.channel,
            f.user,
            f.since,
            f.until,
            f.id,
            f.kind,
            i64::try_from(f.limit).unwrap_or(i64::MAX),
            f.query,
            f.mine,
            f.sort,
            f.offset as i64,
            f.other_includes_media
        ],
        |r| {
            let id: String = r.get(5)?;
            let ts: String = r.get(6)?;
            let url: Option<String> = r.get(8)?;
            let thread: Option<String> = r.get::<_, Option<String>>(10)?.filter(|root| root != &ts);
            Ok(File {
                file: views::File {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    mimetype: r.get(2)?,
                    size: r.get(3)?,
                    local_path: r
                        .get::<_, Option<String>>(4)?
                        .filter(|p| std::path::Path::new(p).is_file()),
                },
                permalink: url.map(|u| {
                    let mut link = format!(
                        "{}/archives/{id}/p{}",
                        u.trim_end_matches('/'),
                        ts.replace('.', "")
                    );
                    if let Some(root) = thread {
                        link.push_str(&format!("?thread_ts={root}&cid={id}"));
                    }
                    link
                }),
                conversation_id: Some(id),
                ts: Some(ts),
                user_id: r.get(7)?,
                hidden_by_limit: r.get(9)?,
                title: r.get(11)?,
            })
        },
    )?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

#[derive(Serialize)]
pub struct ChannelInfo {
    pub conversation: views::Conversation,
    pub created: Option<i64>,
    pub creator: Option<String>,
    pub num_members: Option<i64>,
    pub members: Option<Vec<views::User>>,
    pub pins: Option<Vec<views::Message>>,
    pub members_synced_at: Option<String>,
}
pub fn channel_info(c: &Connection, id: &str) -> Result<ChannelInfo> {
    let conversation =
        views::conversation(c, id)?.ok_or_else(|| references::conversation_not_found(id))?;
    let (created, raw): (Option<i64>, Option<String>) = c.query_row(
        "SELECT created,raw_json FROM conversations WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let raw: serde_json::Value = raw
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_default();
    let synced: Option<i64> = c
        .query_row(
            "SELECT synced_at FROM conversation_details_sync WHERE conversation_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let members = if synced.is_some() {
        let mut stmt = c.prepare(
            "SELECT
             COALESCE(u.id,cm.user_id),COALESCE(u.name,cm.user_id),u.display_name,u.real_name,COALESCE(u.is_bot,0),COALESCE(u.deleted,0),u.raw_json,
             (SELECT id FROM conversations WHERE dm_user_id=u.id AND team_id=u.team_id ORDER BY id LIMIT 1),
         p.user_id, p.presence, p.dnd_enabled, p.dnd_start, p.dnd_end,
         p.snooze_enabled, p.snooze_end, p.manual_away, p.auto_away, p.online,
         p.connection_count, p.presence_fresh, p.dnd_fresh, p.updated_at
             FROM conversation_members cm LEFT JOIN users u ON u.id=cm.user_id LEFT JOIN presence p ON p.user_id=cm.user_id WHERE cm.conversation_id=?1 ORDER
             BY u.name,u.id",
        )?;
        let users = stmt
            .query_map([id], views::user_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Some(users)
    } else {
        None
    };
    let pins = if synced.is_some() {
        let mut stmt = c.prepare("SELECT ts FROM pins WHERE conversation_id=?1 ORDER BY ts")?;
        let keys = stmt
            .query_map([id], |r| Ok((id.to_owned(), r.get(0)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Some(views::messages(c, &keys)?)
    } else {
        None
    };
    Ok(ChannelInfo {
        conversation,
        created,
        creator: raw["creator"].as_str().map(str::to_owned),
        num_members: raw["num_members"].as_i64(),
        members,
        pins,
        members_synced_at: synced
            .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
            .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
    })
}

/// A file may have multiple shares; choose the newest one in this team.
pub fn file(c: &Connection, team: &str, id: &str) -> Result<File> {
    let mut f = files(
        c,
        team,
        &FileFilter {
            id: Some(id.into()),
            limit: 1,
            ..Default::default()
        },
    )?
    .into_iter()
    .find(|f| f.file.id == id)
    .ok_or_else(|| Error::NotFound(format!("no stored file {id:?}")))?;
    if f.hidden_by_limit {
        return Err(Error::NotFound(format!(
            "file {id:?} is hidden by the Slack Free limit"
        )));
    }
    if f.file
        .local_path
        .as_ref()
        .is_some_and(|p| !std::path::Path::new(p).is_file())
    {
        f.file.local_path = None;
    }
    Ok(f)
}
pub fn pending_file(c: &Connection, id: &str) -> Result<crate::store::files::PendingDownload> {
    c.query_row(
        "SELECT id, name, mimetype, size,
                COALESCE(url_private_download, url_private), local_path
         FROM files WHERE id = ?1 AND hidden_by_limit = 0 AND is_external = 0
           AND COALESCE(url_private_download, url_private) IS NOT NULL",
        [id],
        |r| {
            Ok(crate::store::files::PendingDownload {
                id: r.get(0)?,
                name: r.get(1)?,
                mimetype: r.get(2)?,
                size: r.get(3)?,
                url: r.get(4)?,
                local_path: r.get(5)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("file {id:?} has no downloadable stored URL")))
}
