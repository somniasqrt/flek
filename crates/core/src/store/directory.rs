//! Teams, users and conversations.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};

use crate::api::{Conversation, ConversationKind, User};
use crate::Result;

/// Inserts or updates the team row.
pub fn upsert_team(
    conn: &Connection,
    id: &str,
    name: &str,
    url: &str,
    self_user: &str,
) -> Result<()> {
    let domain = url
        .trim_start_matches("https://")
        .split('.')
        .next()
        .filter(|d| !d.is_empty());
    conn.execute(
        "INSERT INTO teams (id, name, domain, url, self_user_id) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, domain = excluded.domain,
            url = excluded.url, self_user_id = excluded.self_user_id",
        params![id, name, domain, url, self_user],
    )?;
    Ok(())
}

/// Who the stored session belongs to, as recorded by the last directory
/// sync; lets the app start offline.
pub fn stored_identity(conn: &Connection, team_id: &str) -> Result<Option<crate::api::AuthTest>> {
    Ok(conn
        .query_row(
            "SELECT t.url, t.name, u.name, t.id, t.self_user_id FROM teams t
             JOIN users u ON u.id = t.self_user_id
             WHERE t.id = ?1",
            [team_id],
            |r| {
                Ok(crate::api::AuthTest {
                    url: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    team: r.get(1)?,
                    user: r.get(2)?,
                    team_id: r.get(3)?,
                    user_id: r.get(4)?,
                    enterprise_id: None,
                })
            },
        )
        .optional()?)
}

