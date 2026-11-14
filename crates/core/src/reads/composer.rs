//! Small DB-only directory for composer completion. No profile hydration or network.
use crate::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Serialize)]
pub struct ComposerPerson {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub real_name: Option<String>,
    pub is_bot: bool,
    pub is_member: bool,
    pub recent_dm: bool,
}

pub fn composer_people(
    c: &Connection,
    team: &str,
    conversation: &str,
) -> Result<Vec<ComposerPerson>> {
    let mut stmt = c.prepare(
        "SELECT u.id, u.name, u.display_name, u.real_name, u.is_bot,
            EXISTS(SELECT 1 FROM conversation_members cm
                WHERE cm.conversation_id = ?2 AND cm.user_id = u.id),
            u.id IN (SELECT dm_user_id FROM conversations WHERE team_id = ?1
                AND kind = 'im' AND dm_user_id IS NOT NULL AND latest_ts IS NOT NULL
                ORDER BY latest_ts DESC, id LIMIT 20)
         FROM users u WHERE u.team_id = ?1 AND NOT u.deleted ORDER BY u.name, u.id",
    )?;
    let rows = stmt
        .query_map(params![team, conversation], |r| {
            Ok(ComposerPerson {
                id: r.get(0)?,
                name: r.get(1)?,
                display_name: r.get(2)?,
                real_name: r.get(3)?,
                is_bot: r.get(4)?,
                is_member: r.get(5)?,
                recent_dm: r.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}
