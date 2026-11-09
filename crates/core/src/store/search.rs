//! Local full-text search over messages (FTS5) with Slack-like filters:
//! `from:alice in:#general has:link has:file before:2024-01-31 after:2023-12-31`.

use rusqlite::types::Value as SqlValue;
use rusqlite::{params_from_iter, Connection};

use crate::{Error, Result};

/// A parsed search query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// FTS5 MATCH expression built from the free-text part (empty = filters only).
    pub fts: String,
    pub from: Vec<String>,
    pub in_conversations: Vec<String>,
    pub has_link: bool,
    pub has_file: bool,
    /// Exclusive upper bound (ts of the start of the `before:` day, UTC).
    pub before: Option<String>,
    /// Inclusive lower bound (ts of the start of the day after `after:`, UTC).
    pub after: Option<String>,
}

/// Parses the user's query. Free-text terms are quoted for FTS5, so
/// punctuation never breaks the query; `word*` is a prefix search, `"two
/// words"` a phrase, `-word` excludes.
pub fn parse(input: &str) -> Result<Query> {
    let mut q = Query::default();
    let mut include = Vec::new();
    let mut exclude = Vec::new();
    for token in tokenize(input) {
        let (key, value) = match token.split_once(':') {
            Some((k, v)) if !token.starts_with('"') && !v.is_empty() => (k.to_ascii_lowercase(), v),
            _ => (String::new(), token.as_str()),
        };
        match key.as_str() {
            "from" => q.from.push(value.trim_start_matches('@').to_lowercase()),
            "in" => q
                .in_conversations
                .push(value.trim_start_matches(['#', '@']).to_lowercase()),
            "has" => match value.to_ascii_lowercase().as_str() {
                "link" => q.has_link = true,
                "file" => q.has_file = true,
                other => {
                    return Err(Error::Store(format!(
                        "unknown has:{other} (use has:link or has:file)"
                    )))
                }
            },
            "before" => q.before = Some(day_ts(value, 0)?),
            "after" => q.after = Some(day_ts(value, 1)?),
            _ => {
                if let Some(neg) = token.strip_prefix('-').filter(|t| !t.is_empty()) {
                    exclude.push(fts_term(neg));
                } else {
                    include.push(fts_term(&token));
                }
            }
        }
    }
    if include.is_empty() && !exclude.is_empty() {
        return Err(Error::Store(
            "a query needs at least one word that is not excluded".into(),
        ));
    }
    q.fts = include.join(" ");
    for e in exclude {
        q.fts.push_str(" NOT ");
        q.fts.push_str(&e);
    }
    Ok(q)
}

