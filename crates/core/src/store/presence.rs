//! Ephemeral state persisted for DB-only readers. Freshness is per source:
//! a DND update cannot revive yesterday's active presence after a disconnect.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::api::{Dnd, UserPresence};
use crate::Result;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Presence {
    pub user_id: String,
    pub presence: Option<String>,
    pub dnd_enabled: bool,
    pub dnd_start: i64,
    pub dnd_end: i64,
    pub snooze_enabled: bool,
    pub snooze_end: i64,
    pub manual_away: Option<bool>,
    pub auto_away: Option<bool>,
    pub online: Option<bool>,
    pub connection_count: Option<i64>,
    pub presence_fresh: bool,
    pub dnd_fresh: bool,
    pub updated_at: i64,
}

impl Presence {
    /// Manual away wins for self. Otherwise current snooze/scheduled DND wins
    /// over active; away/offline/auto-away is grey. Stale presence is unknown
    /// (also grey), never yesterday's green. DND windows are [start, end).
    pub fn indicator(&self, now: i64) -> &'static str {
        if !self.presence_fresh {
            return "unknown";
        }
        if self.manual_away == Some(true) {
            return "away";
        }
        if self.dnd_until(now).is_some() {
            return "dnd";
        }
        if self.presence.as_deref() == Some("active")
            && self.auto_away != Some(true)
            && self.online != Some(false)
            && self.connection_count != Some(0)
        {
            "active"
        } else {
            "away"
        }
    }

    pub fn dnd_until(&self, now: i64) -> Option<i64> {
        if !self.dnd_fresh {
            return None;
        }
        let snooze = (self.snooze_enabled && self.snooze_end > now).then_some(self.snooze_end);
        let scheduled = (self.dnd_enabled && self.dnd_start <= now && self.dnd_end > now)
            .then_some(self.dnd_end);
        snooze.into_iter().chain(scheduled).max()
    }
}

pub fn mark_stale(c: &Connection) -> Result<()> {
    c.execute(
        "UPDATE presence SET presence_fresh = 0, dnd_fresh = 0,
             presence_version = presence_version + 1, dnd_version = dnd_version + 1",
        [],
    )?;
    Ok(())
}

pub fn set_presence(c: &Connection, user: &str, p: &UserPresence) -> Result<()> {
    c.execute(
        "INSERT INTO presence (user_id, presence, manual_away, auto_away, online,
                               connection_count, presence_fresh, presence_version, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1, unixepoch())
         ON CONFLICT(user_id) DO UPDATE SET presence = excluded.presence,
             manual_away = COALESCE(excluded.manual_away, presence.manual_away),
             auto_away = excluded.auto_away, online = excluded.online,
             connection_count = excluded.connection_count, presence_fresh = 1,
             presence_version = presence_version + 1,
             updated_at = excluded.updated_at",
        params![
            user,
            p.presence,
            p.manual_away,
            p.auto_away,
            p.online,
            p.connection_count
        ],
    )?;
    Ok(())
}

pub fn set_dnd(c: &Connection, user: &str, d: &Dnd) -> Result<()> {
    c.execute(
        "INSERT INTO presence (user_id, dnd_enabled, dnd_start, dnd_end, snooze_enabled,
                               snooze_end, dnd_fresh, dnd_version, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1, unixepoch())
         ON CONFLICT(user_id) DO UPDATE SET dnd_enabled = excluded.dnd_enabled,
             dnd_start = excluded.dnd_start, dnd_end = excluded.dnd_end,
             snooze_enabled = excluded.snooze_enabled, snooze_end = excluded.snooze_end,
             dnd_fresh = 1, dnd_version = dnd_version + 1, updated_at = excluded.updated_at",
        params![
            user,
            d.dnd_enabled,
            d.next_dnd_start_ts,
            d.next_dnd_end_ts,
            d.snooze_enabled,
            d.snooze_endtime
        ],
    )?;
    Ok(())
}

pub fn get(c: &Connection, user: &str) -> Result<Option<Presence>> {
    Ok(c.query_row(
        "SELECT user_id, presence, dnd_enabled, dnd_start, dnd_end, snooze_enabled,
                snooze_end, manual_away, auto_away, online, connection_count,
                presence_fresh, dnd_fresh, updated_at
         FROM presence WHERE user_id = ?1",
        [user],
        |r| {
            Ok(Presence {
                user_id: r.get(0)?,
                presence: r.get(1)?,
                dnd_enabled: r.get(2)?,
                dnd_start: r.get(3)?,
                dnd_end: r.get(4)?,
                snooze_enabled: r.get(5)?,
                snooze_end: r.get(6)?,
                manual_away: r.get(7)?,
                auto_away: r.get(8)?,
                online: r.get(9)?,
                connection_count: r.get(10)?,
                presence_fresh: r.get(11)?,
                dnd_fresh: r.get(12)?,
                updated_at: r.get(13)?,
            })
        },
    )
    .optional()?)
}

pub fn for_users(c: &Connection, ids: &[String]) -> Result<Vec<Presence>> {
    ids.iter()
        .map(|id| {
            Ok(get(c, id)?.unwrap_or_else(|| Presence {
                user_id: id.clone(),
                ..Default::default()
            }))
        })
        .collect()
}

/// Merge only the returned profile; directory fields and cached photo URLs survive.
pub fn set_profile(c: &Connection, user: &str, profile: &serde_json::Value) -> Result<()> {
    c.execute(
        "UPDATE users SET raw_json = json_set(COALESCE(raw_json, '{}'), '$.profile',
             json_patch(COALESCE(json_extract(raw_json, '$.profile'), '{}'), json(?2)))
         WHERE id = ?1",
        params![user, profile.to_string()],
    )?;
    Ok(())
}

/// Invalidate people newly added to a subscription before showing snapshots.
pub fn stale_users(c: &Connection, ids: &[String]) -> Result<()> {
    let mut stmt =
        c.prepare("UPDATE presence SET presence_fresh = 0, dnd_fresh = 0,
             presence_version = presence_version + 1, dnd_version = dnd_version + 1 WHERE user_id = ?1")?;
    for id in ids {
        stmt.execute([id])?;
    }
    Ok(())
}

/// Snapshot guards: a slow HTTP response cannot replace a newer live update.
pub fn versions(c: &Connection, user: &str) -> Result<(i64, i64)> {
    Ok(c.query_row(
        "SELECT presence_version, dnd_version FROM presence WHERE user_id = ?1",
        [user],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?
    .unwrap_or_default())
}

/// Snooze setters may omit all schedule fields (endSnooze may return only
/// ok). Preserve the schedule read from dnd.info/teamInfo or a live event.
pub fn set_snooze(c: &Connection, user: &str, enabled: bool, end: i64) -> Result<()> {
    c.execute(
        "INSERT INTO presence (user_id, snooze_enabled, snooze_end, dnd_fresh, dnd_version, updated_at)
         VALUES (?1, ?2, ?3, 1, 1, unixepoch())
         ON CONFLICT(user_id) DO UPDATE SET snooze_enabled = excluded.snooze_enabled,
             snooze_end = excluded.snooze_end,
             dnd_enabled = CASE WHEN dnd_fresh THEN dnd_enabled ELSE 0 END,
             dnd_fresh = 1,
             dnd_version = dnd_version + 1, updated_at = excluded.updated_at",
        params![user, enabled, end],
    )?;
    Ok(())
}

pub fn user_exists(c: &Connection, user: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1)",
        [user],
        |r| r.get(0),
    )?)
}
