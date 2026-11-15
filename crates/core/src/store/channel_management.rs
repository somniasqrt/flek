//! Confirmed channel changes. Deltas are idempotent and never remove history.
use crate::Result;
use rusqlite::{params, Connection};

pub enum Update {
    Topic(String),
    Purpose(String),
    Rename(String),
    Archive,
    Member {
        user: String,
        member: bool,
        own: bool,
    },
}

pub fn apply(c: &Connection, id: &str, update: Update) -> Result<()> {
    match update {
        Update::Topic(value) => {
            c.execute(
                "
                    UPDATE conversations
                    SET topic = ?2, raw_json = json_set(COALESCE(raw_json, '{}'), '$.topic.value', ?2)
                    WHERE id = ?1
                ",
                params![id, value],
            )?;
        }
        Update::Purpose(value) => {
            c.execute(
                "
                    UPDATE conversations
                    SET purpose = ?2, raw_json = json_set(COALESCE(raw_json, '{}'), '$.purpose.value', ?2)
                    WHERE id = ?1
                ",
                params![id, value],
            )?;
        }
        Update::Rename(value) => {
            c.execute(
                "
                    UPDATE conversations
                    SET name = ?2, label = ?2, raw_json = json_set(COALESCE(raw_json, '{}'),
                        '$.name', ?2, '$.name_normalized', ?2)
                    WHERE id = ?1
                ",
                params![id, value],
            )?;
        }
        Update::Archive => {
            c.execute(
                "
                    UPDATE conversations
                    SET is_archived = 1, raw_json = json_set(COALESCE(raw_json, '{}'),
                        '$.is_archived', json('true'))
                    WHERE id = ?1
                ",
                [id],
            )?;
        }
        Update::Member { user, member, own } => {
            if member {
                c.execute(
                    "
                        INSERT OR IGNORE INTO conversation_members(conversation_id, user_id)
                        VALUES (?1, ?2)
                    ",
                    params![id, user],
                )?;
            } else {
                c.execute(
                    "
                        DELETE FROM conversation_members
                        WHERE conversation_id = ?1 AND user_id = ?2
                    ",
                    params![id, user],
                )?;
            }
            if own {
                c.execute(
                    "
                        UPDATE conversations
                        SET is_member = ?2, unread_count = CASE WHEN ?2 THEN unread_count ELSE 0 END,
                            raw_json = json_set(COALESCE(raw_json, '{}'), '$.is_member', json(?3))
                        WHERE id = ?1
                    ",
                    params![id, member, if member { "true" } else { "false" }],
                )?;
            }
            // A delta does not turn a partial member set into a complete snapshot.
            // Count from the snapshot when present; otherwise discard the stale hint.
            c.execute(
                "
                    UPDATE conversations
                    SET raw_json = CASE WHEN EXISTS (
                        SELECT 1 FROM conversation_details_sync WHERE conversation_id = ?1
                    ) THEN json_set(COALESCE(raw_json, '{}'), '$.num_members', (
                        SELECT COUNT(*) FROM conversation_members WHERE conversation_id = ?1
                    )) ELSE json_remove(COALESCE(raw_json, '{}'), '$.num_members') END
                    WHERE id = ?1
                ",
                [id],
            )?;
        }
    }
    Ok(())
}
