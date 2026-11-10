//! Unambiguous references shared by CLI, app and future integrations.
use crate::store::directory::ConversationRow;
use crate::{Error, Result};
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub id: String,
    pub label: String,
    pub kind: String,
}

pub fn kind(kind: &str) -> &str {
    match kind {
        "public_channel" => "channel",
        "private_channel" => "private",
        other => other,
    }
}

#[derive(Debug)]
pub enum Resolution {
    Found(ConversationRow),
    NotFound,
    Ambiguous(Vec<Candidate>),
}

impl Resolution {
    pub fn required(self, query: &str) -> Result<ConversationRow> {
        match self {
            Self::Found(c) => Ok(c),
            Self::NotFound => Err(conversation_not_found(query)),
            Self::Ambiguous(candidates) => Err(Error::Ambiguous { candidates }),
        }
    }
}

/// Matching policy: writes require a complete identity, reads may use prefixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Exact,
    AllowPrefix,
}

pub fn conversation_not_found(query: &str) -> Error {
    Error::NotFound(format!(
        "no conversation matches {query:?}; try `flek channels`"
    ))
}

/// Exact id > exact alias > Unicode case-insensitive alias > prefix.
/// Sigils restrict the namespace to channels or personal DMs.
pub fn resolve(conn: &Connection, team: &str, query: &str, mode: Match) -> Result<Resolution> {
    let link = parse_permalink(query);
    let query = link
        .as_ref()
        .map_or(query.trim(), |r| r.conversation_id.as_str());
    if query.starts_with("https://") {
        return Ok(Resolution::NotFound);
    }
    let q = query.trim_start_matches(['#', '@']);
    if q.is_empty() {
        return Ok(Resolution::NotFound);
    }
    let mut best = 5;
    let mut found = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT c.id, c.kind, COALESCE(c.label,c.name,c.id), c.is_archived, c.topic,
                c.message_count, c.unread_count, c.latest_ts, c.dm_user_id,
                c.name, u.name, u.display_name, u.real_name,
                CASE WHEN json_valid(u.raw_json) THEN json_extract(u.raw_json,'$.profile.email') END
         FROM conversations c LEFT JOIN users u ON u.id=c.dm_user_id
         WHERE c.team_id=?1
         ORDER BY CASE c.kind WHEN 'public_channel' THEN 0 WHEN 'private_channel' THEN 0 WHEN 'mpim' THEN 1 ELSE 2 END,
                  c.is_archived, lower(COALESCE(c.label,c.name,c.id))"
    )?;
    let rows = stmt.query_map([team], |r| {
        let row = ConversationRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            label: r.get(2)?,
            is_archived: r.get(3)?,
            topic: r.get(4)?,
            messages: r.get(5)?,
            unread: r.get(6)?,
            latest_ts: r.get(7)?,
            dm_user_id: r.get(8)?,
        };
        let mut aliases = vec![row.label.clone()];
        for i in 9..14 {
            aliases.extend(r.get::<_, Option<String>>(i)?);
        }
        Ok((row, aliases))
    })?;
    let folded = q.to_lowercase();
    for entry in rows {
        let (row, aliases) = entry?;
        if query.starts_with('@') && row.kind != "im" {
            continue;
        }
        if query.starts_with('#')
            && !matches!(row.kind.as_str(), "public_channel" | "private_channel")
        {
            continue;
        }
        let rank = if row.id == q {
            0
        } else if aliases.iter().any(|a| a == q) {
            1
        } else if row.id.to_lowercase() == folded
            || aliases.iter().any(|a| a.to_lowercase() == folded)
        {
            2
        } else if aliases
            .iter()
            .any(|a| a.to_lowercase().starts_with(&folded))
        {
            3
        } else {
            continue;
        };
        if rank < best {
            found.clear();
            best = rank;
        }
        if rank == best {
            found.push(row);
        }
    }
    Ok(match found.len() {
        0 => Resolution::NotFound,
        1 if best <= 2 || mode == Match::AllowPrefix => Resolution::Found(found.remove(0)),
        _ => Resolution::Ambiguous(
            found
                .into_iter()
                .map(|r| Candidate {
                    id: r.id,
                    label: r.label,
                    kind: kind(&r.kind).to_owned(),
                })
                .collect(),
        ),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MessageReference {
    pub conversation_id: String,
    pub ts: String,
    pub thread_ts: Option<String>,
}

pub fn valid_ts(ts: &str) -> bool {
    ts.split_once('.').is_some_and(|(s, us)| {
        !s.is_empty()
            && s.bytes().all(|b| b.is_ascii_digit())
            && us.len() == 6
            && us.bytes().all(|b| b.is_ascii_digit())
    })
}

pub fn parse_permalink(input: &str) -> Option<MessageReference> {
    let url = reqwest::Url::parse(input).ok()?;
    if url.scheme() != "https"
        || !url.host_str()?.ends_with(".slack.com")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let parts: Vec<_> = url.path_segments()?.collect();
    if parts.len() != 3
        || parts[0] != "archives"
        || !parts[1].starts_with(['C', 'G', 'D'])
        || !parts[1].bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    let digits = parts[2].strip_prefix('p')?;
    if digits.len() != 16 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let thread_ts = url
        .query_pairs()
        .find(|(k, _)| k == "thread_ts")
        .map(|(_, v)| v.into_owned());
    if thread_ts.as_deref().is_some_and(|t| !valid_ts(t)) {
        return None;
    }
    Some(MessageReference {
        conversation_id: parts[1].into(),
        ts: format!("{}.{}", &digits[..10], &digits[10..]),
        thread_ts,
    })
}

/// Parse a permalink, or resolve a conversation plus an explicit timestamp.
pub fn message_reference(
    conn: &Connection,
    team: &str,
    reference: &str,
    ts: Option<&str>,
) -> Result<MessageReference> {
    if ts.is_none() {
        return parse_permalink(reference).ok_or_else(|| {
            Error::NotFound(format!("invalid Slack message permalink {reference:?}"))
        });
    }
    let ts = ts.unwrap();
    if !valid_ts(ts) {
        return Err(Error::Usage(
            "expected a Slack timestamp (seconds.microseconds)".into(),
        ));
    }
    let conv = resolve(conn, team, reference, Match::AllowPrefix)?.required(reference)?;
    Ok(MessageReference {
        conversation_id: conv.id,
        ts: ts.into(),
        thread_ts: None,
    })
}

/// Resolve a stored person using the same ranked identity policy as conversations.
pub fn resolve_user(
    conn: &Connection,
    team: &str,
    query: &str,
    mode: Match,
) -> Result<crate::views::User> {
    let q = query.trim().trim_start_matches('@');
    let folded = q.to_lowercase();
    let users = crate::reads::users(conn, team, None, true, true, usize::MAX)?;
    let mut best = 5;
    let mut found = Vec::new();
    for u in users {
        if q.is_empty() {
            break;
        }
        let aliases = [
            Some(u.name.as_str()),
            u.display_name.as_deref(),
            u.real_name.as_deref(),
            u.email.as_deref(),
        ];
        let rank = if u.id == q {
            0
        } else if aliases.iter().flatten().any(|a| *a == q) {
            1
        } else if u.id.to_lowercase() == folded
            || aliases.iter().flatten().any(|a| a.to_lowercase() == folded)
        {
            2
        } else if aliases
            .iter()
            .flatten()
            .any(|a| a.to_lowercase().starts_with(&folded))
        {
            3
        } else {
            continue;
        };
        if rank < best {
            found.clear();
            best = rank;
        }
        if rank == best {
            found.push(u);
        }
    }
    match found.len() {
        0 => Err(Error::NotFound(format!(
            "no user matches {query:?}; try `flek users`"
        ))),
        1 if best <= 2 || mode == Match::AllowPrefix => Ok(found.remove(0)),
        _ => Err(Error::Ambiguous {
            candidates: found
                .into_iter()
                .map(|u| Candidate {
                    id: u.id,
                    label: u.display_name.unwrap_or(u.name),
                    kind: "user".into(),
                })
                .collect(),
        }),
    }
}
