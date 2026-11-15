//! Small confirmed message-action cache updates. No network or policy here.
use crate::Result;
use rusqlite::{params, Connection};

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
        }
        _ => return Err(crate::Error::Forbidden),
    }
    Ok(())
}
