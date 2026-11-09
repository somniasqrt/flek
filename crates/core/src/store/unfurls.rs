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

/// Preview images not tried yet, newest messages first.
pub fn images_to_fetch(conn: &Connection, limit: usize) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT m.raw_json FROM messages m
         WHERE m.deleted = 0 AND m.raw_json LIKE '%\"attachments\"%'
         ORDER BY m.ts DESC",
    )?;
    let mut known = conn.prepare_cached("SELECT 1 FROM link_images WHERE hash = ?1")?;
    let mut out: Vec<String> = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let raw: String = r.get(0)?;
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        for a in v
            .get("attachments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for url in image_urls(a) {
                if out.iter().any(|u| u == url) || known.exists([url_hash(url)])? {
                    continue;
                }
                out.push(url.to_owned());
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }
    }
    Ok(out)
}

/// Records a fetched image (`Some(path)`) or a failed one (`None`, not retried).
pub fn set_image(conn: &Connection, url: &str, path: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO link_images (hash, url, local_path, failed, fetched_at)
         VALUES (?1, ?2, ?3, ?4, unixepoch())
         ON CONFLICT(hash) DO UPDATE SET local_path = excluded.local_path,
            failed = excluded.failed, fetched_at = excluded.fetched_at",
        params![url_hash(url), url, path, path.is_none()],
    )?;
    Ok(())
}

pub fn image_path(conn: &Connection, hash: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT local_path FROM link_images WHERE hash = ?1",
            [hash],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
}

/// Whether `url` is cached (for [`from_raw`]).
pub fn is_cached(conn: &Connection, url: &str) -> bool {
    conn.prepare_cached("SELECT 1 FROM link_images WHERE hash = ?1 AND local_path IS NOT NULL")
        .and_then(|mut s| s.exists([url_hash(url)]))
        .unwrap_or(false)
}
