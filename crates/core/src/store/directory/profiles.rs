//! Profiles storage views.

use crate::Result;
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;

/// Avatar URL Slack shows next to messages (72 px, sharp at 36 px on Retina).
const AVATAR_URL: &str = "json_extract(u.raw_json, '$.profile.image_72')";

/// `user id → cached avatar` (the hash `flekfile://localhost/link/<hash>`
/// serves). Users whose avatar is not downloaded yet are left out.
pub fn avatars(conn: &Connection, team_id: &str) -> Result<HashMap<String, String>> {
    let mut stmt = conn.prepare(&format!(
        "
            SELECT u.id, {AVATAR_URL}
            FROM users u
            WHERE u.team_id = ?1
            AND {AVATAR_URL} IS NOT NULL
        "
    ))?;
    let mut out = HashMap::new();
    let mut rows = stmt.query([team_id])?;
    while let Some(r) = rows.next()? {
        let url: String = r.get(1)?;
        if crate::store::unfurls::is_cached(conn, &url) {
            out.insert(r.get(0)?, crate::store::unfurls::url_hash(&url));
        }
    }
    Ok(out)
}

/// Avatars not tried yet, of people the timeline can show: message authors
/// and DM partners (not the whole directory).
pub fn avatars_to_fetch(conn: &Connection, team_id: &str, limit: usize) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!(
        "
            SELECT DISTINCT {AVATAR_URL}
            FROM users u
            WHERE u.team_id = ?1
            AND u.deleted = 0
            AND {AVATAR_URL} LIKE 'http%'
            AND (u.id IN (
                    SELECT user_id
                    FROM messages
                    WHERE user_id IS NOT NULL)
                OR u.id IN (
                    SELECT dm_user_id
                    FROM conversations
                    WHERE dm_user_id IS NOT NULL))
        "
    ))?;
    let mut out = Vec::new();
    let mut rows = stmt.query([team_id])?;
    while let Some(r) = rows.next()? {
        let url: String = r.get(0)?;
        if crate::store::unfurls::image_pending(conn, &url)? {
            out.push(url);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

/// What the profile panel shows about one person, from the stored
/// `users.list` entry. Email and phone are personal data: shown, never
/// logged (the `Debug` impl hides them).
#[derive(Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Profile {
    pub id: String,
    /// Handle (`name`).
    pub name: String,
    pub real_name: Option<String>,
    pub display_name: Option<String>,
    /// Job title, e.g. "Доцент".
    pub title: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub pronouns: Option<String>,
    pub status_text: Option<String>,
    pub status_emoji: Option<String>,
    pub status_expiration: Option<i64>,
    /// IANA zone, e.g. `Europe/Kyiv`.
    pub tz: Option<String>,
    /// Human zone name, e.g. "Eastern European Summer Time".
    pub tz_label: Option<String>,
    /// Seconds east of UTC (Slack's value when the directory was synced).
    pub tz_offset: Option<i64>,
    pub is_bot: bool,
    pub deleted: bool,
    /// Cached 72 px avatar (hash for `flekfile://localhost/link/<hash>`).
    pub avatar: Option<String>,
    /// Cached large photo, once `sync::files::fetch_profile_photo` got it.
    pub photo: Option<String>,
    /// Whether Slack lists a large photo to fetch.
    pub has_photo: bool,
    /// The DM with this person, if Flek has it.
    pub dm_conversation_id: Option<String>,
}

impl std::fmt::Debug for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Profile")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("email", &self.email.as_ref().map(|_| "***"))
            .field("phone", &self.phone.as_ref().map(|_| "***"))
            .finish_non_exhaustive()
    }
}

/// The largest photo Slack lists for a user (512, then 192 px).
pub fn profile_photo_url(conn: &Connection, user_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "
                SELECT COALESCE(json_extract(raw_json, '$.profile.image_512'), json_extract(raw_json,
                            '$.profile.image_192'))
                FROM users
                WHERE id = ?1
            ",
            [user_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .filter(|u| u.starts_with("http")))
}

/// One person's profile, or `None` for an unknown user id.
pub fn user_profile(conn: &Connection, user_id: &str) -> Result<Option<Profile>> {
    let row = conn
        .query_row(
            "
                SELECT name, real_name, is_bot, deleted, raw_json
                FROM users
                WHERE id = ?1
            ",
            [user_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, bool>(2)?,
                    r.get::<_, bool>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((name, real_name, is_bot, deleted, raw)) = row else {
        return Ok(None);
    };
    let raw: serde_json::Value = raw
        .as_deref()
        .and_then(|r| serde_json::from_str(r).ok())
        .unwrap_or_default();
    let p = &raw["profile"];
    let text = |v: &serde_json::Value| {
        v.as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let cached = |url: Option<String>| {
        url.filter(|u| crate::store::unfurls::is_cached(conn, u))
            .map(|u| crate::store::unfurls::url_hash(&u))
    };
    let photo_url = profile_photo_url(conn, user_id)?;
    let dm_conversation_id = conn
        .query_row(
            "
                SELECT id
                FROM conversations
                WHERE kind = 'im'
                AND dm_user_id = ?1
                ORDER BY is_archived
                LIMIT 1
            ",
            [user_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(Some(Profile {
        id: user_id.to_owned(),
        name,
        real_name: text(&p["real_name"]).or(real_name.filter(|s| !s.is_empty())),
        // The column holds the label (it falls back to the real name).
        display_name: text(&p["display_name"]),
        title: text(&p["title"]),
        email: text(&p["email"]),
        phone: text(&p["phone"]),
        pronouns: text(&p["pronouns"]),
        status_text: text(&p["status_text"]),
        status_emoji: text(&p["status_emoji"]),
        status_expiration: p["status_expiration"].as_i64(),
        tz: text(&raw["tz"]),
        tz_label: text(&raw["tz_label"]),
        tz_offset: raw["tz_offset"].as_i64(),
        is_bot,
        deleted,
        avatar: cached(text(&p["image_72"])),
        has_photo: photo_url.is_some(),
        photo: cached(photo_url),
        dm_conversation_id,
    }))
}

#[derive(Clone, Default, serde::Serialize)]
pub struct CustomStatus {
    pub status_text: Option<String>,
    pub status_emoji: Option<String>,
    pub status_expiration: Option<i64>,
}

pub fn statuses_for(c: &Connection, ids: &[String]) -> Result<HashMap<String, CustomStatus>> {
    let mut query = c.prepare(
        "
            SELECT json_extract(raw_json, '$.profile.status_text'), json_extract(raw_json,
                    '$.profile.status_emoji'), json_extract(raw_json,
                    '$.profile.status_expiration')
            FROM users
            WHERE id = ?1
        ",
    )?;
    let mut out = HashMap::new();
    for id in ids {
        if let Some(status) = query
            .query_row([id], |r| {
                Ok(CustomStatus {
                    status_text: r.get(0)?,
                    status_emoji: r.get(1)?,
                    status_expiration: r.get(2)?,
                })
            })
            .optional()?
        {
            out.insert(id.clone(), status);
        }
    }
    Ok(out)
}
