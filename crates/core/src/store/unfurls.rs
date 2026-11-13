//! Link previews (Slack "unfurls", the `attachments` of a message) and the
//! cache of their images.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::Result;

/// A link preview, ready to show.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Unfurl {
    pub service: Option<String>,
    pub title: Option<String>,
    pub title_link: Option<String>,
    pub text: Option<String>,
    pub author: Option<String>,
    pub footer: Option<String>,
    /// Link the preview is about.
    pub url: Option<String>,
    /// Bar colour, `#rrggbb`.
    pub color: Option<String>,
    /// Cached image (`flekfile://localhost/link/<hash>`) and its size.
    pub image: Option<String>,
    pub image_width: Option<i64>,
    pub image_height: Option<i64>,
    /// The image is a small thumbnail beside the text, not a large one below.
    pub thumb: bool,
    pub icon: Option<String>,
    /// A video (YouTube and the like): show a play mark over the image.
    pub video: bool,
    /// Key/value fields of app attachments.
    pub fields: Vec<(String, String)>,
}

/// Images an attachment refers to: `(url, is_icon)`.
fn image_urls(a: &Value) -> impl Iterator<Item = &str> {
    ["image_url", "thumb_url", "service_icon"]
        .into_iter()
        .filter_map(|k| a.get(k).and_then(Value::as_str))
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
}

/// Documented Block Kit image slots only; no arbitrary recursive URL discovery.
pub fn block_image_urls(blocks: &Value) -> Vec<&str> {
    let mut urls = Vec::new();
    for block in blocks.as_array().into_iter().flatten().take(100) {
        let slots: Vec<&Value> = match block.get("type").and_then(Value::as_str) {
            Some("image") => vec![block],
            Some("section") => block.get("accessory").into_iter().collect(),
            Some("context") => block
                .get("elements")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(10)
                .collect(),
            _ => Vec::new(),
        };
        for slot in slots {
            if slot.get("type").and_then(Value::as_str) == Some("image") {
                if let Some(url) = slot.get("image_url").and_then(Value::as_str) {
                    if url.starts_with("https://") {
                        urls.push(url);
                    }
                }
            }
        }
    }
    urls
}

pub fn cached_block_images(
    conn: &Connection,
    blocks: &Value,
) -> std::collections::HashMap<String, String> {
    block_image_urls(blocks)
        .into_iter()
        .filter(|url| is_cached(conn, url))
        .map(|url| (url.to_owned(), url_hash(url)))
        .collect()
}