/// Splits on whitespace, keeping `"quoted phrases"` (with their quotes) together.
fn tokenize(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in input.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn fts_term(token: &str) -> String {
    let (body, prefix) = match token.strip_suffix('*') {
        Some(b) if !token.starts_with('"') => (b, true),
        _ => (token, false),
    };
    let body = body.trim_matches('"').replace('"', "\"\"");
    format!("\"{body}\"{}", if prefix { "*" } else { "" })
}

/// `YYYY-MM-DD` (+ `plus_days`) → ts of that day's midnight, UTC.
fn day_ts(date: &str, plus_days: i64) -> Result<String> {
    let bad = || Error::Store(format!("bad date {date:?}, expected YYYY-MM-DD"));
    let mut parts = date
        .splitn(3, '-')
        .map(|p| p.parse::<i64>().map_err(|_| bad()));
    let (y, m, d) = (
        parts.next().ok_or_else(bad)??,
        parts.next().ok_or_else(bad)??,
        parts.next().ok_or_else(bad)??,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(bad());
    }
    let days = days_from_civil(y, m, d) + plus_days;
    Ok(format!("{:010}.000000", (days * 86_400).max(0)))
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// One search result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Hit {
    pub conversation_id: String,
    pub conversation: String,
    pub ts: String,
    pub thread_ts: Option<String>,
    /// Author's user id (for the avatar); `None` for bots and imported messages.
    pub user_id: Option<String>,
    pub author: Option<String>,
    /// Text around the match with `open`/`close` markers around matched terms.
    pub snippet: String,
    pub hidden_by_limit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    /// Best match first (bm25); newest first when there is no free text.
    #[default]
    Relevance,
    Recent,
}

/// Runs a query against one team's messages.
pub fn search(
    conn: &Connection,
    team_id: &str,
    q: &Query,
    sort: Sort,
    limit: usize,
    markers: (&str, &str),
) -> Result<Vec<Hit>> {
    let mut args: Vec<SqlValue> = Vec::new();
    let mut arg = |v: SqlValue| {
        args.push(v);
        format!("?{}", args.len())
    };
    let text_search = !q.fts.is_empty();

    let snippet = if text_search {
        format!(
            "snippet(messages_fts, 0, {}, {}, '…', 16)",
            arg(markers.0.to_owned().into()),
            arg(markers.1.to_owned().into())
        )
    } else {
        "substr(COALESCE(m.text, ''), 1, 200)".to_owned()
    };
    let mut sql = format!(
        "SELECT m.conversation_id, COALESCE(c.label, c.name, c.id), m.ts, m.thread_ts,
                COALESCE(u.display_name, u.name, m.author_name), {snippet}, m.hidden_by_limit,
                m.user_id
         FROM messages m
         JOIN conversations c ON c.id = m.conversation_id
         LEFT JOIN users u ON u.id = m.user_id"
    );
    if text_search {
        sql.push_str(" JOIN messages_fts ON messages_fts.rowid = m.id");
    }
    sql.push_str(&format!(
        " WHERE c.team_id = {} AND m.deleted = 0",
        arg(team_id.to_owned().into())
    ));
    if text_search {
        sql.push_str(&format!(
            " AND messages_fts MATCH {}",
            arg(q.fts.clone().into())
        ));
    }
    if !q.from.is_empty() {
        let mut ors = Vec::new();
        for name in &q.from {
            let p = arg(name.clone().into());
            ors.push(format!(
                "m.user_id IN (SELECT id FROM users WHERE lower(name) = {p} OR lower(display_name) = {p}
                               OR lower(real_name) = {p} OR lower(id) = {p})
                 OR lower(m.author_name) = {p}"
            ));
        }
        sql.push_str(&format!(" AND ({})", ors.join(" OR ")));
    }
    if !q.in_conversations.is_empty() {
        let mut ors = Vec::new();
        for name in &q.in_conversations {
            let p = arg(name.clone().into());
            ors.push(format!(
                "lower(c.label) = {p} OR lower(c.name) = {p} OR lower(c.id) = {p}"
            ));
        }
        sql.push_str(&format!(" AND ({})", ors.join(" OR ")));
    }
    if q.has_link {
        sql.push_str(" AND m.text LIKE '%http%'");
    }
    if q.has_file {
        sql.push_str(
            " AND (EXISTS (SELECT 1 FROM message_files f WHERE f.conversation_id = m.conversation_id AND f.ts = m.ts)
                   OR m.text LIKE '%[file]%')",
        );
    }
    if let Some(b) = &q.before {
        sql.push_str(&format!(" AND m.ts < {}", arg(b.clone().into())));
    }
    if let Some(a) = &q.after {
        sql.push_str(&format!(" AND m.ts >= {}", arg(a.clone().into())));
    }
    sql.push_str(match (sort, text_search) {
        (Sort::Relevance, true) => " ORDER BY bm25(messages_fts), m.ts DESC",
        _ => " ORDER BY m.ts DESC",
    });
    sql.push_str(&format!(" LIMIT {}", arg((limit as i64).into())));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(args), |r| {
            Ok(Hit {
                conversation_id: r.get(0)?,
                conversation: r.get(1)?,
                ts: r.get(2)?,
                thread_ts: r.get(3)?,
                author: r.get(4)?,
                snippet: r.get(5)?,
                hidden_by_limit: r.get(6)?,
                user_id: r.get(7)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_filters_and_terms() {
        let q = parse(r#"from:@Alice in:#General has:link has:FILE before:2024-01-31 after:2023-12-31 grades "big sheet" draft* -old"#).unwrap();
        assert_eq!(q.from, ["alice"]);
        assert_eq!(q.in_conversations, ["general"]);
        assert!(q.has_link && q.has_file);
        assert_eq!(q.before.as_deref(), Some("1706659200.000000"));
        assert_eq!(q.after.as_deref(), Some("1704067200.000000"));
        assert_eq!(q.fts, r#""grades" "big sheet" "draft"* NOT "old""#);
    }

    #[test]
    fn punctuation_is_safe() {
        assert_eq!(parse(r#"a"b c:"#).unwrap().fts, r#""a""b c:""#);
        assert_eq!(
            parse("http://x.example/y?z=1").unwrap().fts,
            r#""http://x.example/y?z=1""#
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("before:2024-13-01").is_err());
        assert!(parse("has:emoji").is_err());
        assert!(parse("-only").is_err());
    }

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
    }
}
