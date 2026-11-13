//! Conversations storage views.

use crate::api::{Conversation, ConversationKind};
use crate::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// Upserts conversations and computes their labels from the users table.
pub fn upsert_conversations(
    conn: &mut Connection,
    team_id: &str,
    self_user: &str,
    convs: &[Conversation],
) -> Result<()> {
    let tx = conn.transaction()?;
    upsert_rows(&tx, team_id, self_user, convs)?;
    tx.commit()?;
    Ok(())
}

fn upsert_rows(
    conn: &Connection,
    team_id: &str,
    self_user: &str,
    convs: &[Conversation],
) -> Result<()> {
    let users = user_labels(conn)?;
    {
        let mut upsert = conn.prepare(
            "
                INSERT INTO conversations(id, team_id, kind, name, label, dm_user_id, topic, purpose,
                        is_archived, is_member, created, raw_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?11)
                ON CONFLICT (id) DO UPDATE
                SET kind = excluded.kind, name = excluded.name, label = excluded.label, dm_user_id =
                    excluded.dm_user_id, topic = excluded.topic, purpose = excluded.purpose,
                    is_archived = excluded.is_archived, is_member = 1, created = excluded.created,
                    raw_json = excluded.raw_json
            ",
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
    Ok(())
}

/// Reconcile only a complete users.conversations result. The outer transaction
/// also rolls back upserts if membership reconciliation fails. Only channels
/// lose membership: whether Slack lists every DM here is unverified, and a DM
/// dropped by mistake would leave the sidebar.
pub fn replace_memberships(
    conn: &mut Connection,
    team_id: &str,
    self_user: &str,
    convs: &[Conversation],
) -> Result<()> {
    let tx = conn.transaction()?;
    upsert_rows(&tx, team_id, self_user, convs)?;
    let ids: std::collections::HashSet<_> = convs.iter().map(|c| c.id.as_str()).collect();
    let stored: Vec<String> = {
        let mut stmt = tx.prepare(
            "
                SELECT id
                FROM conversations
                WHERE team_id = ?1
                AND is_member = 1
                AND kind IN ('public_channel', 'private_channel')
            ",
        )?;
        let rows = stmt
            .query_map([team_id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    for id in stored {
        if !ids.contains(id.as_str()) {
            tx.execute(
                "UPDATE conversations SET is_member = 0, unread_count = 0 WHERE id = ?1",
                [id],
            )?;
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
    let mut stmt = conn.prepare(
        "
            SELECT id, name, COALESCE(display_name, name)
            FROM users
        ",
    )?;
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
    pub is_member: bool,
    pub last_read: Option<String>,
    pub starred: bool,
    pub notification: String,
    pub is_general: bool,
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
    /// The other person of a DM, for their avatar.
    pub dm_user_id: Option<String>,
}

/// Conversations of a team: channels first, then group DMs and DMs, by label.
/// Counts come from stored counters (migration 0007), never from `COUNT(*)`.
pub fn list_conversations(conn: &Connection, team_id: &str) -> Result<Vec<ConversationRow>> {
    list_conversations_including(conn, team_id, false)
}

pub fn list_conversations_including(
    conn: &Connection,
    team_id: &str,
    non_member: bool,
) -> Result<Vec<ConversationRow>> {
    let mut stmt = conn.prepare(
        "
            SELECT id, kind, COALESCE(label, name, id), is_archived, topic, message_count, CASE
            WHEN notification = 'muted' THEN 0
            ELSE unread_count END, latest_ts, dm_user_id, last_read, starred, notification,
                COALESCE(json_extract(raw_json, '$.is_general'), 0), is_member
            FROM conversations
            WHERE team_id = ?1
            AND (?2
                OR is_member = 1)
            AND closed_at IS NULL
            ORDER BY CASE kind
            WHEN 'public_channel' THEN 0
            WHEN 'private_channel' THEN 0
            WHEN 'mpim' THEN 1
            ELSE 2 END, is_archived, lower(COALESCE(label, name, id))
        ",
    )?;
    let rows = stmt
        .query_map(params![team_id, non_member], |r| {
            Ok(ConversationRow {
                is_member: r.get(13)?,
                last_read: r.get(9)?,
                starred: r.get(10)?,
                notification: r.get(11)?,
                is_general: r.get(12)?,
                id: r.get(0)?,
                kind: r.get(1)?,
                label: r.get(2)?,
                is_archived: r.get(3)?,
                topic: r.get(4)?,
                messages: r.get(5)?,
                unread: r.get(6)?,
                latest_ts: r.get(7)?,
                dm_user_id: r.get(8)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Finds a conversation by id, name or label (`#general`, `general`, `Bob`).
pub fn find_conversation(
    conn: &Connection,
    team_id: &str,
    query: &str,
) -> Result<Option<ConversationRow>> {
    match crate::references::resolve(conn, team_id, query, crate::references::Match::AllowPrefix)? {
        crate::references::Resolution::Found(c) => Ok(Some(c)),
        crate::references::Resolution::NotFound => Ok(None),
        crate::references::Resolution::Ambiguous(candidates) => {
            Err(crate::Error::Ambiguous { candidates })
        }
    }
}
