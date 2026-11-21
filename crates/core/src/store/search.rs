//! Local full-text search over messages (FTS5) with Slack-like filters:
//! `from:alice from:me -from:bob in:#general -in:#random with:alice has:link has:file has:pin has::tada: hasmy::tada: is:thread is:saved before:2024-01-31 after:2023-12-31 on:2024-03-05 during:2024-03`.

use rusqlite::types::Value as SqlValue;
use rusqlite::{params_from_iter, Connection, OptionalExtension};

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
    pub on_date: Option<String>,
    pub during_date: Option<String>,
    /// Agent callers resolve filters to ids before executing.
    pub resolved: bool,
    /// Original references retain sigils/case for ranked identity resolution.
    pub from_references: Vec<String>,
    pub exclude_from_references: Vec<String>,
    pub conversation_references: Vec<String>,
    pub exclude_conversation_references: Vec<String>,
    pub with_references: Vec<String>,
    pub from: Vec<String>,
    /// `from:me` — resolved against `teams.self_user_id` at prepare/query time.
    pub from_me: bool,
    pub exclude_from: Vec<String>,
    /// `-from:me` — resolved against `teams.self_user_id` at prepare/query time.
    pub exclude_from_me: bool,
    pub in_conversations: Vec<String>,
    pub exclude_in_conversations: Vec<String>,
    pub with: Vec<String>,
    pub has_link: bool,
    pub has_file: bool,
    pub has_pin: bool,
    pub has_reactions: Vec<String>,
    pub has_my_reactions: Vec<String>,
    pub is_thread: bool,
    pub is_saved: bool,
    /// Exclusive upper bound (ts of the start of the `before:` day, UTC).
    pub before: Option<String>,
    /// Inclusive lower bound (ts of the start of the day after `after:`, UTC).
    pub after: Option<String>,
}

/// Resolves the signed-in user id recorded for `team_id` (used by `from:me`).
pub fn resolve_self_user_id(conn: &Connection, team_id: &str) -> Result<String> {
    resolve_self_user_id_for(conn, team_id, "from:me")
}

/// Resolves `teams.self_user_id`, naming `modifier` in the error when missing.
pub(crate) fn resolve_self_user_id_for(
    conn: &Connection,
    team_id: &str,
    modifier: &str,
) -> Result<String> {
    conn.query_row(
        "SELECT self_user_id FROM teams WHERE id = ?1 AND self_user_id != ''",
        [team_id],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .filter(|id| !id.trim().is_empty())
    .ok_or_else(|| {
        Error::NotFound(format!(
            "cannot resolve {modifier}: no current user is recorded for this team"
        ))
    })
}

/// First modifier on `q` that requires `teams.self_user_id`, if any.
pub(crate) fn self_modifier(q: &Query) -> Option<&'static str> {
    if q.from_me {
        Some("from:me")
    } else if q.exclude_from_me {
        Some("-from:me")
    } else if !q.has_my_reactions.is_empty() {
        Some("hasmy:")
    } else if q.is_saved {
        Some("is:saved")
    } else {
        None
    }
}

/// SQL predicate matching messages that belong to a thread: a root with
/// replies (`reply_count > 0`), a reply (`thread_ts` pointing to another ts),
/// or a root with at least one stored non-deleted reply (the `EXISTS` branch
/// covers imported archives whose roots may lack `reply_count`, and is served
/// by `messages_thread(conversation_id, thread_ts)`).
pub(crate) fn in_thread_sql(alias: &str) -> String {
    format!(
        "({alias}.reply_count > 0 \
         OR ({alias}.thread_ts IS NOT NULL AND {alias}.thread_ts != {alias}.ts) \
         OR EXISTS (SELECT 1 FROM messages r \
                    WHERE r.conversation_id = {alias}.conversation_id \
                    AND r.thread_ts = {alias}.ts \
                    AND r.ts != {alias}.ts \
                    AND r.deleted = 0))"
    )
}

/// SQL predicate matching a message in a thread where a target person authored
/// at least one non-deleted message (root or reply).
///
/// Split into two `EXISTS` subqueries over `tm.ts = root` and
/// `tm.thread_ts = root` (with `root = COALESCE(m.thread_ts, m.ts)` computed
/// from the outer row) instead of `COALESCE(tm.thread_ts, tm.ts) = root`: an
/// expression on `tm` cannot use an index and would scan the whole
/// conversation, whereas the two branches seek `UNIQUE(conversation_id, ts)`
/// for the root and `messages_thread`/`messages_thread_activity`
/// `(conversation_id, thread_ts)` for replies.
pub(crate) fn thread_participant_sql(msg_alias: &str, author_predicate: &str) -> String {
    let in_thread = in_thread_sql(msg_alias);
    let root = format!("COALESCE({msg_alias}.thread_ts, {msg_alias}.ts)");
    format!(
        "({in_thread} AND (\
            EXISTS (SELECT 1 FROM messages tm \
                    WHERE tm.conversation_id = {msg_alias}.conversation_id \
                      AND tm.ts = {root} \
                      AND tm.deleted = 0 \
                      AND ({author_predicate})) \
            OR EXISTS (SELECT 1 FROM messages tm \
                       WHERE tm.conversation_id = {msg_alias}.conversation_id \
                         AND tm.thread_ts = {root} \
                         AND tm.thread_ts IS NOT NULL \
                         AND tm.deleted = 0 \
                         AND ({author_predicate}))))"
    )
}

/// Escapes `\`, `%`, and `_` for SQLite `LIKE ... ESCAPE '\'`.
pub(crate) fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Appends one author-matching predicate for a `messages` / `search_materials`
/// alias. Each branch guards against `NULL` so negating the disjunction with
/// `NOT` never drops rows with `NULL` `user_id` or `author_name`.
pub(crate) fn push_author_match(
    ors: &mut Vec<String>,
    alias: &str,
    team_id: &str,
    name: &str,
    resolved: bool,
    arg: &mut impl FnMut(SqlValue) -> String,
) {
    let p = arg(name.to_lowercase().into());
    if resolved {
        ors.push(format!(
            "({alias}.user_id IS NOT NULL AND lower({alias}.user_id) = {p})"
        ));
    } else {
        let team_p = arg(team_id.to_owned().into());
        ors.push(format!(
            "({alias}.user_id IS NOT NULL AND {alias}.user_id IN (\
                 SELECT id FROM users \
                 WHERE team_id = {team_p} \
                   AND (lower(name) = {p} \
                     OR lower(display_name) = {p} \
                     OR lower(real_name) = {p} \
                     OR lower(id) = {p})) \
             OR ({alias}.author_name IS NOT NULL AND lower({alias}.author_name) = {p}))"
        ));
    }
}