/// Upserts users. Users from other teams (shared channels) get a stub team row.
pub fn upsert_users(conn: &mut Connection, default_team: &str, users: &[User]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stub_team = tx.prepare("INSERT OR IGNORE INTO teams (id, name) VALUES (?1, '')")?;
        let mut upsert = tx.prepare(
            "INSERT INTO users (id, team_id, name, real_name, display_name, is_bot, deleted, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                team_id = excluded.team_id, name = excluded.name, real_name = excluded.real_name,
                display_name = excluded.display_name, is_bot = excluded.is_bot,
                deleted = excluded.deleted, raw_json = excluded.raw_json",
        )?;
        for u in users {
            let team = u.team_id.as_deref().unwrap_or(default_team);
            stub_team.execute([team])?;
            let real_name = Some(u.profile.real_name.as_str())
                .filter(|s| !s.is_empty())
                .or(u.real_name.as_deref());
            upsert.execute(params![
                u.id,
                team,
                u.name,
                real_name,
                u.label(),
                u.is_bot,
                u.deleted,
                u.raw.to_string()
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Upserts conversations and computes their labels from the users table.
pub fn upsert_conversations(
    conn: &mut Connection,
    team_id: &str,
    self_user: &str,
    convs: &[Conversation],
) -> Result<()> {
    let users = user_labels(conn)?;
    let tx = conn.transaction()?;
    {
        let mut upsert = tx.prepare(
            "INSERT INTO conversations
                (id, team_id, kind, name, label, dm_user_id, topic, purpose, is_archived, is_member, created, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind, name = excluded.name, label = excluded.label,
                dm_user_id = excluded.dm_user_id, topic = excluded.topic, purpose = excluded.purpose,
                is_archived = excluded.is_archived, is_member = 1, created = excluded.created,
                raw_json = excluded.raw_json",
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
    tx.commit()?;
    Ok(())
}

struct UserLabels {
    by_id: HashMap<String, String>,
    by_handle: HashMap<String, (String, String)>,
}

fn user_labels(conn: &Connection) -> Result<UserLabels> {
    let mut stmt = conn.prepare("SELECT id, name, COALESCE(display_name, name) FROM users")?;
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
        "SELECT id, kind, COALESCE(label, name, id), is_archived, topic,
                message_count, unread_count, latest_ts, dm_user_id
         FROM conversations
         WHERE team_id = ?1 AND (?2 OR is_member = 1)
         ORDER BY CASE kind WHEN 'public_channel' THEN 0 WHEN 'private_channel' THEN 0 WHEN 'mpim' THEN 1 ELSE 2 END,
                  is_archived, lower(COALESCE(label, name, id))",
    )?;
    let rows = stmt
        .query_map(params![team_id, non_member], |r| {
            Ok(ConversationRow {
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

/// `id → display label` of every known user of a team, for mentions.
pub fn people(
    conn: &Connection,
    team_id: &str,
) -> Result<std::collections::HashMap<String, String>> {
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(NULLIF(display_name, ''), NULLIF(real_name, ''), name)
         FROM users WHERE team_id = ?1",
    )?;
    let rows = stmt
        .query_map([team_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Display label of one user, if known.
pub fn user_label(conn: &Connection, user_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT COALESCE(display_name, name) FROM users WHERE id = ?1",
            [user_id],
            |r| r.get(0),
        )
        .optional()?)
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

/// Stores Slack's read markers (`client.counts`, later `channel_marked` events).
pub fn set_read_markers(conn: &mut Connection, markers: &[(String, Option<String>)]) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare("UPDATE conversations SET last_read = ?2 WHERE id = ?1")?;
        for (id, last_read) in markers {
            stmt.execute(params![id, last_read])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Stores the newest ts Slack reports per conversation (`client.counts`).
/// Conversations missing from `latest`, or every one when it is `None`
/// (the call failed), become unknown, so they are never skipped on old data.
pub fn set_remote_latest(
    conn: &mut Connection,
    team_id: &str,
    latest: Option<&[(String, Option<String>)]>,
) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE conversations SET remote_latest = NULL WHERE team_id = ?1",
        [team_id],
    )?;
    {
        let mut stmt = tx.prepare(
            "UPDATE conversations SET remote_latest = ?2 WHERE id = ?1 AND team_id = ?3",
        )?;
        for (id, ts) in latest.unwrap_or_default() {
            stmt.execute(params![id, ts, team_id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Conversations whose stored history already reaches the newest message
/// Slack reports: a catch-up there would fetch nothing.
pub fn unchanged_conversations(conn: &Connection, team_id: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT c.id FROM conversations c JOIN sync_state s ON s.conversation_id = c.id
         WHERE c.team_id = ?1 AND c.is_member = 1 AND c.remote_latest IS NOT NULL
           AND s.newest_ts IS NOT NULL AND c.remote_latest <= s.newest_ts",
    )?;
    let ids = stmt
        .query_map([team_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

/// `(id, label)` of a team's conversations, most recently active first
/// (then never-synced ones), so fresh conversations are synced first.
pub fn conversation_activity(conn: &Connection, team_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, COALESCE(c.label, c.name, c.id) FROM conversations c
         LEFT JOIN sync_state s ON s.conversation_id = c.id
         WHERE c.team_id = ?1 AND c.is_member = 1
         ORDER BY s.newest_ts IS NULL, s.newest_ts DESC, c.is_archived, lower(COALESCE(c.label, c.name, c.id))",
    )?;
    let rows = stmt
        .query_map([team_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Avatar URL Slack shows next to messages (72 px, sharp at 36 px on Retina).
const AVATAR_URL: &str = "json_extract(u.raw_json, '$.profile.image_72')";

/// `user id → cached avatar` (the hash `flekfile://localhost/link/<hash>`
/// serves). Users whose avatar is not downloaded yet are left out.
pub fn avatars(conn: &Connection, team_id: &str) -> Result<HashMap<String, String>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT u.id, {AVATAR_URL} FROM users u WHERE u.team_id = ?1 AND {AVATAR_URL} IS NOT NULL"
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
        "SELECT DISTINCT {AVATAR_URL} FROM users u
         WHERE u.team_id = ?1 AND u.deleted = 0 AND {AVATAR_URL} LIKE 'http%'
           AND (u.id IN (SELECT user_id FROM messages WHERE user_id IS NOT NULL)
                OR u.id IN (SELECT dm_user_id FROM conversations WHERE dm_user_id IS NOT NULL))"
    ))?;
    let mut known = conn.prepare_cached("SELECT 1 FROM link_images WHERE hash = ?1")?;
    let mut out = Vec::new();
    let mut rows = stmt.query([team_id])?;
    while let Some(r) = rows.next()? {
        let url: String = r.get(0)?;
        if !known.exists([crate::store::unfurls::url_hash(&url)])? {
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
            "SELECT COALESCE(json_extract(raw_json, '$.profile.image_512'),
                             json_extract(raw_json, '$.profile.image_192'))
             FROM users WHERE id = ?1",
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
            "SELECT name, real_name, is_bot, deleted, raw_json FROM users WHERE id = ?1",
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
            "SELECT id FROM conversations WHERE kind = 'im' AND dm_user_id = ?1
             ORDER BY is_archived LIMIT 1",
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
