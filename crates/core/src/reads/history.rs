//! Offline history reads.

use crate::{references, views, Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

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

const VISIBLE: &str = "
    deleted = 0
    AND ( (?2 IS NULL
            AND (thread_ts IS NULL
                OR thread_ts = ts
                OR subtype = 'thread_broadcast'))
        OR (?2 IS NOT NULL
            AND (ts = ?2
                OR thread_ts = ?2)))
";

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
    let mut filter = String::from(
        "
            conversation_id = ?1
            AND deleted = 0
        ",
    );
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
        format!(
            "
                SELECT ts
                FROM messages
                WHERE {filter}
                AND thread_ts = ?2
                UNION
                SELECT ts
                FROM messages
                WHERE {filter}
                AND ts = ?2
            "
        )
    } else {
        format!(
            "
                SELECT ts
                FROM messages
                WHERE {filter}
                AND (thread_ts IS NULL
                    OR thread_ts = ts
                    OR subtype = 'thread_broadcast')
            "
        )
    };
    let order = if newer { "ASC" } else { "DESC" };
    values.push(Value::Integer(i64::try_from(limit).unwrap_or(i64::MAX)));
    sql.push_str(&format!(
        "
            ORDER BY ts {order}
            LIMIT ?{}
        ",
        values.len()
    ));
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
                "
                    SELECT EXISTS (
                        SELECT 1
                        FROM messages
                        WHERE conversation_id = ?1
                        AND {VISIBLE}
                        AND ts = ?5
                        AND (?3 IS NULL
                            OR ts >= ?3)
                        AND (?4 IS NULL
                            OR ts <= ?4))
                "
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
        "
            SELECT COALESCE(thread_ts, ts)
            FROM messages
            WHERE conversation_id = ?1
            AND ts = ?2
        ",
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