/// Appends one 1:1 DM partner predicate for a `conversations` alias.
pub(crate) fn push_dm_user_match(
    ors: &mut Vec<String>,
    conv_alias: &str,
    team_id: &str,
    name: &str,
    resolved: bool,
    arg: &mut impl FnMut(SqlValue) -> String,
) {
    let p = arg(name.to_lowercase().into());
    if resolved {
        ors.push(format!(
            "({conv_alias}.dm_user_id IS NOT NULL AND lower({conv_alias}.dm_user_id) = {p})"
        ));
    } else {
        let team_p = arg(team_id.to_owned().into());
        ors.push(format!(
            "({conv_alias}.dm_user_id IS NOT NULL AND (\
                 lower({conv_alias}.dm_user_id) = {p} \
                 OR {conv_alias}.dm_user_id IN (\
                     SELECT id FROM users \
                     WHERE team_id = {team_p} \
                       AND (lower(name) = {p} \
                         OR lower(display_name) = {p} \
                         OR lower(real_name) = {p} \
                         OR lower(id) = {p}))))"
        ));
    }
}

/// SQL `EXISTS` predicate for `has::emoji:` / `hasmy::emoji:`, matching either
/// the exact reaction name or any `name::skin-tone-*` variant.
pub(crate) fn reaction_exists_sql(
    alias: &str,
    emoji: &str,
    user_id: Option<&str>,
    arg: &mut impl FnMut(SqlValue) -> String,
) -> String {
    let p_exact = arg(emoji.to_owned().into());
    let p_skin = arg(format!("{}::skin-tone-%", escape_like(emoji)).into());
    let user_clause = match user_id {
        Some(uid) => format!(" AND rx.user_id = {}", arg(uid.to_owned().into())),
        None => String::new(),
    };
    format!(
        "EXISTS (SELECT 1 FROM reactions rx \
         WHERE rx.conversation_id = {alias}.conversation_id \
           AND rx.ts = {alias}.ts{user_clause} \
           AND (rx.name = {p_exact} OR rx.name LIKE {p_skin} ESCAPE '\\'))"
    )
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
        if let Some((key, value)) = split_filter(&token) {
            match key.as_str() {
                "from" => {
                    if value.eq_ignore_ascii_case("me") {
                        q.from_me = true;
                    } else {
                        q.from_references.push(value.into());
                        q.from.push(value.trim_start_matches('@').to_lowercase());
                    }
                }
                "-from" => {
                    if value.eq_ignore_ascii_case("me") {
                        q.exclude_from_me = true;
                    } else {
                        q.exclude_from_references.push(value.into());
                        q.exclude_from
                            .push(value.trim_start_matches('@').to_lowercase());
                    }
                }
                "in" => {
                    q.conversation_references.push(value.into());
                    q.in_conversations
                        .push(value.trim_start_matches(['#', '@']).to_lowercase());
                }
                "-in" => {
                    q.exclude_conversation_references.push(value.into());
                    q.exclude_in_conversations
                        .push(value.trim_start_matches(['#', '@']).to_lowercase());
                }
                "with" => {
                    if value.eq_ignore_ascii_case("me") {
                        return Err(Error::Usage("with:me is not supported".into()));
                    }
                    q.with_references.push(value.into());
                    q.with.push(value.trim_start_matches('@').to_lowercase());
                }
                "has" => match value.to_ascii_lowercase().as_str() {
                    "link" => q.has_link = true,
                    "file" => q.has_file = true,
                    "pin" => q.has_pin = true,
                    _ if value.starts_with(':') => {
                        let name = parse_emoji_value("has", value)?;
                        if !q.has_reactions.contains(&name) {
                            q.has_reactions.push(name);
                        }
                    }
                    other => {
                        return Err(Error::Usage(format!(
                            "unknown has:{other} (use has:link, has:file, has:pin, has::emoji:)"
                        )))
                    }
                },
                "hasmy" => {
                    let name = parse_emoji_value("hasmy", value)?;
                    if !q.has_my_reactions.contains(&name) {
                        q.has_my_reactions.push(name);
                    }
                }
                "is" => match value.to_ascii_lowercase().as_str() {
                    "thread" => q.is_thread = true,
                    "saved" => q.is_saved = true,
                    other => {
                        return Err(Error::Usage(format!(
                            "unknown is:{other} (use is:thread, is:saved)"
                        )))
                    }
                },
                "before" => {
                    let ts = day_ts(value, 0)?;
                    tighten_before(&mut q.before, ts);
                    q.before_date = Some(match q.before_date.take() {
                        Some(old) if old.as_str() < value => old,
                        _ => value.into(),
                    });
                }
                "after" => {
                    let ts = day_ts(value, 1)?;
                    tighten_after(&mut q.after, ts);
                    q.after_date = Some(match q.after_date.take() {
                        Some(old) if old.as_str() > value => old,
                        _ => value.into(),
                    });
                }
                "on" => {
                    let start = day_ts(value, 0)?;
                    let end = day_ts(value, 1)?;
                    tighten_after(&mut q.after, start);
                    tighten_before(&mut q.before, end);
                    q.on_date = Some(value.into());
                }
                "during" => {
                    let (start, end) = during_bounds(value)?;
                    tighten_after(&mut q.after, start);
                    tighten_before(&mut q.before, end);
                    q.during_date = Some(value.into());
                }
                "creator" => {
                    return Err(Error::Usage(
                        "creator: is not supported (Flek has no canvases or lists)".into(),
                    ));
                }
                _ => unreachable!("split_filter only returns known filter keys"),
            }
        } else {
            q.terms.push(token.clone());
            if let Some(neg) = token.strip_prefix('-').filter(|t| !t.is_empty()) {
                exclude.push(fts_term(neg));
            } else {
                include.push(word_term(&token).unwrap_or_else(|| fts_term(&token)));
            }
        }
    }
    if include.is_empty() && !exclude.is_empty() {
        return Err(Error::Usage(
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

fn parse_emoji_value(modifier: &str, value: &str) -> Result<String> {
    let hint = if modifier == "has" {
        "has:link, has:file, has:pin, has::emoji:"
    } else {
        "hasmy::emoji:"
    };
    let inner = value
        .strip_prefix(':')
        .and_then(|s| s.strip_suffix(':'))
        .ok_or_else(|| Error::Usage(format!("invalid {modifier}:{value} (use {hint})")))?;
    if inner.is_empty() {
        return Err(Error::Usage(format!(
            "empty emoji name in {modifier}:{value} (use {hint})"
        )));
    }
    Ok(inner.to_ascii_lowercase())
}

/// Decides whether a token is a Slack-style `key:value` filter or ordinary free
/// text that merely contains a colon.
///
/// Rule: a token is treated as a filter iff ALL of the following hold:
/// 1. It is not quoted (`"from:alice"` stays literal text).
/// 2. Splitting at the first `:` yields a recognized Slack filter key
///    (case-insensitive: `from`, `-from`, `in`, `-in`, `with`, `has`, `hasmy`,
///    `is`, `before`, `after`, `on`, `during`, `creator`) and a non-empty value.
/// 3. The value does not start with `//` (URLs such as `https://example.com/a`
///    or `slack://…` stay free text).
/// 4. Values starting with `:` (such as `std::vec`) stay free text except for
///    `has` and `hasmy`, where `has::tada:` / `hasmy::tada:` take an emoji code.
///
/// Every other `word:value` token (`foo:bar`, `re:meeting`, `TODO:fix`,
/// `Note:important`, `deadline:tomorrow`, `to:me`, `type:pdf`, `mailto:…`,
/// `10:30`, `:tada:`, `C:\Users`, `example.com:8080`, `note:`) stays plain
/// free text.
fn split_filter(token: &str) -> Option<(String, &str)> {
    if token.starts_with('"') {
        return None;
    }
    let (raw_key, value) = token.split_once(':')?;
    if value.is_empty() || value.starts_with("//") {
        return None;
    }
    let key = raw_key.to_ascii_lowercase();
    if !matches!(
        key.as_str(),
        "from"
            | "-from"
            | "in"
            | "-in"
            | "with"
            | "has"
            | "hasmy"
            | "is"
            | "before"
            | "after"
            | "on"
            | "during"
            | "creator"
    ) {
        return None;
    }
    if value.starts_with(':') && !matches!(key.as_str(), "has" | "hasmy") {
        return None;
    }
    Some((key, value))
}

fn tighten_before(slot: &mut Option<String>, ts: String) {
    *slot = Some(match slot.take() {
        Some(old) => old.min(ts),
        None => ts,
    });
}

fn tighten_after(slot: &mut Option<String>, ts: String) {
    *slot = Some(match slot.take() {
        Some(old) => old.max(ts),
        None => ts,
    });
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

fn date_to_ts(date: chrono::NaiveDate) -> Option<String> {
    let ts = date.and_hms_opt(0, 0, 0)?.and_utc().timestamp();
    if ts < 0 {
        return None;
    }
    Some(format!("{ts:010}.000000"))
}

/// `YYYY-MM-DD` (+ `plus_days`) → ts of that day's midnight, UTC.
fn day_ts(date: &str, plus_days: i64) -> Result<String> {
    let bad = || Error::Usage(format!("bad date {date:?}, expected YYYY-MM-DD"));
    let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .filter(|d| d.format("%Y-%m-%d").to_string() == date)
        .ok_or_else(bad)?;
    let date = date
        .checked_add_signed(chrono::Duration::days(plus_days))
        .ok_or_else(bad)?;
    date_to_ts(date).ok_or_else(bad)
}

/// `YYYY` or `YYYY-MM` → `(inclusive_start, exclusive_end)` UTC timestamps.
fn during_bounds(period: &str) -> Result<(String, String)> {
    let bad = || Error::Usage(format!("bad date {period:?}, expected YYYY or YYYY-MM"));
    let (start, end) = if period.len() == 4 && period.bytes().all(|b| b.is_ascii_digit()) {
        let year: i32 = period.parse().map_err(|_| bad())?;
        let start = chrono::NaiveDate::from_ymd_opt(year, 1, 1).ok_or_else(bad)?;
        let next_year = year.checked_add(1).ok_or_else(bad)?;
        let end = chrono::NaiveDate::from_ymd_opt(next_year, 1, 1).ok_or_else(bad)?;
        (start, end)
    } else if period.len() == 7
        && period.as_bytes()[4] == b'-'
        && period[..4].bytes().all(|b| b.is_ascii_digit())
        && period[5..].bytes().all(|b| b.is_ascii_digit())
    {
        let year: i32 = period[..4].parse().map_err(|_| bad())?;
        let month: u32 = period[5..].parse().map_err(|_| bad())?;
        let start = chrono::NaiveDate::from_ymd_opt(year, month, 1).ok_or_else(bad)?;
        let (next_year, next_month) = if month == 12 {
            (year.checked_add(1).ok_or_else(bad)?, 1)
        } else {
            (year, month + 1)
        };
        let end = chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1).ok_or_else(bad)?;
        (start, end)
    } else {
        return Err(bad());
    };
    let start_ts = date_to_ts(start).ok_or_else(bad)?;
    let end_ts = date_to_ts(end).ok_or_else(bad)?;
    Ok((start_ts, end_ts))
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
    let self_id = match self_modifier(q) {
        Some(modifier) => Some(resolve_self_user_id_for(conn, team_id, modifier)?),
        None => None,
    };
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
    // Newest-first free-text search: pick the page first and render snippets only
    // for it. Sorting by ts while snippet() is in the select list makes SQLite
    // probe FTS per row (~4x slower on frequent words, M6.6.3). Relevance order
    // and filter-only queries keep the single-query shape, which is faster there.
    let page_first = text_search && sort == Sort::Recent;
    // The page is sorted from the narrow covering index (0025) instead of full
    // rows, but only when every filter reads covered columns; otherwise each
    // hit would cost an index probe plus the row lookup it was meant to avoid.
    let covered = !q.has_link
        && !q.has_file
        && !q.has_pin
        && q.has_reactions.is_empty()
        && q.has_my_reactions.is_empty()
        && !q.is_thread
        && !q.is_saved
        && q.with.is_empty()
        && (q.from.is_empty() || q.resolved)
        && (q.exclude_from.is_empty() || q.resolved);
    let mut sql = if page_first {
        format!(
            "
            WITH matched_page AS MATERIALIZED (
            SELECT m.id
            FROM messages m{}
            JOIN conversations c ON c.id = m.conversation_id
        ",
            if covered {
                " INDEXED BY messages_search_cover"
            } else {
                ""
            }
        )
    } else {
        format!(
            "
            SELECT m.conversation_id, COALESCE(c.label, c.name, c.id), m.ts, m.thread_ts,
                COALESCE(u.display_name, u.name, m.author_name), {snippet}, m.hidden_by_limit, m.user_id
            FROM messages m
            JOIN conversations c ON c.id = m.conversation_id
            LEFT JOIN users u ON u.id = m.user_id
        "
        )
    };
    if text_search {
        sql.push_str(
            "
                JOIN messages_fts ON messages_fts.rowid = m.id
            ",
        );
    }
    sql.push_str(&format!(
        "
            WHERE c.team_id = {}
            AND m.deleted = 0
        ",
        arg(team_id.to_owned().into())
    ));
    let match_arg = text_search.then(|| arg(q.fts.clone().into()));
    if let Some(matched) = &match_arg {
        sql.push_str(&format!(" AND messages_fts MATCH {matched}"));
    }
    if !q.from.is_empty() || q.from_me {
        let mut ors = Vec::new();
        for name in &q.from {
            push_author_match(&mut ors, "m", team_id, name, q.resolved, &mut arg);
        }
        if q.from_me {
            let p = arg(self_id.as_deref().unwrap().to_owned().into());
            ors.push(format!("(m.user_id IS NOT NULL AND m.user_id = {p})"));
        }
        sql.push_str(&format!(" AND ({})", ors.join(" OR ")));
    }
    if !q.exclude_from.is_empty() || q.exclude_from_me {
        let mut ors = Vec::new();
        for name in &q.exclude_from {
            push_author_match(&mut ors, "m", team_id, name, q.resolved, &mut arg);
        }
        if q.exclude_from_me {
            let p = arg(self_id.as_deref().unwrap().to_owned().into());
            ors.push(format!("(m.user_id IS NOT NULL AND m.user_id = {p})"));
        }
        sql.push_str(&format!(" AND NOT COALESCE(({}), 0)", ors.join(" OR ")));
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
    if !q.exclude_in_conversations.is_empty() {
        let mut ors = Vec::new();
        for name in &q.exclude_in_conversations {
            let p = arg(name.to_lowercase().into());
            if q.resolved {
                ors.push(format!("lower(c.id) = {p}"));
                continue;
            }
            ors.push(format!(
                "COALESCE(lower(c.label) = {p}, 0) OR COALESCE(lower(c.name) = {p}, 0) OR lower(c.id) = {p}"
            ));
        }
        sql.push_str(&format!(" AND NOT COALESCE(({}), 0)", ors.join(" OR ")));
    }
    if !q.with.is_empty() {
        let mut dm_ors = Vec::new();
        let mut thread_ors = Vec::new();
        for name in &q.with {
            push_dm_user_match(&mut dm_ors, "c", team_id, name, q.resolved, &mut arg);
            push_author_match(&mut thread_ors, "tm", team_id, name, q.resolved, &mut arg);
        }
        sql.push_str(&format!(
            " AND ((c.kind = 'im' AND ({})) OR {})",
            dm_ors.join(" OR "),
            thread_participant_sql("m", &thread_ors.join(" OR ")),
        ));
    }
    if q.has_link {
        sql.push_str(" AND m.text LIKE '%http%'");
    }
    if q.has_file {
        sql.push_str(
            "
                AND (EXISTS (
                        SELECT 1
                        FROM message_files f
                        WHERE f.conversation_id = m.conversation_id
                        AND f.ts = m.ts)
                    OR m.text LIKE '%[file]%')
            ",
        );
    }
    if q.has_pin {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM pins pin WHERE pin.conversation_id = m.conversation_id AND pin.ts = m.ts)",
        );
    }
    for emoji in &q.has_reactions {
        sql.push_str(&format!(
            " AND {}",
            reaction_exists_sql("m", emoji, None, &mut arg)
        ));
    }
    for emoji in &q.has_my_reactions {
        sql.push_str(&format!(
            " AND {}",
            reaction_exists_sql("m", emoji, self_id.as_deref(), &mut arg)
        ));
    }
    if q.is_thread {
        sql.push_str(&format!(" AND {}", in_thread_sql("m")));
    }
    if q.is_saved {
        let p_me = arg(self_id.as_deref().unwrap().to_owned().into());
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM saved_messages sm \
              WHERE sm.conversation_id = m.conversation_id \
                AND sm.ts = m.ts \
                AND sm.user_id = {p_me})"
        ));
    }
    if let Some(b) = &q.before {
        sql.push_str(&format!(
            "
                AND m.ts < {}
            ",
            arg(b.clone().into())
        ));
    }
    if let Some(a) = &q.after {
        sql.push_str(&format!(
            "
                AND m.ts >= {}
            ",
            arg(a.clone().into())
        ));
    }
    sql.push_str(match (sort, text_search) {
        (Sort::Relevance, true) => {
            "
                ORDER BY bm25(messages_fts), m.ts DESC, m.conversation_id
            "
        }
        _ => {
            "
                ORDER BY m.ts DESC, m.conversation_id
            "
        }
    });
    sql.push_str(&format!(
        "
            LIMIT {}
        ",
        arg(i64::try_from(limit).unwrap_or(i64::MAX).into())
    ));

    sql.push_str(&format!(
        " OFFSET {}",
        arg(i64::try_from(offset).unwrap_or(i64::MAX).into())
    ));
    if let Some(matched) = match_arg.filter(|_| page_first) {
        // Scan MATCH once and probe the bounded materialized page; probing MATCH
        // per result would rebuild prefix posting-list unions for every row.
        sql.push_str(&format!(
            "
            )
            SELECT m.conversation_id, COALESCE(c.label, c.name, c.id), m.ts, m.thread_ts,
                COALESCE(u.display_name, u.name, m.author_name), {snippet}, m.hidden_by_limit, m.user_id
            FROM messages_fts CROSS JOIN matched_page p ON p.id = messages_fts.rowid
            JOIN messages m ON m.id = p.id
            JOIN conversations c ON c.id = m.conversation_id
            LEFT JOIN users u ON u.id = m.user_id
            WHERE messages_fts MATCH {matched}
            ORDER BY m.ts DESC, m.conversation_id
        "
        ));
    }
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

    #[test]
    fn parses_on_during_is_thread_and_from_me() {
        let q = parse("on:2024-03-05 is:THREAD from:ME").unwrap();
        assert_eq!(q.on_date.as_deref(), Some("2024-03-05"));
        assert_eq!(q.after.as_deref(), Some("1709596800.000000"));
        assert_eq!(q.before.as_deref(), Some("1709683200.000000"));
        assert!(q.is_thread);
        assert!(q.from_me);
        assert!(q.from.is_empty());

        let m = parse("during:2024-02").unwrap();
        assert_eq!(m.during_date.as_deref(), Some("2024-02"));
        assert_eq!(m.after.as_deref(), Some("1706745600.000000")); // 2024-02-01
        assert_eq!(m.before.as_deref(), Some("1709251200.000000")); // 2024-03-01 (leap year)

        let dec = parse("during:2024-12").unwrap();
        assert_eq!(dec.after.as_deref(), Some("1733011200.000000")); // 2024-12-01
        assert_eq!(dec.before.as_deref(), Some("1735689600.000000")); // 2025-01-01

        let y = parse("during:2024").unwrap();
        assert_eq!(y.during_date.as_deref(), Some("2024"));
        assert_eq!(y.after.as_deref(), Some("1704067200.000000")); // 2024-01-01
        assert_eq!(y.before.as_deref(), Some("1735689600.000000")); // 2025-01-01
    }

    #[test]
    fn date_filters_intersect_regardless_of_order() {
        // Wider before:/after: do not loosen on:.
        let a = parse("on:2024-03-05 after:2024-03-01 before:2024-03-10").unwrap();
        let b = parse("before:2024-03-10 after:2024-03-01 on:2024-03-05").unwrap();
        assert_eq!(a.after, b.after);
        assert_eq!(a.before, b.before);
        assert_eq!(a.after.as_deref(), Some("1709596800.000000")); // 2024-03-05
        assert_eq!(a.before.as_deref(), Some("1709683200.000000")); // 2024-03-06

        // Tighter after:/before: narrow during:.
        let c = parse("during:2024-03 after:2024-03-10 before:2024-03-20").unwrap();
        assert_eq!(c.after.as_deref(), Some("1710115200.000000")); // 2024-03-11
        assert_eq!(c.before.as_deref(), Some("1710892800.000000")); // 2024-03-20

        // Tighter before: narrows on: to an empty intersection (after == before).
        let d = parse("on:2024-03-05 before:2024-03-05").unwrap();
        assert_eq!(d.after.as_deref(), Some("1709596800.000000"));
        assert_eq!(d.before.as_deref(), Some("1709596800.000000"));
    }

    #[test]
    fn rejects_invalid_dates_and_filter_values() {
        for bad in [
            "on:2023-02-29",
            "on:2024-03-32",
            "on:2024-3-5",
            "on:2024-03",
            "during:2024-13",
            "during:2024-00",
            "during:2024-3",
            "during:2024-03-05",
            "during:1969",
            "during:1969-12",
            "during:24",
            "during:march",
        ] {
            let err = parse(bad).unwrap_err();
            assert_eq!(err.kind().exit_code(), 2, "expected usage error for {bad}");
        }

        for bad_is in ["is:dm", "is:pinned", "is:starred"] {
            let err = parse(bad_is).unwrap_err();
            assert_eq!(err.kind().exit_code(), 2, "{bad_is}: {err}");
            let msg = err.to_string();
            assert!(
                msg.contains("is:thread, is:saved"),
                "expected supported list in error for {bad_is}: {msg}"
            );
        }

        for bad_has in ["has:emoji", "has:my::tada:", "has::", "has::tada"] {
            let err = parse(bad_has).unwrap_err();
            assert_eq!(err.kind().exit_code(), 2, "{bad_has}: {err}");
            let msg = err.to_string();
            assert!(
                msg.contains("has:link, has:file, has:pin, has::emoji:"),
                "expected supported list in error for {bad_has}: {msg}"
            );
        }

        for bad_hasmy in ["hasmy:eyes", "hasmy::", "hasmy::tada"] {
            let err = parse(bad_hasmy).unwrap_err();
            assert_eq!(err.kind().exit_code(), 2, "{bad_hasmy}: {err}");
            assert!(
                err.to_string().contains("hasmy::emoji:"),
                "{bad_hasmy}: {err}"
            );
        }

        for bad_creator in ["creator:alice", "CREATOR:@bob"] {
            let err = parse(bad_creator).unwrap_err();
            assert_eq!(err.kind().exit_code(), 2, "{bad_creator}: {err}");
            assert_eq!(
                err.to_string(),
                "creator: is not supported (Flek has no canvases or lists)"
            );
        }

        for bad_with in ["with:me", "WITH:ME"] {
            let err = parse(bad_with).unwrap_err();
            assert_eq!(err.kind().exit_code(), 2, "{bad_with}: {err}");
            assert_eq!(err.to_string(), "with:me is not supported");
        }
    }

    #[test]
    fn ordinary_colons_remain_free_text() {
        for (input, expected_fts) in [
            ("https://example.com/a", r#""https://example.com/a""#),
            (
                "slack://channel?team=T1&id=C1",
                r#""slack://channel?team=T1&id=C1""#,
            ),
            ("mailto:alice@example.com", r#""mailto:alice@example.com""#),
            ("10:30", r#""10:30""#),
            ("12:34:56", r#""12:34:56""#),
            (r#""foo:bar""#, r#""foo:bar""#),
            (r#""with:alice""#, r#""with:alice""#),
            (r#""-from:bob""#, r#""-from:bob""#),
            ("foo:bar", r#""foo:bar""#),
            ("re:meeting", r#""re:meeting""#),
            ("TODO:fix", r#""TODO:fix""#),
            ("Note:important", r#""Note:important""#),
            ("deadline:tomorrow", r#""deadline:tomorrow""#),
            ("to:me", r#""to:me""#),
            ("type:pdf", r#""type:pdf""#),
            (":tada:", r#"":tada:""#),
            (":+1:", r#"":+1:""#),
            ("std::vec", r#""std::vec""#),
            ("note:", r#""note:""#),
            ("example.com:8080", r#""example.com:8080""#),
            (r"C:\Users\demo", r#""C:\Users\demo""#),
        ] {
            let q = parse(input).unwrap_or_else(|e| panic!("{input} should parse as text: {e}"));
            assert_eq!(q.fts, expected_fts, "{input}");
            assert_eq!(q.terms, [input], "{input}");
        }
    }

    #[test]
    fn parses_new_modifiers_and_filters_only_exclusions() {
        let q = parse(
            "-from:bob -FROM:me -in:#General -in:@dm with:@Alice has:PIN has::Tada: has::+1: hasmy::50%_off: is:SAVED",
        )
        .unwrap();
        assert!(q.fts.is_empty());
        assert_eq!(q.exclude_from, ["bob"]);
        assert!(q.exclude_from_me);
        assert_eq!(q.exclude_in_conversations, ["general", "dm"]);
        assert_eq!(q.with, ["alice"]);
        assert!(q.has_pin);
        assert_eq!(q.has_reactions, ["tada", "+1"]);
        assert_eq!(q.has_my_reactions, ["50%_off"]);
        assert!(q.is_saved);

        // Filters-only negated queries are valid; free-text exclusion alone is still rejected.
        assert!(parse("-from:bob").is_ok());
        assert!(parse("-in:#general").is_ok());
        assert!(parse("-from:bob -only").is_err());
    }

    #[test]
    fn sql_executes_on_during_is_thread_from_me_and_colon_text() {
        use crate::store::messages::{self, MessageRow};
        use crate::store::Store;

        let store = Store::open_in_memory().unwrap();
        store
            .with_conn(|c| {
                c.execute_batch(
                    "INSERT INTO teams (id, name, self_user_id) VALUES
                        ('TFAKE', 'Fake', 'U0SELF'),
                        ('TNOSELF', 'NoSelf', NULL),
                        ('TEMPTYSELF', 'EmptySelf', '');
                     INSERT INTO users (id, team_id, name, display_name) VALUES
                        ('U0SELF', 'TFAKE', 'selfuser', 'Self User'),
                        ('U0BOB', 'TFAKE', 'bob', 'Bob');
                     INSERT INTO conversations (id, team_id, kind, name, label) VALUES
                        ('C0FAKE', 'TFAKE', 'public_channel', 'general', 'general'),
                        ('C0NOSELF', 'TNOSELF', 'public_channel', 'general', 'general'),
                        ('C0EMPTY', 'TEMPTYSELF', 'public_channel', 'general', 'general');",
                )?;
                let row = |ts: &str, user: &str, text: &str| MessageRow {
                    ts: ts.into(),
                    user_id: Some(user.into()),
                    text: text.into(),
                    raw_json: "{}".into(),
                    ..Default::default()
                };
                let mut root_with_count = row("1709596800.000000", "U0SELF", "thread root count");
                root_with_count.thread_ts = Some("1709596800.000000".into());
                root_with_count.reply_count = 1;

                let mut reply = row("1709683199.999999", "U0BOB", "thread reply at 10:30 :tada:");
                reply.thread_ts = Some("1709596800.000000".into());

                // Imported-style root (reply_count=0, thread_ts=None) with a stored reply.
                let imported_root = row("1709251199.000000", "U0BOB", "imported root");
                let mut imported_reply = row("1709251200.000000", "U0SELF", "imported reply");
                imported_reply.thread_ts = Some("1709251199.000000".into());

                // Stale thread_ts == ts with reply_count == 0 and only a deleted reply.
                let mut empty_root = row("1709596799.999999", "U0BOB", "not a thread anymore");
                empty_root.thread_ts = Some("1709596799.999999".into());
                let mut deleted_reply = row("1709596799.999998", "U0SELF", "gone reply");
                deleted_reply.thread_ts = Some("1709596799.999999".into());
                deleted_reply.deleted = true;

                // Plain messages for date & colon text checks.
                let next_day = row(
                    "1709683200.000000",
                    "U0SELF",
                    "see https://example.com/a and foo:bar",
                );
                let prev_year = row("1704067199.999999", "U0BOB", "end of 2023");

                messages::upsert_messages(
                    c,
                    "C0FAKE",
                    &[
                        prev_year,
                        imported_root,
                        imported_reply,
                        empty_root,
                        deleted_reply,
                        root_with_count,
                        reply,
                        next_day,
                    ],
                )?;
                Ok(())
            })
            .unwrap();

        let hits = |team: &str, query: &str, sort: Sort| -> Result<Vec<String>> {
            let q = parse(query)?;
            store.with_conn(|c| {
                Ok(search(c, team, &q, sort, 50, ("[", "]"))?
                    .into_iter()
                    .map(|h| h.ts)
                    .collect())
            })
        };

        // on:2024-03-05 -> [1709596800.000000, 1709683200.000000)
        assert_eq!(
            hits("TFAKE", "on:2024-03-05", Sort::Recent).unwrap(),
            ["1709683199.999999", "1709596800.000000"]
        );
        // on: combined with disjoint before: yields empty set.
        assert!(
            hits("TFAKE", "on:2024-03-05 before:2024-03-05", Sort::Recent)
                .unwrap()
                .is_empty()
        );

        // during:2024-02 includes 2024-02-29 23:59:59, excludes 2024-03-01 00:00:00.
        assert_eq!(
            hits("TFAKE", "during:2024-02", Sort::Recent).unwrap(),
            ["1709251199.000000"]
        );
        // during:2023 matches 2023-12-31 23:59:59.999999 only.
        assert_eq!(
            hits("TFAKE", "during:2023", Sort::Recent).unwrap(),
            ["1704067199.999999"]
        );

        // is:thread includes roots with replies (counted or stored) and replies; excludes plain & empty roots.
        for sort in [Sort::Recent, Sort::Relevance] {
            assert_eq!(
                hits("TFAKE", "is:thread", sort).unwrap(),
                [
                    "1709683199.999999",
                    "1709596800.000000",
                    "1709251200.000000",
                    "1709251199.000000",
                ]
            );
            // Free text + is:thread + Sort::Recent exercises page_first without covering index.
            let mut found = hits("TFAKE", "thread is:thread", sort).unwrap();
            if sort == Sort::Recent {
                assert_eq!(found, ["1709683199.999999", "1709596800.000000"]);
            } else {
                found.sort();
                assert_eq!(found, ["1709596800.000000", "1709683199.999999"]);
            }
        }

        // from:me resolves to U0SELF (including with free text + Sort::Recent).
        assert_eq!(
            hits("TFAKE", "from:me", Sort::Recent).unwrap(),
            [
                "1709683200.000000",
                "1709596800.000000",
                "1709251200.000000",
            ]
        );
        assert_eq!(
            hits("TFAKE", "from:me reply", Sort::Recent).unwrap(),
            ["1709251200.000000"]
        );
        // from:me combined with from:bob uses OR semantics.
        assert_eq!(
            hits("TFAKE", "from:me from:bob on:2024-03-05", Sort::Recent).unwrap(),
            ["1709683199.999999", "1709596800.000000"]
        );

        // Unresolved from:me returns a clear NotFound error rather than matching nothing.
        for team in ["TNOSELF", "TEMPTYSELF", "TMISSING"] {
            let err = hits(team, "from:me", Sort::Recent).unwrap_err();
            assert!(
                matches!(err, Error::NotFound(_)) && err.to_string().contains("from:me"),
                "{team}: {err:?}"
            );
        }

        // URLs, times, quoted colons and :emoji: codes match stored text.
        assert_eq!(
            hits("TFAKE", "https://example.com/a", Sort::Recent).unwrap(),
            ["1709683200.000000"]
        );
        assert_eq!(
            hits("TFAKE", r#""foo:bar""#, Sort::Recent).unwrap(),
            ["1709683200.000000"]
        );
        assert_eq!(
            hits("TFAKE", "10:30", Sort::Recent).unwrap(),
            ["1709683199.999999"]
        );
        assert_eq!(
            hits("TFAKE", ":tada:", Sort::Recent).unwrap(),
            ["1709683199.999999"]
        );
        assert_eq!(
            hits("TFAKE", "foo:bar", Sort::Recent).unwrap(),
            ["1709683200.000000"]
        );
    }

    #[test]
    fn sql_executes_exclusions_with_pin_reactions_and_saved() {
        use crate::store::messages::{self, MessageRow};
        use crate::store::Store;

        let store = Store::open_in_memory().unwrap();
        store
            .with_conn(|c| {
                c.execute_batch(
                    "INSERT INTO teams (id, name, self_user_id) VALUES
                        ('TFAKE', 'Fake', 'U0SELF'),
                        ('TNOSELF', 'NoSelf', NULL);
                     INSERT INTO users (id, team_id, name, display_name) VALUES
                        ('U0SELF', 'TFAKE', 'selfuser', 'Self User'),
                        ('U0BOB', 'TFAKE', 'bob', 'Bob'),
                        ('U0CAROL', 'TFAKE', 'carol', 'Carol');
                     INSERT INTO conversations (id, team_id, kind, name, label, dm_user_id) VALUES
                        ('C0GEN', 'TFAKE', 'public_channel', 'general', 'general', NULL),
                        ('C0RND', 'TFAKE', 'public_channel', 'random', 'random', NULL),
                        ('D0BOB', 'TFAKE', 'im', NULL, 'Bob', 'U0BOB'),
                        ('D0CAROL', 'TFAKE', 'im', NULL, 'Carol', 'U0CAROL'),
                        ('C0NOSELF', 'TNOSELF', 'public_channel', 'general', 'general', NULL);",
                )?;
                let row = |ts: &str, user: Option<&str>, text: &str| MessageRow {
                    ts: ts.into(),
                    user_id: user.map(str::to_owned),
                    text: text.into(),
                    raw_json: "{}".into(),
                    ..Default::default()
                };

                // #general:
                // 100: plain msg by Bob (NOT in a thread) — must NOT match with:bob
                let mut bob_plain = row("1700000100.000000", Some("U0BOB"), "alpha plain bob");
                bob_plain.reactions.push(("tadaa".into(), "U0SELF".into()));
                bob_plain
                    .reactions
                    .push(("50XXoff::skin-tone-2".into(), "U0BOB".into()));

                // 200: thread root by Self, 201: reply by Bob -> matches with:bob
                let mut t1_root = row("1700000200.000000", Some("U0SELF"), "alpha thread root");
                t1_root.thread_ts = Some("1700000200.000000".into());
                t1_root.reply_count = 1;
                t1_root.reactions.push(("tada".into(), "U0BOB".into()));
                let mut t1_reply = row("1700000201.000000", Some("U0BOB"), "alpha bob reply");
                t1_reply.thread_ts = Some("1700000200.000000".into());
                t1_reply
                    .reactions
                    .push(("tada::skin-tone-3".into(), "U0SELF".into()));

                // 300: thread root by Self, 301: reply by Carol, 302: deleted reply by Bob
                let mut t2_root = row("1700000300.000000", Some("U0SELF"), "alpha carol thread");
                t2_root.thread_ts = Some("1700000300.000000".into());
                t2_root.reply_count = 1;
                let mut t2_reply = row("1700000301.000000", Some("U0CAROL"), "alpha carol reply");
                t2_reply.thread_ts = Some("1700000300.000000".into());
                t2_reply
                    .reactions
                    .push(("50%_off::skin-tone-2".into(), "U0SELF".into()));
                let mut t2_deleted_bob =
                    row("1700000302.000000", Some("U0BOB"), "alpha deleted bob");
                t2_deleted_bob.thread_ts = Some("1700000300.000000".into());
                t2_deleted_bob.deleted = true;

                // 400: system/bot message with NULL user_id and NULL author_name
                let anon = row("1700000400.000000", None, "alpha anon");

                messages::upsert_messages(
                    c,
                    "C0GEN",
                    &[
                        bob_plain,
                        t1_root,
                        t1_reply,
                        t2_root,
                        t2_reply,
                        t2_deleted_bob,
                        anon,
                    ],
                )?;

                // #random:
                let mut rnd_msg = row("1700000500.000000", Some("U0CAROL"), "alpha random");
                rnd_msg
                    .reactions
                    .push((r"a\b::skin-tone-4".into(), "U0BOB".into()));
                messages::upsert_messages(c, "C0RND", &[rnd_msg])?;

                // DM with Bob (name IS NULL) and DM with Carol (name IS NULL):
                messages::upsert_messages(
                    c,
                    "D0BOB",
                    &[row("1700000600.000000", Some("U0SELF"), "alpha dm bob")],
                )?;
                messages::upsert_messages(
                    c,
                    "D0CAROL",
                    &[row("1700000700.000000", Some("U0CAROL"), "alpha dm carol")],
                )?;

                // Pins & saved_messages:
                c.execute(
                    "INSERT INTO pins (conversation_id, ts, pinned_by, pinned_at)
                     VALUES ('C0GEN', '1700000200.000000', 'U0SELF', 1)",
                    [],
                )?;
                c.execute_batch(
                    "INSERT INTO saved_messages (conversation_id, ts, user_id) VALUES
                        ('C0GEN', '1700000200.000000', 'U0SELF'),
                        ('C0RND', '1700000500.000000', 'U0BOB');",
                )?;
                Ok(())
            })
            .unwrap();

        let hits = |team: &str, query: &str, sort: Sort| -> Result<Vec<String>> {
            let q = parse(query)?;
            store.with_conn(|c| {
                Ok(search(c, team, &q, sort, 50, ("[", "]"))?
                    .into_iter()
                    .map(|h| h.ts)
                    .collect())
            })
        };

        // B1: -from:bob keeps NULL-author rows and excludes Bob's rows.
        assert_eq!(
            hits("TFAKE", "-from:bob in:#general", Sort::Recent).unwrap(),
            [
                "1700000400.000000",
                "1700000301.000000",
                "1700000300.000000",
                "1700000200.000000",
            ]
        );
        // -from:me excludes U0SELF.
        assert_eq!(
            hits("TFAKE", "-from:me in:#general", Sort::Recent).unwrap(),
            [
                "1700000400.000000",
                "1700000301.000000",
                "1700000201.000000",
                "1700000100.000000",
            ]
        );
        // Combining from: and -from:
        assert_eq!(
            hits(
                "TFAKE",
                "from:bob from:carol -from:bob in:#general",
                Sort::Recent
            )
            .unwrap(),
            ["1700000301.000000"]
        );
        assert!(hits("TFAKE", "from:bob -from:bob", Sort::Recent)
            .unwrap()
            .is_empty());

        // -in:#general keeps DMs (even with conversations.name IS NULL) and #random.
        assert_eq!(
            hits("TFAKE", "-in:#general", Sort::Recent).unwrap(),
            [
                "1700000700.000000",
                "1700000600.000000",
                "1700000500.000000",
            ]
        );
        // Two -in: values exclude both conversations.
        assert_eq!(
            hits("TFAKE", "-in:#general -in:#random", Sort::Recent).unwrap(),
            ["1700000700.000000", "1700000600.000000"]
        );

        // B2: with:bob matches DM with bob and thread where bob replied,
        // NOT a thread without bob (or with only deleted bob reply),
        // NOT a plain channel message by bob outside threads.
        for sort in [Sort::Recent, Sort::Relevance] {
            let mut got = hits("TFAKE", "alpha with:bob", sort).unwrap();
            got.sort();
            assert_eq!(
                got,
                [
                    "1700000200.000000",
                    "1700000201.000000",
                    "1700000600.000000",
                ],
                "{sort:?}"
            );
        }
        // Multiple with: values use OR semantics.
        assert_eq!(
            hits("TFAKE", "with:bob with:carol", Sort::Recent).unwrap(),
            [
                "1700000700.000000",
                "1700000600.000000",
                "1700000301.000000",
                "1700000300.000000",
                "1700000201.000000",
                "1700000200.000000",
            ]
        );

        // B3: has:pin
        assert_eq!(
            hits("TFAKE", "has:pin", Sort::Recent).unwrap(),
            ["1700000200.000000"]
        );
        assert_eq!(
            hits("TFAKE", "alpha has:pin", Sort::Recent).unwrap(),
            ["1700000200.000000"]
        );

        // B4: has::emoji: matches exact and skin-tone variants, NOT prefix like tadaa.
        assert_eq!(
            hits("TFAKE", "has::tada:", Sort::Recent).unwrap(),
            ["1700000201.000000", "1700000200.000000"]
        );
        // LIKE special characters (%, _, \) are escaped in skin-tone match.
        assert_eq!(
            hits("TFAKE", "has::50%_off:", Sort::Recent).unwrap(),
            ["1700000301.000000"]
        );
        assert_eq!(
            hits("TFAKE", r"has::a\b:", Sort::Recent).unwrap(),
            ["1700000500.000000"]
        );

        // B5: hasmy::emoji: only matches current user's reactions (including skin-tone).
        assert_eq!(
            hits("TFAKE", "hasmy::tada:", Sort::Recent).unwrap(),
            ["1700000201.000000"]
        );

        // B6: is:saved only matches current user's saved rows.
        assert_eq!(
            hits("TFAKE", "is:saved", Sort::Recent).unwrap(),
            ["1700000200.000000"]
        );
        // is:saved does NOT leak into from:me filtering.
        store
            .with_conn(|c| {
                c.execute(
                    "INSERT INTO saved_messages (conversation_id, ts, user_id)
                     VALUES ('C0GEN', '1700000201.000000', 'U0SELF')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            hits("TFAKE", "is:saved", Sort::Recent).unwrap(),
            ["1700000201.000000", "1700000200.000000"]
        );

        // Unresolvable current user returns NotFound for -from:me, hasmy:, is:saved.
        for (query, label) in [
            ("-from:me", "-from:me"),
            ("hasmy::tada:", "hasmy:"),
            ("is:saved", "is:saved"),
        ] {
            let err = hits("TNOSELF", query, Sort::Recent).unwrap_err();
            assert!(
                matches!(err, Error::NotFound(_)) && err.to_string().contains(label),
                "{query}: {err:?}"
            );
        }
    }

    #[test]
    fn with_thread_participant_query_plan_uses_root_and_thread_indexes() {
        use crate::store::Store;

        let store = Store::open_in_memory().unwrap();
        store
            .with_conn(|c| {
                for resolved in [false, true] {
                    let mut args: Vec<SqlValue> = Vec::new();
                    let mut arg = |v: SqlValue| {
                        args.push(v);
                        format!("?{}", args.len())
                    };
                    let mut thread_ors = Vec::new();
                    push_author_match(&mut thread_ors, "tm", "TFAKE", "bob", resolved, &mut arg);
                    let pred = thread_participant_sql("m", &thread_ors.join(" OR "));
                    let sql =
                        format!("EXPLAIN QUERY PLAN SELECT m.id FROM messages m WHERE {pred}");
                    let mut stmt = c.prepare(&sql)?;
                    let plans: Vec<String> = stmt
                        .query_map(params_from_iter(args), |r| r.get::<_, String>(3))?
                        .collect::<std::result::Result<_, _>>()?;
                    let tm_plans: Vec<&str> = plans
                        .iter()
                        .map(String::as_str)
                        .filter(|line| line.contains(" tm "))
                        .collect();
                    assert_eq!(tm_plans.len(), 2, "resolved={resolved}: {plans:#?}");
                    assert!(
                        tm_plans
                            .iter()
                            .any(|line| line.contains("conversation_id=? AND ts=?")),
                        "root EXISTS must seek (conversation_id, ts): {tm_plans:#?}"
                    );
                    assert!(
                        tm_plans
                            .iter()
                            .any(|line| line.contains("conversation_id=? AND thread_ts=?")),
                        "replies EXISTS must seek (conversation_id, thread_ts): {tm_plans:#?}"
                    );
                }
                Ok(())
            })
            .unwrap();
    }
}
