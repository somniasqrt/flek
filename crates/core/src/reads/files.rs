//! Offline files reads.

use crate::{views, Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Serialize)]
pub struct File {
    #[serde(flatten)]
    pub file: views::File,
    pub conversation_id: Option<String>,
    pub ts: Option<String>,
    pub user_id: Option<String>,
    pub permalink: Option<String>,
    pub hidden_by_limit: bool,
    pub title: Option<String>,
}

#[derive(Default)]
pub struct FileFilter {
    pub id: Option<String>,
    pub channel: Option<String>,
    pub user: Option<String>,
    pub kind: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: usize,
    pub query: Option<String>,
    pub sort: Option<String>,
    pub mine: bool,
    pub offset: usize,
    pub other_includes_media: bool,
}

pub fn files(c: &Connection, team: &str, f: &FileFilter) -> Result<Vec<File>> {
    if f.kind.as_deref().is_some_and(|k| {
        !["image", "pdf", "video", "audio", "doc", "other"].contains(&k)
            && (!k.contains('/') || k.chars().any(char::is_whitespace))
    }) {
        return Err(Error::Usage(
            "--type expects image, pdf, video, audio, doc, other or a mimetype prefix".into(),
        ));
    }

    let mut stmt = c.prepare(
        "
            SELECT f.id, f.name, f.mimetype, f.size, f.local_path, m.conversation_id, m.ts, m.user_id,
                t.url, f.hidden_by_limit, m.thread_ts, f.title
            FROM message_files mf
            JOIN files f ON f.id = mf.file_id
            JOIN messages m ON m.conversation_id = mf.conversation_id
            AND m.ts = mf.ts
            JOIN conversations c ON c.id = m.conversation_id
            JOIN teams t ON t.id = c.team_id
            WHERE c.team_id = ?1
            AND m.deleted = 0
            AND (?2 IS NULL
                OR c.id = ?2)
            AND (?3 IS NULL
                OR m.user_id = ?3)
            AND (?4 IS NULL
                OR m.ts >= ?4)
            AND (?5 IS NULL
                OR m.ts <= ?5)
            AND (?6 IS NULL
                OR f.id = ?6)
            AND (?7 IS NULL
                OR (instr(?7, '/') > 0
                    AND substr(COALESCE(f.mimetype, ''), 1, length(?7)) = ?7)
                OR CASE
                WHEN f.mimetype LIKE 'image/%' THEN 'image'
                WHEN f.mimetype = 'application/pdf' THEN 'pdf'
                WHEN f.mimetype LIKE 'video/%' THEN 'video'
                WHEN f.mimetype LIKE 'audio/%' THEN 'audio'
                WHEN f.mimetype LIKE 'text/%'
                OR f.mimetype LIKE '%word%'
                OR f.mimetype LIKE '%document%'
                OR f.mimetype LIKE '%excel%'
                OR f.mimetype LIKE '%spreadsheet%'
                OR f.mimetype LIKE '%presentation%'
                OR f.mimetype LIKE '%powerpoint%' THEN 'doc'
                ELSE 'other' END = ?7
                OR (?13
                    AND ?7 = 'other'
                    AND (f.mimetype LIKE 'video/%'
                        OR f.mimetype LIKE 'audio/%')))
            AND (?9 IS NULL
                OR instr(lower(COALESCE(f.name, '') || ' ' || COALESCE(f.title, '')), lower(?9))
                    > 0)
            AND (NOT ?10
                OR m.user_id = t.self_user_id)
            ORDER BY CASE
            WHEN ?11 = 'name' THEN lower(COALESCE(f.title, f.name)) END, CASE
            WHEN ?11 = 'size' THEN f.size END DESC, m.ts DESC, f.id
            LIMIT ?8
            OFFSET ?12
        ",
    )?;
    let rows = stmt.query_map(
        params![
            team,
            f.channel,
            f.user,
            f.since,
            f.until,
            f.id,
            f.kind,
            i64::try_from(f.limit).unwrap_or(i64::MAX),
            f.query,
            f.mine,
            f.sort,
            f.offset as i64,
            f.other_includes_media
        ],
        |r| {
            let id: String = r.get(5)?;
            let ts: String = r.get(6)?;
            let url: Option<String> = r.get(8)?;
            let thread: Option<String> = r.get::<_, Option<String>>(10)?.filter(|root| root != &ts);
            Ok(File {
                file: views::File {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    mimetype: r.get(2)?,
                    size: r.get(3)?,
                    local_path: r
                        .get::<_, Option<String>>(4)?
                        .filter(|p| std::path::Path::new(p).is_file()),
                },
                permalink: url.map(|u| {
                    let mut link = format!(
                        "{}/archives/{id}/p{}",
                        u.trim_end_matches('/'),
                        ts.replace('.', "")
                    );
                    if let Some(root) = thread {
                        link.push_str(&format!("?thread_ts={root}&cid={id}"));
                    }
                    link
                }),
                conversation_id: Some(id),
                ts: Some(ts),
                user_id: r.get(7)?,
                hidden_by_limit: r.get(9)?,
                title: r.get(11)?,
            })
        },
    )?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// A file may have multiple shares; choose the newest one in this team.
pub fn file(c: &Connection, team: &str, id: &str) -> Result<File> {
    let mut f = files(
        c,
        team,
        &FileFilter {
            id: Some(id.into()),
            limit: 1,
            ..Default::default()
        },
    )?
    .into_iter()
    .find(|f| f.file.id == id)
    .ok_or_else(|| Error::NotFound(format!("no stored file {id:?}")))?;
    if f.hidden_by_limit {
        return Err(Error::NotFound(format!(
            "file {id:?} is hidden by the Slack Free limit"
        )));
    }
    if f.file
        .local_path
        .as_ref()
        .is_some_and(|p| !std::path::Path::new(p).is_file())
    {
        f.file.local_path = None;
    }
    Ok(f)
}

pub fn pending_file(c: &Connection, id: &str) -> Result<crate::store::files::PendingDownload> {
    c.query_row(
        "
            SELECT id, name, mimetype, SIZE, COALESCE(url_private_download, url_private),
                local_path
            FROM files
            WHERE id = ?1
            AND hidden_by_limit = 0
            AND is_external = 0
            AND COALESCE(url_private_download, url_private) IS NOT NULL
        ",
        [id],
        |r| {
            Ok(crate::store::files::PendingDownload {
                id: r.get(0)?,
                name: r.get(1)?,
                mimetype: r.get(2)?,
                size: r.get(3)?,
                url: r.get(4)?,
                local_path: r.get(5)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("file {id:?} has no downloadable stored URL")))
}
