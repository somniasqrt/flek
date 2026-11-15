//! Message-action cache updates: Slack-confirmed pins/stars, durable message
//! links and per-user local bookmarks. No network or policy here.
use crate::{references::valid_ts, Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub fn confirmed(
    c: &mut Connection,
    action: &str,
    channel: &str,
    ts: &str,
    user: &str,
) -> Result<()> {
    match action {
        "pin" => {
            c.execute(
                "INSERT INTO pins (conversation_id, ts, pinned_by, pinned_at)
            VALUES (?1, ?2, ?3, unixepoch()) ON CONFLICT (conversation_id, ts) DO NOTHING",
                params![channel, ts, user],
            )?;
        }
        "unpin" => {
            c.execute(
                "DELETE FROM pins WHERE conversation_id = ?1 AND ts = ?2",
                params![channel, ts],
            )?;
        }
        "save" => {
            // This is a legacy Slack star snapshot, not the modern Later state.
            let n = c.execute(
                "UPDATE messages SET raw_json = json_set(raw_json, '$.is_starred', json('true'))
                WHERE conversation_id = ?1 AND ts = ?2 AND json_valid(raw_json)",
                params![channel, ts],
            )?;
            if n == 0 {
                return Err(crate::Error::NotFound(
                    "saved message is not cached; sync it".into(),
                ));
            }
            // Mirror into the per-user local list shown by the app.
            save(c, channel, ts, user, true, true)?;
        }
        _ => return Err(crate::Error::Forbidden),
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct State {
    pub saved: bool,
    pub saved_local_only: bool,
    pub pinned: bool,
}

pub fn state(c: &Connection, id: &str, ts: &str, user: &str) -> Result<State> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM saved_messages WHERE conversation_id=?1 AND ts=?2 AND user_id=?3),
         COALESCE((SELECT local_only FROM saved_messages WHERE conversation_id=?1 AND ts=?2 AND user_id=?3),1),
         EXISTS(SELECT 1 FROM pins WHERE conversation_id=?1 AND ts=?2)",
        params![id, ts, user],
        |r| Ok(State { saved: r.get(0)?, saved_local_only: r.get(1)?, pinned: r.get(2)? }),
    )?)
}

pub fn require_message(c: &Connection, id: &str, ts: &str) -> Result<()> {
    if !valid_ts(ts) {
        return Err(Error::Usage("invalid message timestamp".into()));
    }
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE conversation_id=?1 AND ts=?2 AND deleted=0)",
        params![id, ts],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(Error::NotFound("message is not stored locally".into()));
    }
    Ok(())
}

/// Cached API URL, then a permalink built from the stored workspace URL/domain.
pub fn permalink(c: &Connection, id: &str, ts: &str) -> Result<Option<String>> {
    if !valid_ts(ts) {
        return Err(Error::Usage("invalid message timestamp".into()));
    }
    let cached: Option<String> = c
        .query_row(
            "SELECT permalink FROM message_permalinks WHERE conversation_id=?1 AND ts=?2",
            params![id, ts],
            |r| r.get(0),
        )
        .optional()?;
    if cached.is_some() {
        return Ok(cached);
    }
    let (url, domain, thread): (Option<String>, Option<String>, Option<String>) = c.query_row(
        "SELECT t.url,t.domain,m.thread_ts FROM conversations c JOIN teams t ON t.id=c.team_id
         LEFT JOIN messages m ON m.conversation_id=c.id AND m.ts=?2 WHERE c.id=?1",
        params![id, ts],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let candidates = [url, domain.map(|d| format!("https://{d}.slack.com"))];
    let Some(mut base) = candidates
        .into_iter()
        .flatten()
        .filter_map(|u| reqwest::Url::parse(&u).ok())
        .find(|u| {
            u.scheme() == "https"
                && u.host_str().is_some_and(|h| h.ends_with(".slack.com"))
                && u.username().is_empty()
                && u.password().is_none()
                && u.port().is_none()
        })
    else {
        return Ok(None);
    };
    base.set_path(&format!("/archives/{id}/p{}", ts.replace('.', "")));
    base.set_query(None);
    base.set_fragment(None);
    if let Some(root) = thread.filter(|t| t != ts && valid_ts(t)) {
        base.query_pairs_mut()
            .append_pair("thread_ts", &root)
            .append_pair("cid", id);
    }
    Ok(Some(base.into()))
}

pub fn save(c: &Connection, id: &str, ts: &str, user: &str, on: bool, local: bool) -> Result<()> {
    if on {
        c.execute(
            "INSERT INTO saved_messages(conversation_id,ts,user_id,local_only) VALUES (?1,?2,?3,?4)
            ON CONFLICT(conversation_id,ts,user_id) DO UPDATE SET local_only=excluded.local_only",
            params![id, ts, user, local],
        )?;
    } else {
        c.execute(
            "DELETE FROM saved_messages WHERE conversation_id=?1 AND ts=?2 AND user_id=?3",
            params![id, ts, user],
        )?;
    }
    Ok(())
}

/// Local saved list, including messages no longer visible in Slack Free.
pub fn saved(
    c: &Connection,
    team: &str,
    user: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<crate::views::Message>> {
    let mut stmt = c.prepare("SELECT s.conversation_id,s.ts FROM saved_messages s JOIN conversations c ON c.id=s.conversation_id
        JOIN messages m ON m.conversation_id=s.conversation_id AND m.ts=s.ts
        WHERE c.team_id=?1 AND s.user_id=?2 AND m.deleted=0 ORDER BY s.saved_at DESC,s.ts DESC,s.conversation_id LIMIT ?3 OFFSET ?4")?;
    let keys = stmt
        .query_map(
            params![
                team,
                user,
                limit.min(500) as i64,
                i64::try_from(offset).unwrap_or(i64::MAX)
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    crate::views::messages(c, &keys)
}
