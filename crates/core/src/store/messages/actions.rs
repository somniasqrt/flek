//! Own-message write preconditions and partial HTTP edit responses.
use crate::{api::chat::Updated, Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

/// Content observed before HTTP; compare it as well as edited.ts when Slack's
/// partial response has no timestamp (including two concurrent local writes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditVersion {
    pub edited_ts: Option<String>,
    pub text: String,
    pub blocks_json: Option<String>,
}

/// The version observed before the remote write. A response without edited.ts
/// must not overwrite a newer version that arrived while HTTP was in flight.
pub fn require_own_message(
    c: &Connection,
    team: &str,
    user: &str,
    channel: &str,
    ts: &str,
) -> Result<EditVersion> {
    let row = c
        .query_row(
            "
            SELECT m.user_id, m.edited_ts, json_extract(m.raw_json, '$.bot_id'),
                COALESCE(m.text, ''), m.blocks_json
            FROM messages m
            JOIN conversations c ON c.id = m.conversation_id
            WHERE m.conversation_id = ?1 AND m.ts = ?2
            AND c.team_id = ?3 AND m.deleted = 0
        ",
            params![channel, ts, team],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| Error::NotFound("Message is not stored in this workspace".into()))?;
    if user.is_empty() || row.0.as_deref() != Some(user) || row.2.is_some() {
        return Err(Error::Forbidden);
    }
    Ok(EditVersion {
        edited_ts: row.1,
        text: row.3,
        blocks_json: row.4,
    })
}

/// SQLite is the authority even when the last own message is outside the UI's
/// paginated window. Timeline scope excludes ordinary thread replies.
pub fn last_own_message(
    c: &Connection,
    team: &str,
    user: &str,
    channel: &str,
    thread: Option<&str>,
) -> Result<Option<String>> {
    Ok(c.query_row(
        "
            SELECT m.ts
            FROM messages m
            JOIN conversations c ON c.id = m.conversation_id
            WHERE c.team_id = ?1 AND m.user_id = ?2 AND m.conversation_id = ?3
            AND m.deleted = 0 AND m.hidden_by_limit = 0
            AND json_extract(m.raw_json, '$.bot_id') IS NULL
            AND (m.subtype IS NULL OR m.subtype IN ('me_message', 'file_share', 'thread_broadcast', 'reply_broadcast'))
            AND ((?4 IS NULL AND (m.thread_ts IS NULL OR m.thread_ts = m.ts
                OR m.subtype IN ('thread_broadcast', 'reply_broadcast')))
                OR (?4 IS NOT NULL AND m.thread_ts = ?4 AND m.ts != ?4))
            ORDER BY m.ts DESC
            LIMIT 1
        ", params![team, user, channel, thread], |r| r.get(0),
    ).optional()?)
}

/// Partial HTTP edits use the live merge's version/deletion rules and the same
/// FTS triggers, without replacing snapshots of reactions, files or replies.
pub fn apply_confirmed_edit(
    c: &mut Connection,
    update: &Updated,
    previous: &EditVersion,
) -> Result<()> {
    let tx = c.transaction()?;
    let row = tx
        .query_row(
            "
            SELECT raw_json, edited_ts, COALESCE(text, ''), blocks_json
            FROM messages
            WHERE conversation_id = ?1 AND ts = ?2 AND deleted = 0
        ",
            params![update.channel, update.ts],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    EditVersion {
                        edited_ts: r.get(1)?,
                        text: r.get(2)?,
                        blocks_json: r.get(3)?,
                    },
                ))
            },
        )
        .optional()?;
    if let Some((raw, current)) = row {
        let eligible = match update.edited_ts.as_deref() {
            Some(version) => current
                .edited_ts
                .as_deref()
                .is_none_or(|current| version >= current),
            None => &current == previous,
        };
        if eligible {
            // The response often has no edit timestamp. Reuse the known version
            // (or creation ts) as a provisional marker, never the local clock:
            // the authoritative RTM echo/catch-up can always replace it.
            let version = update
                .edited_ts
                .as_deref()
                .or(previous.edited_ts.as_deref())
                .unwrap_or(&update.ts);
            let mut raw: serde_json::Value = serde_json::from_str(&raw)?;
            if !raw.is_object() {
                raw = serde_json::json!({});
            }
            raw["ts"] = update.ts.clone().into();
            raw["text"] = update.text.clone().into();
            raw["blocks"] = update.blocks.clone().unwrap_or(serde_json::Value::Null);
            raw["edited"] = serde_json::json!({"ts": version});
            // The next authoritative revision removes this local-only marker.
            // Same-version history must not replace the confirmed new content.
            if update.edited_ts.is_none() {
                raw["_flek_edit_pending"] = true.into();
            } else {
                raw.as_object_mut().unwrap().remove("_flek_edit_pending");
            }
            let blocks = update
                .blocks
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            tx.execute(
                "
                    UPDATE messages
                    SET text = ?3, blocks_json = ?4, edited_ts = ?5, raw_json = ?6,
                        hidden_by_limit = 0
                    WHERE conversation_id = ?1 AND ts = ?2 AND deleted = 0
                ",
                params![
                    update.channel,
                    update.ts,
                    update.text,
                    blocks,
                    version,
                    raw.to_string()
                ],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// A confirmed deletion/event is evidence even for a hidden row. In contrast,
/// mark_deleted (absence from refreshed history) must preserve hidden archives.
pub fn mark_deleted_explicit(c: &Connection, channel: &str, ts: &str) -> Result<()> {
    c.execute(
        "
            UPDATE messages
            SET deleted = 1
            WHERE conversation_id = ?1 AND ts = ?2 AND deleted = 0
        ",
        params![channel, ts],
    )?;
    Ok(())
}
