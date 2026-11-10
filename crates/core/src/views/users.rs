//! Users storage views.

use crate::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub real_name: Option<String>,
    pub is_bot: bool,
    pub deleted: bool,
    pub tz: Option<String>,
    pub title: Option<String>,
    pub status_text: Option<String>,
    pub status_emoji: Option<String>,
    pub status_expiration: Option<i64>,
    pub presence: String,
    pub dnd_until: Option<i64>,
    pub tz_label: Option<String>,
    pub email: Option<String>,
    pub is_admin: Option<bool>,
    pub is_owner: Option<bool>,
    pub dm_conversation_id: Option<String>,
}

pub fn user(conn: &Connection, id: &str) -> Result<Option<User>> {
    Ok(conn.query_row("
        SELECT u.id, u.name, u.display_name, u.real_name, u.is_bot, u.deleted, u.raw_json, (
            SELECT id
            FROM conversations
            WHERE dm_user_id = u.id
            AND team_id = u.team_id
            ORDER BY id
            LIMIT 1), p.user_id, p.presence, p.dnd_enabled, p.dnd_start, p.dnd_end, p.snooze_enabled,
            p.snooze_end, p.manual_away, p.auto_away, p.online, p.connection_count, p.presence_fresh,
            p.dnd_fresh, p.updated_at
        FROM users u
        LEFT JOIN presence p ON p.user_id = u.id
        WHERE u.id = ?1
    ", [id], user_row).optional()?)
}

pub(crate) fn user_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    let raw: Option<String> = r.get(6)?;
    let v: serde_json::Value = raw
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let p = &v["profile"];
    let text = |v: &serde_json::Value| v.as_str().map(str::to_owned);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let presence = if let Some(user_id) = r.get::<_, Option<String>>(8)? {
        crate::store::presence::Presence {
            user_id,
            presence: r.get(9)?,
            dnd_enabled: r.get(10)?,
            dnd_start: r.get(11)?,
            dnd_end: r.get(12)?,
            snooze_enabled: r.get(13)?,
            snooze_end: r.get(14)?,
            manual_away: r.get(15)?,
            auto_away: r.get(16)?,
            online: r.get(17)?,
            connection_count: r.get(18)?,
            presence_fresh: r.get(19)?,
            dnd_fresh: r.get(20)?,
            updated_at: r.get(21)?,
        }
    } else {
        Default::default()
    };
    Ok(User {
        presence: presence.indicator(now).to_owned(),
        dnd_until: presence.dnd_until(now),
        id: r.get(0)?,
        name: r.get(1)?,
        display_name: r.get(2)?,
        real_name: r.get(3)?,
        is_bot: r.get(4)?,
        deleted: r.get(5)?,
        tz: text(&v["tz"]),
        title: text(&p["title"]),
        status_text: text(&p["status_text"]),
        status_emoji: text(&p["status_emoji"]),
        status_expiration: p["status_expiration"].as_i64(),
        tz_label: text(&v["tz_label"]),
        email: text(&p["email"]),
        is_admin: v["is_admin"].as_bool(),
        is_owner: v["is_owner"].as_bool(),
        dm_conversation_id: r.get(7)?,
    })
}