pub fn url_hash(url: &str) -> String {
    Sha1::digest(url.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Unfurls of a message from its raw JSON; `cached(url)` says whether an
/// image is in the cache (only cached images are shown: offline-first, and
/// the UI never contacts third-party sites).
pub fn from_raw(raw: &str, cached: &dyn Fn(&str) -> bool) -> Vec<Unfurl> {
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(list) = v.get("attachments").and_then(Value::as_array) else {
        return Vec::new();
    };
    let s = |a: &Value, k: &str| {
        a.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
    };
    let n = |a: &Value, k: &str| a.get(k).and_then(Value::as_i64);
    let image = |url: Option<String>| url.filter(|u| cached(u)).map(|u| url_hash(&u));
    list.iter()
        .map(|a| {
            let (img, w, h, thumb) = match s(a, "image_url") {
                Some(u) => (Some(u), n(a, "image_width"), n(a, "image_height"), false),
                None => (
                    s(a, "thumb_url"),
                    n(a, "thumb_width"),
                    n(a, "thumb_height"),
                    true,
                ),
            };
            let video =
                a.get("video_html").is_some() || s(a, "service_name").as_deref() == Some("YouTube");
            Unfurl {
                service: s(a, "service_name"),
                title: s(a, "title"),
                title_link: s(a, "title_link"),
                text: s(a, "text").or_else(|| s(a, "pretext")),
                author: s(a, "author_name"),
                footer: s(a, "footer"),
                url: s(a, "original_url").or_else(|| s(a, "from_url")),
                color: s(a, "color").map(|c| {
                    if c.starts_with('#') {
                        c
                    } else {
                        format!("#{c}")
                    }
                }),
                image: image(img),
                image_width: w,
                image_height: h,
                // Video thumbnails are shown large even when sent as thumbs.
                thumb: thumb && !video,
                icon: image(s(a, "service_icon")),
                video,
                fields: a
                    .get("fields")
                    .and_then(Value::as_array)
                    .map(|fs| {
                        fs.iter()
                            .filter_map(|f| {
                                Some((s(f, "title")?, s(f, "value").unwrap_or_default()))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        })
        .filter(|u| {
            u.title.is_some() || u.text.is_some() || u.image.is_some() || !u.fields.is_empty()
        })
        .collect()
}

/// Attachments requiring actions/embeds, or with no readable legacy content.
/// Keep their raw evidence in SQLite and expose a native-client fallback.
pub fn unsupported_attachments(raw: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return false;
    };
    value
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|a| {
            ["blocks", "actions", "video_html"]
                .iter()
                .any(|key| a.get(key).is_some())
                || !["title", "text", "pretext", "image_url", "thumb_url"]
                    .iter()
                    .any(|key| {
                        a.get(key)
                            .and_then(Value::as_str)
                            .is_some_and(|s| !s.is_empty())
                    })
                    && !a
                        .get("fields")
                        .and_then(Value::as_array)
                        .is_some_and(|fields| !fields.is_empty())
        })
}

/// Pending images, newest messages first, including a read-only overlay of
/// changes not yet persisted by sync. Duplicate URLs retain their newest use.
pub fn images_to_fetch(conn: &Connection, limit: usize) -> Result<Vec<String>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "
            SELECT m.ts, m.id, i.url
            FROM media_images i
            JOIN messages m ON m.id = i.message_id
            WHERE NOT EXISTS (
                SELECT 1 FROM media_message_changes d WHERE d.message_id = i.message_id)
        ",
    )?;
    let mut candidates = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for message in scan_media_ordered(conn, i64::MAX, true)? {
        for url in message.images {
            candidates.push((message.ts.clone(), message.id, url));
        }
    }
    candidates.sort_unstable_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (_, _, url) in candidates {
        if seen.insert(url.clone()) && image_pending(conn, &url)? {
            out.push(url);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

/// Parsed on a reader, then installed with a short writer transaction.
pub struct MediaMessage {
    position: i64,
    id: i64,
    ts: String,
    pub(crate) team: String,
    pub(crate) images: std::collections::BTreeSet<String>,
    pub(crate) emoji: std::collections::BTreeSet<String>,
}

pub fn scan_media(conn: &Connection) -> Result<Vec<MediaMessage>> {
    scan_media_through(conn, i64::MAX)
}

/// Scan only positions present at the start of a refresh. Edits receive newer
/// positions, so continuously changing rows cannot keep that refresh alive.
pub fn scan_media_through(conn: &Connection, through: i64) -> Result<Vec<MediaMessage>> {
    scan_media_ordered(conn, through, false)
}

fn scan_media_ordered(
    conn: &Connection,
    through: i64,
    newest_first: bool,
) -> Result<Vec<MediaMessage>> {
    // Fixed SQL fragments only: queue scans use the position index; the bounded
    // image overlay needs the newest queued messages, regardless of enqueue order.
    let order = if newest_first {
        "m.ts DESC, m.id DESC"
    } else {
        "d.position"
    };
    let mut stmt = conn.prepare(&format!(
        "
            SELECT d.position, m.id, c.team_id, COALESCE(m.text, ''),
                COALESCE(m.raw_json, '{{}}'), m.deleted, m.ts
            FROM media_message_changes d
            JOIN messages m ON m.id = d.message_id
            JOIN conversations c ON c.id = m.conversation_id
            WHERE d.position <= ?1
            ORDER BY {order}
            LIMIT 1000
        ",
    ))?;
    let mut rows = stmt.query([through])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut message = MediaMessage {
            position: row.get(0)?,
            id: row.get(1)?,
            ts: row.get(6)?,
            team: row.get(2)?,
            images: Default::default(),
            emoji: Default::default(),
        };
        if !row.get::<_, bool>(5)? {
            let text: String = row.get(3)?;
            message
                .emoji
                .extend(crate::store::emoji::names_in(&text).map(str::to_owned));
            let raw: String = row.get(4)?;
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                for a in value
                    .get("attachments")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    message.images.extend(image_urls(a).map(str::to_owned));
                }
                if let Some(blocks) = value.get("blocks") {
                    message
                        .images
                        .extend(block_image_urls(blocks).into_iter().map(str::to_owned));
                }
            }
        }
        out.push(message);
    }
    Ok(out)
}

pub fn save_media_scan(conn: &mut Connection, messages: Vec<MediaMessage>) -> Result<()> {
    let tx = conn.transaction()?;
    for message in messages {
        // A concurrent edit/newer scan must not be replaced with an old snapshot.
        if !tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM media_message_changes WHERE position = ?1 AND message_id = ?2)",
            params![message.position, message.id], |r| r.get::<_, bool>(0),
        )? { continue; }
        tx.execute(
            "DELETE FROM media_images WHERE message_id = ?1",
            [message.id],
        )?;
        tx.execute(
            "DELETE FROM media_emoji WHERE message_id = ?1",
            [message.id],
        )?;
        for url in message.images {
            tx.execute(
                "INSERT INTO media_images(message_id, url) VALUES (?1, ?2)",
                params![message.id, url],
            )?;
        }
        for name in message.emoji {
            tx.execute(
                "INSERT INTO media_emoji(message_id, name) VALUES (?1, ?2)",
                params![message.id, name],
            )?;
        }
        tx.execute(
            "DELETE FROM media_message_changes WHERE position = ?1",
            [message.position],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// No filesystem work here: missing files are reconciled at startup/read time.
pub fn image_pending(conn: &Connection, url: &str) -> Result<bool> {
    Ok(!conn.query_row(
        "
            SELECT EXISTS (SELECT 1 FROM link_images
                WHERE hash = ?1
                AND (local_path IS NOT NULL OR failed = 1 OR retry_after > unixepoch()))
        ",
        [url_hash(url)],
        |r| r.get::<_, bool>(0),
    )?)
}

/// Records success, or a definitive refusal for compatibility with offline callers.
pub fn set_image(conn: &Connection, url: &str, path: Option<&str>) -> Result<()> {
    conn.execute(
        "
            INSERT INTO link_images(HASH, url, local_path, failed, fetched_at, failure_count,
                last_failure_at, retry_after)
            VALUES (?1, ?2, ?3, ?4, unixepoch(), ?4, CASE WHEN ?4 THEN unixepoch() END, 0)
            ON CONFLICT (HASH) DO UPDATE
            SET local_path = excluded.local_path, failed = excluded.failed, fetched_at =
                excluded.fetched_at, failure_count = excluded.failure_count,
                last_failure_at = excluded.last_failure_at, retry_after = 0
        ",
        params![url_hash(url), url, path, path.is_none()],
    )?;
    Ok(())
}

/// Transient errors wait one minute, doubling up to one day. `failed` means permanent.
pub fn record_image_failure(conn: &Connection, url: &str, permanent: bool) -> Result<()> {
    conn.execute(
        "
            INSERT INTO link_images(hash, url, local_path, failed, fetched_at,
                failure_count, last_failure_at, retry_after)
            VALUES (?1, ?2, NULL, ?3, unixepoch(), 1, unixepoch(), unixepoch() + 60)
            ON CONFLICT (hash) DO UPDATE
            SET local_path = NULL, failed = ?3, fetched_at = unixepoch(),
                failure_count = MIN(failure_count + 1, 31), last_failure_at = unixepoch(),
                retry_after = unixepoch() + MIN(86400, 60 * (1 << MIN(failure_count, 11)))
        ",
        params![url_hash(url), url, permanent],
    )?;
    Ok(())
}

// Read-only connections cannot invalidate a missing path. Queue exact stale
// values, then drain them on the next sync writer call (never clear a newer path).
type MissingImages = std::collections::HashSet<(String, String, String)>;
fn missing_images() -> &'static std::sync::Mutex<MissingImages> {
    static MISSING: std::sync::OnceLock<std::sync::Mutex<MissingImages>> =
        std::sync::OnceLock::new();
    MISSING.get_or_init(Default::default)
}

pub fn image_path(conn: &Connection, hash: &str) -> Result<Option<String>> {
    let path: Option<String> = conn
        .query_row(
            "SELECT local_path FROM link_images WHERE hash = ?1",
            [hash],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(path.filter(|path| {
        let exists = std::path::Path::new(path).is_file();
        if !exists {
            if let Ok(mut missing) = missing_images().lock() {
                missing.insert((
                    conn.path().unwrap_or_default().to_owned(),
                    hash.to_owned(),
                    path.clone(),
                ));
            }
        }
        exists
    }))
}

/// Whether `url` has an existing file (only checked on the display/read path).
pub fn is_cached(conn: &Connection, url: &str) -> bool {
    image_path(conn, &url_hash(url)).ok().flatten().is_some()
}

pub fn invalidate_missing(conn: &Connection, missing: &[(String, String)]) -> Result<()> {
    for (hash, path) in missing {
        conn.execute(
            "UPDATE link_images SET local_path = NULL WHERE hash = ?1 AND local_path = ?2",
            params![hash, path],
        )?;
        if let Ok(mut hints) = missing_images().lock() {
            hints.remove(&(
                conn.path().unwrap_or_default().to_owned(),
                hash.clone(),
                path.clone(),
            ));
        }
    }
    Ok(())
}

/// Hints are scoped to the database and carry the exact stale path.
pub fn missing_image_hints(conn: &Connection) -> Vec<(String, String)> {
    missing_images()
        .lock()
        .map(|m| {
            m.iter()
                .filter(|(db, _, _)| db == conn.path().unwrap_or_default())
                .map(|(_, hash, path)| (hash.clone(), path.clone()))
                .collect()
        })
        .unwrap_or_default()
}
