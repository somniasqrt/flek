//! Teams, users and conversations.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};

use crate::api::{Conversation, ConversationKind, User};
use crate::Result;

/// Inserts or updates the team row.
pub fn upsert_team(
    conn: &Connection,
    id: &str,
    name: &str,
    url: &str,
    self_user: &str,
) -> Result<()> {
    let domain = url
        .trim_start_matches("https://")
        .split('.')
        .next()
        .filter(|d| !d.is_empty());
    conn.execute(
        "INSERT INTO teams (id, name, domain, url, self_user_id) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, domain = excluded.domain,
            url = excluded.url, self_user_id = excluded.self_user_id",
        params![id, name, domain, url, self_user],
    )?;
    Ok(())
}

/// Upserts users. Users from other teams (shared channels) get a stub team row.
pub fn upsert_users(conn: &mut Connection, default_team: &str, users: &[User]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stub_team = tx.prepare("INSERT OR IGNORE INTO teams (id, name) VALUES (?1, '')")?;
        let mut upsert = tx.prepare(
            "INSERT INTO users (id, team_id, name, real_name, display_name, is_bot, deleted, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                team_id = excluded.team_id, name = excluded.name, real_name = excluded.real_name,
                display_name = excluded.display_name, is_bot = excluded.is_bot,
                deleted = excluded.deleted, raw_json = excluded.raw_json",
        )?;
        for u in users {
            let team = u.team_id.as_deref().unwrap_or(default_team);
            stub_team.execute([team])?;
            let real_name = Some(u.profile.real_name.as_str())
                .filter(|s| !s.is_empty())
                .or(u.real_name.as_deref());
            upsert.execute(params![
                u.id,
                team,
                u.name,
                real_name,
                u.label(),
                u.is_bot,
                u.deleted,
                u.raw.to_string()
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Upserts conversations and computes their labels from the users table.
pub fn upsert_conversations(
    conn: &mut Connection,
    team_id: &str,
    self_user: &str,
    convs: &[Conversation],
) -> Result<()> {
    let users = user_labels(conn)?;
    let tx = conn.transaction()?;
    {
        let mut upsert = tx.prepare(
            "INSERT INTO conversations
                (id, team_id, kind, name, label, dm_user_id, topic, purpose, is_archived, is_member, created, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind, name = excluded.name, label = excluded.label,
                dm_user_id = excluded.dm_user_id, topic = excluded.topic, purpose = excluded.purpose,
                is_archived = excluded.is_archived, is_member = 1, created = excluded.created,
                raw_json = excluded.raw_json",
        )?;
        for c in convs {
            let label = conversation_label(c, self_user, &users);
            upsert.execute(params![
                c.id,
                team_id,
                c.kind().as_str(),
                c.name,
                label,
                c.user,
                Some(c.topic.value.as_str()).filter(|s| !s.is_empty()),
                Some(c.purpose.value.as_str()).filter(|s| !s.is_empty()),
                c.is_archived,
                c.created,
                c.raw.to_string()
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

struct UserLabels {
    by_id: HashMap<String, String>,
    by_handle: HashMap<String, (String, String)>,
}

fn user_labels(conn: &Connection) -> Result<UserLabels> {
    let mut stmt = conn.prepare("SELECT id, name, COALESCE(display_name, name) FROM users")?;
    let mut by_id = HashMap::new();
    let mut by_handle = HashMap::new();
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (id, handle, label) = row?;
        by_handle.insert(handle, (id.clone(), label.clone()));
        by_id.insert(id, label);
    }
    Ok(UserLabels { by_id, by_handle })
}

fn conversation_label(c: &Conversation, self_user: &str, users: &UserLabels) -> String {
    match c.kind() {
        ConversationKind::Im => c
            .user
            .as_deref()
            .map(|u| match users.by_id.get(u) {
                Some(label) => label.clone(),
                // Slack's system DM; not a real user, so users.list skips it.
                None if u == "USLACK" => "Slack".to_owned(),
                None => u.to_owned(),
            })
            .unwrap_or_else(|| c.id.clone()),
        ConversationKind::Mpim => {
            // Slack names group DMs `mpdm-alice--bob--carol-1`.
            let name = c.name.as_deref().unwrap_or_default();
            let inner = name.strip_prefix("mpdm-").unwrap_or(name);
            let inner = inner.rsplit_once('-').map_or(inner, |(head, _)| head);
            let names: Vec<String> = inner
                .split("--")
                .filter(|h| !h.is_empty())
                .filter_map(|h| match users.by_handle.get(h) {
                    Some((id, _)) if id == self_user => None,
                    Some((_, label)) => Some(label.clone()),
                    None => Some(h.to_owned()),
                })
                .collect();
            if names.is_empty() {
                name.to_owned()
            } else {
                names.join(", ")
            }
        }
        _ => c.name.clone().unwrap_or_else(|| c.id.clone()),
    }
}

/// One row for listing.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConversationRow {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub is_archived: bool,
    pub topic: Option<String>,
    pub messages: i64,
    /// Top-level messages from others after Slack's read marker; `None`
    /// when the marker is unknown.
    pub unread: Option<i64>,
    /// Newest stored message, for ordering by activity.
    pub latest_ts: Option<String>,
}

/// Conversations of a team: channels first, then group DMs and DMs, by label.
/// Counts come from stored counters (migration 0007), never from `COUNT(*)`.
pub fn list_conversations(conn: &Connection, team_id: &str) -> Result<Vec<ConversationRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, COALESCE(label, name, id), is_archived, topic,
                message_count, unread_count, latest_ts
         FROM conversations
         WHERE team_id = ?1
         ORDER BY CASE kind WHEN 'public_channel' THEN 0 WHEN 'private_channel' THEN 0 WHEN 'mpim' THEN 1 ELSE 2 END,
                  is_archived, lower(COALESCE(label, name, id))",
    )?;
    let rows = stmt
        .query_map([team_id], |r| {
            Ok(ConversationRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                label: r.get(2)?,
                is_archived: r.get(3)?,
                topic: r.get(4)?,
                messages: r.get(5)?,
                unread: r.get(6)?,
                latest_ts: r.get(7)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Display label of one user, if known.
pub fn user_label(conn: &Connection, user_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT COALESCE(display_name, name) FROM users WHERE id = ?1",
            [user_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// Finds a conversation by id, name or label (`#general`, `general`, `Bob`).
pub fn find_conversation(
    conn: &Connection,
    team_id: &str,
    query: &str,
) -> Result<Option<ConversationRow>> {
    let q = query.trim_start_matches(['#', '@']);
    Ok(list_conversations(conn, team_id)?
        .into_iter()
        .find(|r| r.id.eq_ignore_ascii_case(q) || r.label.eq_ignore_ascii_case(q)))
}

/// Stores Slack's read markers (`client.counts`, later `channel_marked` events).
pub fn set_read_markers(conn: &mut Connection, markers: &[(String, Option<String>)]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare("UPDATE conversations SET last_read = ?2 WHERE id = ?1")?;
        for (id, last_read) in markers {
            stmt.execute(params![id, last_read])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// `(id, label)` of a team's conversations, most recently active first
/// (then never-synced ones), so fresh conversations are synced first.
pub fn conversation_activity(conn: &Connection, team_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, COALESCE(c.label, c.name, c.id) FROM conversations c
         LEFT JOIN sync_state s ON s.conversation_id = c.id
         WHERE c.team_id = ?1 AND c.is_member = 1
         ORDER BY s.newest_ts IS NULL, s.newest_ts DESC, c.is_archived, lower(COALESCE(c.label, c.name, c.id))",
    )?;
    let rows = stmt
        .query_map([team_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}
