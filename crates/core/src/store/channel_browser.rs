//! Cached channel directory. Metadata stays in conversations, including raw JSON.
use crate::{api::Conversation, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChannelRow {
    pub id: String,
    pub name: String,
    pub topic: Option<String>,
    pub purpose: Option<String>,
    pub is_member: bool,
    pub is_private: bool,
    pub is_archived: bool,
    pub num_members: Option<i64>,
    pub created: Option<i64>,
    pub creator: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct Directory {
    pub channels: Vec<ChannelRow>,
    pub fetched_at: Option<i64>,
    pub truncated: bool,
}

pub fn read(c: &Connection, team: &str) -> Result<Directory> {
    let mut stmt = c.prepare(
        "
            SELECT id, COALESCE(name, label, id), topic, purpose, is_member, kind =
                'private_channel', is_archived, json_extract(raw_json, '$.num_members'), created,
                json_extract(raw_json, '$.creator')
            FROM conversations
            WHERE team_id = ?1
            AND kind IN ('public_channel', 'private_channel')
        ",
    )?;
    let channels = stmt
        .query_map([team], |r| {
            Ok(ChannelRow {
                id: r.get(0)?,
                name: r.get(1)?,
                topic: r.get(2)?,
                purpose: r.get(3)?,
                is_member: r.get(4)?,
                is_private: r.get(5)?,
                is_archived: r.get(6)?,
                num_members: r.get(7)?,
                created: r.get(8)?,
                creator: r.get(9)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    let stamp = c
        .query_row(
            "
                SELECT fetched_at, truncated
                FROM channel_directory_sync
                WHERE team_id = ?1
            ",
            [team],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?)),
        )
        .optional()?;
    Ok(Directory {
        channels,
        fetched_at: stamp.map(|s| s.0),
        truncated: stamp.is_some_and(|s| s.1),
    })
}

/// Missing membership means unknown: preserve existing state, default new rows to false.
/// Explicit join/create forces membership; directory responses can confirm either state.
pub fn upsert(c: &Connection, team: &str, channel: &Conversation, joined: bool) -> Result<()> {
    let member = if joined {
        Some(true)
    } else {
        channel.raw["is_member"].as_bool()
    };
    c.execute(
        "
            INSERT INTO conversations(id, team_id, kind, name, label, topic, purpose,
                    is_archived, is_member, created, raw_json)
            VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, COALESCE(?8, 0), ?9, ?10)
            ON CONFLICT (id) DO UPDATE
            SET name = excluded.name, label = excluded.label, kind = excluded.kind, topic =
                excluded.topic, purpose = excluded.purpose, is_archived = excluded.is_archived,
                is_member = COALESCE(?8, conversations.is_member), created = COALESCE(excluded.created,
                    conversations.created), raw_json = json_patch(conversations.raw_json,
                    excluded.raw_json)
        ",
        params![
            channel.id,
            team,
            channel.kind().as_str(),
            channel.name,
            channel.topic.value,
            channel.purpose.value,
            channel.is_archived,
            member,
            channel.created,
            channel.raw.to_string()
        ],
    )?;
    Ok(())
}
