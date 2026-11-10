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
    /// Original free-text tokens, including quotes, exclusions and explicit prefixes.
    pub terms: Vec<String>,
    pub before_date: Option<String>,
    pub after_date: Option<String>,
    /// Agent callers resolve filters to ids before executing.
    pub resolved: bool,
    /// Original references retain sigils/case for ranked identity resolution.
    pub from_references: Vec<String>,
    pub conversation_references: Vec<String>,
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
    // FTS5 cannot take NUL (or other control characters) even inside a
    // quoted string: they separate words, like whitespace.
    let input: String = input
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut q = Query::default();
    let mut include = Vec::new();
    let mut exclude = Vec::new();
    for token in tokenize(&input) {
        let (key, value) = match token.split_once(':') {
            Some((k, v)) if !token.starts_with('"') && !v.is_empty() => (k.to_ascii_lowercase(), v),
            _ => (String::new(), token.as_str()),
        };
        match key.as_str() {
            "from" => {
                q.from_references.push(value.into());
                q.from.push(value.trim_start_matches('@').to_lowercase());
            }
            "in" => {
                q.conversation_references.push(value.into());
                q.in_conversations
                    .push(value.trim_start_matches(['#', '@']).to_lowercase());
            }
            "has" => match value.to_ascii_lowercase().as_str() {
                "link" => q.has_link = true,
                "file" => q.has_file = true,
                other => {
                    return Err(Error::Store(format!(
                        "unknown has:{other} (use has:link or has:file)"
                    )))
                }
            },
            "before" => {
                q.before = Some(day_ts(value, 0)?);
                q.before_date = Some(value.into());
            }
            "after" => {
                q.after = Some(day_ts(value, 1)?);
                q.after_date = Some(value.into());
            }
            _ => {
                q.terms.push(token.clone());
                if let Some(neg) = token.strip_prefix('-').filter(|t| !t.is_empty()) {
                    exclude.push(fts_term(neg));
                } else {
                    include.push(word_term(&token).unwrap_or_else(|| fts_term(&token)));
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

/// A bare word matches its other forms: `метод` finds `методів`, `методів`
/// finds `метод` (Ukrainian inflects heavily, and so does English a bit).
/// The word is cut to a rough stem and searched as a prefix. Only plain
/// words: quoted phrases, `word*` and anything with punctuation stay exact.
fn word_term(token: &str) -> Option<String> {
    if !token.chars().all(char::is_alphabetic) {
        return None;
    }
    let word = token.to_lowercase();
    Some(format!("\"{}\"*", stem(&word)))
}

/// Common endings, longest first. Cutting one is enough for a prefix search;
/// a stem never gets shorter than four letters.
const ENDINGS: &[&str] = &[
    // Ukrainian (and Russian) noun, adjective and verb endings.
    "ться", "ами", "ями", "ові", "еві", "ого", "ому", "ими", "ях", "ах", "ів", "їв", "ям", "ам",
    "ою", "ею", "ий", "ій", "ої", "им", "их", "ом", "ем", "ти", "ть", "ла", "ли", "ло", "ють",
    "ують", "ує", "а", "я", "и", "і", "ї", "е", "у", "ю", "о", "й", "ь", // English.
    "ing", "ed", "es", "s",
];

fn stem(word: &str) -> &str {
    let len = word.chars().count();
    ENDINGS
        .iter()
        .filter(|e| word.ends_with(*e) && len - e.chars().count() >= 4)
        .max_by_key(|e| e.chars().count())
        .map_or(word, |e| &word[..word.len() - e.len()])
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
    let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .filter(|d| d.format("%Y-%m-%d").to_string() == date)
        .ok_or_else(bad)?;
    let date = date
        .checked_add_signed(chrono::Duration::days(plus_days))
        .ok_or_else(bad)?;
    let ts = date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
    if ts < 0 {
        return Err(bad());
    }
    Ok(format!("{ts:010}.000000"))
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
    search_page(conn, team_id, q, sort, limit, 0, markers)
}

/// Offset-based page with deterministic tie-breaking for opaque agent cursors.
#[allow(clippy::too_many_arguments)]
pub fn search_page(
    conn: &Connection,
    team_id: &str,
    q: &Query,
    sort: Sort,
    limit: usize,
    offset: usize,
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
            let p = arg(name.to_lowercase().into());
            if q.resolved {
                ors.push(format!("lower(m.user_id) = {p}"));
                continue;
            }
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
            let p = arg(name.to_lowercase().into());
            if q.resolved {
                ors.push(format!("lower(c.id) = {p}"));
                continue;
            }
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
        (Sort::Relevance, true) => " ORDER BY bm25(messages_fts), m.ts DESC, m.conversation_id",
        _ => " ORDER BY m.ts DESC, m.conversation_id",
    });
    sql.push_str(&format!(
        " LIMIT {}",
        arg(i64::try_from(limit).unwrap_or(i64::MAX).into())
    ));

    sql.push_str(&format!(
        " OFFSET {}",
        arg(i64::try_from(offset).unwrap_or(i64::MAX).into())
    ));
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
        assert_eq!(q.fts, r#""grad"* "big sheet" "draft"* NOT "old""#);
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
    fn date_bounds_validate_calendar_days() {
        assert_eq!(day_ts("1970-01-01", 0).unwrap(), "0000000000.000000");
        assert_eq!(day_ts("2024-02-29", 1).unwrap(), "1709251200.000000");
        for date in ["2023-02-29", "2024-02-30", "2024-04-31", "2024-2-3"] {
            assert!(day_ts(date, 0).is_err(), "{date}");
        }
    }
}
