//! Indexed, offline links/files search with keyset pagination.

use rusqlite::{params_from_iter, types::Value, Connection};
use serde::{Deserialize, Serialize};

use crate::{
    store::search::{self, Hit},
    Error, Result,
};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaterialSort {
    #[default]
    Newest,
    Oldest,
    Name,
}

#[derive(Serialize)]
pub struct Material {
    #[serde(flatten)]
    pub message: Hit,
    pub kind: String,
    /// A share's file id, or the original HTTP(S) URL for a link.
    pub key: String,
    pub title: String,
    pub url: Option<String>,
}

#[derive(Serialize)]
pub struct MaterialPage {
    pub items: Vec<Material>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    team: String,
    query: String,
    sort: MaterialSort,
    value: String,
    id: i64,
}

/// Uses the same query parser and filter semantics as message search. Free
/// words match material labels/URLs as well as their message's text. Empty
/// input browses every stored material. has:link/file selects material kinds
/// (both together include both kinds). Each share retains its own context.
pub fn materials(
    conn: &Connection,
    team: &str,
    input: &str,
    sort: MaterialSort,
    limit: usize,
    cursor: Option<&str>,
) -> Result<MaterialPage> {
    let q = search::parse(input)?;
    let cursor = cursor
        .map(serde_json::from_str::<Cursor>)
        .transpose()
        .map_err(|_| Error::Usage("invalid materials cursor".into()))?;
    if cursor
        .as_ref()
        .is_some_and(|c| c.team != team || c.query != input || c.sort != sort)
    {
        return Err(Error::Usage(
            "materials cursor belongs to another search".into(),
        ));
    }
    let limit = limit.clamp(1, 200);
    let mut args: Vec<Value> = Vec::new();
    let mut arg = |v: Value| {
        args.push(v);
        format!("?{}", args.len())
    };
    let text = !q.fts.is_empty();
    let index = if sort == MaterialSort::Name {
        "materials_name"
    } else {
        "materials_recent"
    };
    let source = format!("search_materials a INDEXED BY {index}");
    // Uncorrelated id sets keep the sort index covering and evaluate each FTS
    // MATCH once per page. Filter columns are covered by both sort indexes,
    // so even sparse author/date filters avoid random table reads. A JOIN or
    // EXISTS can rerun MATCH for every skipped row on later pages. Hydrate
    // messages only after the bounded page is selected.
    let mut sql = format!(
        "WITH page AS MATERIALIZED (SELECT a.id FROM {source} WHERE a.team_id = {}",
        arg(team.to_owned().into())
    );
    if text {
        sql.push_str(&format!(
            " AND a.id IN (SELECT rowid FROM materials_fts WHERE materials_fts MATCH {})",
            arg(q.fts.into())
        ));
    }
    if q.has_link != q.has_file {
        sql.push_str(if q.has_link {
            " AND a.kind = 'link'"
        } else {
            " AND a.kind = 'file'"
        });
    }
    if !q.in_conversations.is_empty() {
        let mut ors = Vec::new();
        for name in q.in_conversations {
            let p = arg(name.into());
            ors.push(format!(
                "lower(label) = {p} OR lower(name) = {p} OR lower(id) = {p}"
            ));
        }
        sql.push_str(&format!(
            " AND a.conversation_id IN (SELECT id FROM conversations WHERE team_id = {} AND ({}))",
            arg(team.to_owned().into()),
            ors.join(" OR ")
        ));
    }
    if !q.from.is_empty() {
        let mut ors = Vec::new();
        for name in q.from {
            let p = arg(name.into());
            ors.push(format!("a.user_id IN (SELECT id FROM users WHERE team_id = {} AND
                (lower(name) = {p} OR lower(display_name) = {p} OR lower(real_name) = {p} OR lower(id) = {p}))
                OR lower(a.author_name) = {p}", arg(team.to_owned().into())));
        }
        sql.push_str(&format!(" AND ({})", ors.join(" OR ")));
    }
    if let Some(before) = q.before {
        sql.push_str(&format!(" AND a.ts < {}", arg(before.into())));
    }
    if let Some(after) = q.after {
        sql.push_str(&format!(" AND a.ts >= {}", arg(after.into())));
    }
    let column = if sort == MaterialSort::Name {
        "a.title_key"
    } else {
        "a.ts"
    };
    let desc = sort == MaterialSort::Newest;
    if let Some(cursor) = cursor {
        sql.push_str(&format!(
            " AND ({column}, a.id) {} ({}, {})",
            if desc { "<" } else { ">" },
            arg(cursor.value.into()),
            arg(cursor.id.into())
        ));
    }
    let direction = if desc { "DESC" } else { "ASC" };
    sql.push_str(&format!(
        " ORDER BY {column} {direction}, a.id {direction} LIMIT {}",
        arg(((limit + 1) as i64).into())
    ));
    sql.push_str(&format!(
        ") SELECT a.id, a.title_key, a.kind, a.material_key, a.title, a.url,
            m.conversation_id, COALESCE(c.label, c.name, c.id), m.ts, m.thread_ts,
            m.user_id, COALESCE(u.display_name, u.name, m.author_name), m.hidden_by_limit
         FROM page
         JOIN search_materials a ON a.id = page.id
         JOIN messages m ON m.id = a.message_id
         JOIN conversations c ON c.id = m.conversation_id
         LEFT JOIN users u ON u.id = m.user_id
         ORDER BY {column} {direction}, a.id {direction}"
    ));
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt
        .query_map(params_from_iter(args), |r| {
            let material = Material {
                kind: r.get(2)?,
                key: r.get(3)?,
                title: crate::files::display_name(&r.get::<_, String>(4)?),
                url: r.get(5)?,
                message: Hit {
                    conversation_id: r.get(6)?,
                    conversation: r.get(7)?,
                    ts: r.get(8)?,
                    thread_ts: r.get(9)?,
                    user_id: r.get(10)?,
                    author: r.get(11)?,
                    hidden_by_limit: r.get(12)?,
                    snippet: String::new(),
                },
            };
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, material))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    let next_cursor = if more {
        rows.last()
            .map(|(id, title_key, m)| {
                serde_json::to_string(&Cursor {
                    team: team.into(),
                    query: input.into(),
                    sort,
                    id: *id,
                    value: if sort == MaterialSort::Name {
                        title_key.clone()
                    } else {
                        m.message.ts.clone()
                    },
                })
            })
            .transpose()?
    } else {
        None
    };
    Ok(MaterialPage {
        items: rows.into_iter().map(|(_, _, material)| material).collect(),
        next_cursor,
    })
}
