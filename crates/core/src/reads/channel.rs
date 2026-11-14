//! Offline channel reads.

use crate::{references, views, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

/// Small, offline-only header view. A missing count differs from a cached empty set.
#[derive(Serialize)]
pub struct ChannelHeader {
    pub num_members: i64,
    pub member_ids: Vec<String>,
}

pub fn channel_header(c: &Connection, id: &str) -> Result<Option<ChannelHeader>> {
    let Some((raw, synced)) = c
        .query_row(
            "SELECT raw_json, EXISTS(SELECT 1 FROM conversation_details_sync WHERE conversation_id = ?1)
             FROM conversations WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, bool>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let cached_count = raw
        .and_then(|r| serde_json::from_str::<serde_json::Value>(&r).ok())
        .and_then(|r| r["num_members"].as_i64())
        .filter(|n| *n >= 0);
    let num_members = if synced {
        // A complete membership snapshot is more precise than directory metadata.
        c.query_row(
            "SELECT COUNT(*) FROM conversation_members WHERE conversation_id = ?1",
            [id],
            |r| r.get(0),
        )?
    } else if let Some(count) = cached_count {
        count
    } else {
        return Ok(None);
    };
    let mut stmt = c.prepare(
        "SELECT user_id FROM conversation_members WHERE conversation_id = ?1 ORDER BY user_id LIMIT 3",
    )?;
    let member_ids = stmt
        .query_map([id], |r| r.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(Some(ChannelHeader {
        num_members,
        member_ids,
    }))
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
        "
            SELECT created, raw_json
            FROM conversations
            WHERE id = ?1
        ",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let raw: serde_json::Value = raw
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_default();
    let synced: Option<i64> = c
        .query_row(
            "
                SELECT synced_at
                FROM conversation_details_sync
                WHERE conversation_id = ?1
            ",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let members = if synced.is_some() {
        let mut stmt = c.prepare(
            "
                SELECT COALESCE(u.id, cm.user_id), COALESCE(u.name, cm.user_id), u.display_name, u.real_name,
                    COALESCE(u.is_bot, 0), COALESCE(u.deleted, 0), u.raw_json, (
                    SELECT id
                    FROM conversations
                    WHERE dm_user_id = u.id
                    AND team_id = u.team_id
                    ORDER BY id
                    LIMIT 1), p.user_id, p.presence, p.dnd_enabled, p.dnd_start, p.dnd_end, p.snooze_enabled,
                    p.snooze_end, p.manual_away, p.auto_away, p.online, p.connection_count, p.presence_fresh,
                    p.dnd_fresh, p.updated_at
                FROM conversation_members cm
                LEFT JOIN users u ON u.id = cm.user_id
                LEFT JOIN presence p ON p.user_id = cm.user_id
                WHERE cm.conversation_id = ?1
                ORDER BY u.name, u.id
            ",
        )?;
        let users = stmt
            .query_map([id], views::user_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Some(users)
    } else {
        None
    };
    let pins = if synced.is_some() {
        let mut stmt = c.prepare(
            "
                SELECT ts
                FROM pins
                WHERE conversation_id = ?1
                ORDER BY ts
            ",
        )?;
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
