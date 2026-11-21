//! Agent search: resolve locally, optionally ingest remote evidence, read SQLite.
use crate::{
    api, reads, references,
    store::{search as local, Store},
    views, Error, Result,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Default, Clone)]
pub struct Options {
    pub since: Option<String>,
    pub until: Option<String>,
    pub context: usize,
    pub offset: usize,
    pub cursor: Option<String>,
    pub limit: usize,
    pub recent: bool,
}
#[derive(Debug, Serialize)]
pub struct Filter {
    pub id: String,
    pub label: String,
}
#[derive(Debug, Serialize)]
pub struct Query {
    pub fts: String,
    pub terms: Vec<String>,
    pub from: Vec<Filter>,
    pub exclude_from: Vec<Filter>,
    pub in_conversations: Vec<Filter>,
    pub exclude_in: Vec<Filter>,
    pub with: Vec<Filter>,
    pub has_link: bool,
    pub has_file: bool,
    pub has_pin: bool,
    pub has_reactions: Vec<String>,
    pub has_my_reactions: Vec<String>,
    pub is_thread: bool,
    pub is_saved: bool,
    /// Query dates are preserved verbatim; normalized bounds are separate.
    pub before: Option<String>,
    pub after: Option<String>,
    pub on: Option<String>,
    pub during: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub since_ts: Option<String>,
    pub until_ts: Option<String>,
}
pub struct Prepared {
    parsed: local::Query,
    pub query: Query,
    pub remote_query: String,
    pub remote_unsupported_filters: Vec<String>,
    offset: usize,
    signature: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    signature: String,
    offset: usize,
}
fn encode(cursor: &Cursor) -> String {
    serde_json::to_vec(cursor)
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn decode(value: &str) -> Result<Cursor> {
    let bad = || Error::Usage("invalid search cursor".into());
    if value.len() > 32768 || !value.len().is_multiple_of(2) || !value.is_ascii() {
        return Err(bad());
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(|_| bad()))
        .collect::<Result<Vec<_>>>()?;
    let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|_| bad())?;
    if cursor.version != 1 {
        return Err(bad());
    }
    Ok(cursor)
}

pub fn prepare(c: &Connection, team: &str, input: &str, o: &Options) -> Result<Prepared> {
    let mut parsed = local::parse(input).map_err(|e| match e {
        Error::Usage(msg) => Error::Usage(msg),
        other => Error::Usage(other.to_string()),
    })?;
    let mut from = Vec::new();
    for (name, reference) in parsed.from.iter_mut().zip(&parsed.from_references) {
        let u = references::resolve_user(c, team, reference, references::Match::AllowPrefix)?;
        *name = u.id.clone();
        from.push(Filter {
            id: u.id,
            label: u.display_name.unwrap_or(u.name),
        });
    }
    if parsed.from_me {
        let me = local::resolve_self_user_id_for(c, team, "from:me")?;
        let label = self_user_label(c, team, &me)?;
        if !parsed.from.iter().any(|id| id == &me) {
            parsed.from.push(me.clone());
            from.push(Filter { id: me, label });
        }
        parsed.from_me = false;
    }
    let mut exclude_from = Vec::new();
    for (name, reference) in parsed
        .exclude_from
        .iter_mut()
        .zip(&parsed.exclude_from_references)
    {
        let u = references::resolve_user(c, team, reference, references::Match::AllowPrefix)?;
        *name = u.id.clone();
        exclude_from.push(Filter {
            id: u.id,
            label: u.display_name.unwrap_or(u.name),
        });
    }
    if parsed.exclude_from_me {
        let me = local::resolve_self_user_id_for(c, team, "-from:me")?;
        let label = self_user_label(c, team, &me)?;
        if !parsed.exclude_from.iter().any(|id| id == &me) {
            parsed.exclude_from.push(me.clone());
            exclude_from.push(Filter { id: me, label });
        }
        parsed.exclude_from_me = false;
    }
    let mut in_conversations = Vec::new();
    for (name, reference) in parsed
        .in_conversations
        .iter_mut()
        .zip(&parsed.conversation_references)
    {
        let conv = references::resolve(c, team, reference, references::Match::AllowPrefix)?
            .required(reference)?;
        *name = conv.id.clone();
        in_conversations.push(Filter {
            id: conv.id,
            label: conv.label,
        });
    }
    let mut exclude_in = Vec::new();
    for (name, reference) in parsed
        .exclude_in_conversations
        .iter_mut()
        .zip(&parsed.exclude_conversation_references)
    {
        let conv = references::resolve(c, team, reference, references::Match::AllowPrefix)?
            .required(reference)?;
        *name = conv.id.clone();
        exclude_in.push(Filter {
            id: conv.id,
            label: conv.label,
        });
    }
    let mut with = Vec::new();
    for (name, reference) in parsed.with.iter_mut().zip(&parsed.with_references) {
        let u = references::resolve_user(c, team, reference, references::Match::AllowPrefix)?;
        *name = u.id.clone();
        with.push(Filter {
            id: u.id,
            label: u.display_name.unwrap_or(u.name),
        });
    }
    if !parsed.has_my_reactions.is_empty() {
        local::resolve_self_user_id_for(c, team, "hasmy:")?;
    }
    if parsed.is_saved {
        local::resolve_self_user_id_for(c, team, "is:saved")?;
    }
    // Flags refine query bounds; neither can silently undo a query filter.
    if let Some(since) = &o.since {
        let ts = crate::time::parse(since)?;
        parsed.after = Some(parsed.after.map_or(ts.clone(), |old| old.max(ts)));
    }
    if let Some(until) = &o.until {
        let ts = crate::time::parse(until)?;
        parsed.before = Some(parsed.before.map_or(ts.clone(), |old| old.min(ts)));
    }
    if parsed
        .after
        .as_ref()
        .zip(parsed.before.as_ref())
        .is_some_and(|(a, b)| a > b)
    {
        return Err(Error::Usage(
            "search lower bound must not exceed upper bound".into(),
        ));
    }
    parsed.resolved = true;
    let query = Query {
        fts: parsed.fts.clone(),
        terms: parsed.terms.clone(),
        from,
        exclude_from,
        in_conversations,
        exclude_in,
        with,
        has_link: parsed.has_link,
        has_file: parsed.has_file,
        has_pin: parsed.has_pin,
        has_reactions: parsed.has_reactions.clone(),
        has_my_reactions: parsed.has_my_reactions.clone(),
        is_thread: parsed.is_thread,
        is_saved: parsed.is_saved,
        before: parsed.before_date.clone(),
        after: parsed.after_date.clone(),
        on: parsed.on_date.clone(),
        during: parsed.during_date.clone(),
        since: o.since.clone(),
        until: o.until.clone(),
        since_ts: parsed.after.clone(),
        until_ts: parsed.before.clone(),
    };
    let signature = serde_json::to_string(&(team, &query, o.recent))?;
    let offset = if let Some(value) = &o.cursor {
        if o.offset != 0 {
            return Err(Error::Usage("--offset conflicts with --cursor".into()));
        }
        let cursor = decode(value)?;
        if cursor.signature != signature {
            return Err(Error::Usage("cursor does not match search query".into()));
        }
        cursor.offset
    } else {
        o.offset
    };
    if offset > i64::MAX as usize || o.limit > i64::MAX as usize - 1 {
        return Err(Error::Usage("search offset/limit is too large".into()));
    }
    let (remote_query, remote_unsupported_filters) = translate(&parsed, o);
    Ok(Prepared {
        parsed,
        query,
        remote_query,
        remote_unsupported_filters,
        offset,
        signature,
    })
}

fn self_user_label(c: &Connection, team: &str, user_id: &str) -> Result<String> {
    Ok(c.query_row(
        "SELECT COALESCE(NULLIF(display_name, ''), name) FROM users WHERE id = ?1 AND team_id = ?2",
        params![user_id, team],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .unwrap_or_else(|| user_id.to_owned()))
}

/// Translate syntax, never forwarding local FTS operators to Slack. Prefixes
/// and automatic stemming have no exact Slack equivalent; omit those terms.
fn translate(q: &local::Query, o: &Options) -> (String, Vec<String>) {
    let mut terms = Vec::new();
    let mut unsupported = Vec::new();
    for term in &q.terms {
        if term.ends_with('*') && !term.starts_with('"') {
            unsupported.push(term.clone());
        } else {
            // Bare terms are retained, but Slack doesn't implement our stemmer.
            if !term.starts_with(['"', '-']) && term.chars().all(char::is_alphabetic) {
                unsupported.push(format!("local_stemming:{term}"));
            }
            // Colon-bearing literal terms (URLs, times, emoji codes) stay literal text remotely.
            if let Some(neg) = term.strip_prefix('-').filter(|t| !t.is_empty()) {
                if neg.contains(':') && !neg.starts_with('"') {
                    terms.push(format!("-\"{}\"", neg.replace('"', "")));
                } else {
                    terms.push(term.clone());
                }
            } else if term.contains(':') && !term.starts_with('"') {
                terms.push(format!("\"{}\"", term.replace('"', "")));
            } else {
                terms.push(term.clone());
            }
        }
    }
    // Repeated local from/in/with filters are OR; repeated Slack filters are not
    // assumed equivalent. Broaden the remote query and reapply locally.
    if q.from.len() > 1 {
        unsupported.push("from:multiple".into());
    } else if let Some(id) = q.from.first() {
        terms.push(format!("from:<@{id}>"));
    }
    for id in &q.exclude_from {
        terms.push(format!("-from:<@{id}>"));
    }
    if q.in_conversations.len() > 1 {
        unsupported.push("in:multiple".into());
    } else if let Some(id) = q.in_conversations.first() {
        terms.push(format!("in:<#{id}>"));
    }
    for id in &q.exclude_in_conversations {
        terms.push(format!("-in:<#{id}>"));
    }
    if q.with.len() > 1 {
        unsupported.push("with:multiple".into());
    } else if let Some(id) = q.with.first() {
        terms.push(format!("with:<@{id}>"));
    }
    if q.has_link {
        terms.push("has:link".into());
    }
    if q.has_file {
        terms.push("has:file".into());
    }
    if q.has_pin {
        terms.push("has:pin".into());
    }
    for name in &q.has_reactions {
        terms.push(format!("has::{name}:"));
    }
    for name in &q.has_my_reactions {
        terms.push(format!("hasmy::{name}:"));
    }
    if q.is_thread {
        terms.push("is:thread".into());
    }
    if q.is_saved {
        terms.push("is:saved".into());
    }
    if let Some(date) = &q.before_date {
        terms.push(format!("before:{date}"));
    }
    if let Some(date) = &q.after_date {
        terms.push(format!("after:{date}"));
    }
    if let Some(date) = &q.on_date {
        terms.push(format!("on:{date}"));
    }
    if let Some(period) = &q.during_date {
        terms.push(format!("during:{period}"));
    }
    // Slack date filters can't represent arbitrary microsecond or local-midnight
    // bounds exactly. Omit flag bounds remotely; SQLite enforces them exactly.
    if o.since.is_some() {
        unsupported.push("--since".into());
    }
    if o.until.is_some() {
        unsupported.push("--until".into());
    }
    (terms.join(" "), unsupported)
}

#[derive(Debug, Default, Serialize)]
pub struct Remote {
    pub searched: bool,
    pub total: usize,
    pub pages: usize,
    pub error: Option<RemoteError>,
}
#[derive(Debug, Serialize)]
pub struct RemoteError {
    pub code: String,
    pub message: String,
}
#[derive(Serialize)]
pub struct Hit {
    #[serde(flatten)]
    pub result: views::SearchResult,
    pub before: Vec<views::Message>,
    pub after: Vec<views::Message>,
    pub source: &'static str,
}
#[derive(Serialize)]
pub struct Page {
    pub query: Query,
    pub results: Vec<Hit>,
    pub next_cursor: Option<String>,
    pub remote: Remote,
    pub remote_unsupported_filters: Vec<String>,
}

/// `client=None` is strictly offline; no credential resolution occurs in core.
pub async fn run(
    store: &Store,
    team: &str,
    p: Prepared,
    o: &Options,
    client: Option<&api::Client>,
    markers: (&str, &str),
) -> Result<Page> {
    let mut remote = Remote::default();
    let mut remote_keys = HashSet::new();
    let mut existing = HashSet::new();
    if let Some(client) = client.filter(|_| o.limit > 0) {
        remote.searched = true;
        let target = p.offset.saturating_add(o.limit).saturating_add(5).min(300);
        // Fixed page size is required by numbered pagination. Distribute the
        // budget across pages so a second page doesn't over-fetch ~100 extras.
        let page_budget = target.div_ceil(100);
        let count = target.div_ceil(page_budget);
        for page in 1..=page_budget {
            let response = client
                .search_messages(
                    &p.remote_query,
                    page,
                    count,
                    if o.recent {
                        api::SearchSort::Timestamp
                    } else {
                        api::SearchSort::Score
                    },
                )
                .await;
            let response = match response {
                Ok(response) => response,
                Err(e) if e.is_auth_error() => return Err(e),
                Err(e) => {
                    // No raw server or transport text: remote errors cannot leak
                    // a query URL, response body, token, or cookie.
                    let code = match &e {
                        Error::Slack { code, .. } => code.clone(),
                        _ => serde_json::to_value(e.kind())?
                            .as_str()
                            .unwrap_or("internal")
                            .into(),
                    };
                    remote.error = Some(RemoteError {
                        message: format!("remote search failed ({code}); showing stored results"),
                        code,
                    });
                    break;
                }
            };
            remote.total = response.total;
            remote.pages += 1;
            let pages = response.pages;
            let empty = response.matches.is_empty();
            let team = team.to_owned();
            let keys = store
                .call(move |c| ingest(c, &team, response.matches))
                .await?;
            for (key, was_stored) in keys {
                if was_stored && !remote_keys.contains(&key) {
                    existing.insert(key.clone());
                }
                remote_keys.insert(key);
            }
            if empty || page >= pages || remote_keys.len() >= target {
                break;
            }
        }
    }
    let sort = if o.recent {
        local::Sort::Recent
    } else {
        local::Sort::Relevance
    };
    let team = team.to_owned();
    let options = o.clone();
    let parsed = p.parsed;
    let offset = p.offset;
    let markers = (markers.0.to_owned(), markers.1.to_owned());
    let (results, has_more) = store
        .read(move |c| {
            let mut hits = local::search_page(
                c,
                &team,
                &parsed,
                sort,
                options.limit.saturating_add(1),
                offset,
                (&markers.0, &markers.1),
            )?;
            let has_more = options.limit > 0 && hits.len() > options.limit;
            hits.truncate(options.limit);
            let results = views::search_results(c, &hits)?
                .into_iter()
                .map(|result| {
                    let m = &result.message;
                    let key = (m.conversation_id.clone(), m.ts.clone());
                    let (before, after) = if options.context == 0 {
                        (Vec::new(), Vec::new())
                    } else {
                        let context =
                            reads::message(c, &m.conversation_id, &m.ts, options.context)?;
                        (context.before, context.after)
                    };
                    let source = if remote_keys.contains(&key) {
                        if existing.contains(&key) {
                            "both"
                        } else {
                            "remote"
                        }
                    } else {
                        "local"
                    };
                    Ok(Hit {
                        result,
                        before,
                        after,
                        source,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((results, has_more))
        })
        .await?;
    let next_cursor = has_more.then(|| {
        encode(&Cursor {
            version: 1,
            signature: p.signature,
            offset: p.offset.saturating_add(o.limit),
        })
    });
    Ok(Page {
        query: p.query,
        results,
        next_cursor,
        remote,
        remote_unsupported_filters: p.remote_unsupported_filters,
    })
}

type Key = (String, String);
fn ingest(
    c: &mut Connection,
    team: &str,
    matches: Vec<api::SearchMatch>,
) -> Result<Vec<(Key, bool)>> {
    let mut keys = Vec::new();
    for hit in matches {
        let id = &hit.channel.id;
        // Search does not establish membership. Existing metadata is authoritative;
        // unknown conversations are discoverable but never auto-synced/listed.
        c.execute(
            "
                INSERT OR IGNORE INTO conversations(id, team_id, kind, name, label, is_member,
                        is_archived, raw_json)
                VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7)
            ",
            params![
                id,
                team,
                hit.channel.kind().as_str(),
                hit.channel.name,
                hit.channel.name.as_deref().unwrap_or(id),
                hit.channel.is_archived,
                hit.channel.raw.to_string()
            ],
        )?;
        let owner: String = c.query_row(
            "
                SELECT team_id
                FROM conversations
                WHERE id = ?1
            ",
            [id],
            |r| r.get(0),
        )?;
        if owner != team {
            continue;
        }
        if let Some(user) = hit.user {
            let known: bool = c.query_row(
                "
                    SELECT EXISTS (
                        SELECT 1
                        FROM users
                        WHERE id = ?1)
                ",
                [&user.id],
                |r| r.get(0),
            )?;
            if !known {
                crate::store::directory::upsert_users(c, team, &[user])?;
            }
        }
        let exists = c
            .query_row(
                "
                    SELECT 1
                    FROM messages
                    WHERE conversation_id = ?1
                    AND ts = ?2
                ",
                params![id, hit.message.ts],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        let row = crate::sync::backfill::to_row(&hit.message);
        // Search is incomplete evidence: use the existing gap-filling merge,
        // retaining complete stored text, metadata, files and reactions.
        crate::store::messages::import_messages(c, id, &[row])?;
        keys.push(((id.clone(), hit.message.ts), exists));
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn fixture() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .with_conn(|c| {
                c.execute_batch(
                    "INSERT INTO teams (id, name, self_user_id) VALUES
                        ('TFAKE', 'Fake', 'U0SELF'),
                        ('TNOUSER', 'NoUserRow', 'U0GHOST'),
                        ('TNOSELF', 'NoSelf', NULL);
                     INSERT INTO users (id, team_id, name, display_name) VALUES
                        ('U0SELF', 'TFAKE', 'selfuser', 'Self User'),
                        ('U0ALICE', 'TFAKE', 'alice', 'Alice'),
                        ('U0BOB', 'TFAKE', 'bob', 'Bob');
                     INSERT INTO conversations (id, team_id, kind, name, label) VALUES
                        ('C0FAKE', 'TFAKE', 'public_channel', 'general', 'general'),
                        ('C0RND', 'TFAKE', 'public_channel', 'random', 'random');",
                )?;
                Ok(())
            })
            .unwrap();
        store
    }

    #[test]
    fn prepare_resolves_from_me_and_round_trips_new_filters() {
        let db = fixture();
        let o = Options {
            limit: 10,
            ..Default::default()
        };
        let p = db
            .with_read(|c| {
                prepare(
                    c,
                    "TFAKE",
                    r#"from:me is:thread on:2024-03-05 during:2024-03 10:30 :tada: "foo:bar""#,
                    &o,
                )
            })
            .unwrap();
        assert_eq!(p.query.from.len(), 1);
        assert_eq!(p.query.from[0].id, "U0SELF");
        assert_eq!(p.query.from[0].label, "Self User");
        assert!(p.query.is_thread);
        assert_eq!(p.query.on.as_deref(), Some("2024-03-05"));
        assert_eq!(p.query.during.as_deref(), Some("2024-03"));
        assert_eq!(p.query.since_ts.as_deref(), Some("1709596800.000000"));
        assert_eq!(p.query.until_ts.as_deref(), Some("1709683200.000000"));
        assert_eq!(
            p.remote_query,
            r#""10:30" ":tada:" "foo:bar" from:<@U0SELF> is:thread on:2024-03-05 during:2024-03"#
        );

        // Falls back to user id when self_user_id has no row in users yet.
        let p = db
            .with_read(|c| prepare(c, "TNOUSER", "from:me during:2024", &o))
            .unwrap();
        assert_eq!(p.query.from[0].id, "U0GHOST");
        assert_eq!(p.query.from[0].label, "U0GHOST");
        assert_eq!(p.remote_query, "from:<@U0GHOST> during:2024");

        // Combining from:me with another from: marks from:multiple remotely.
        let p = db
            .with_read(|c| prepare(c, "TFAKE", "from:me from:alice", &o))
            .unwrap();
        assert_eq!(p.query.from.len(), 2);
        assert!(p
            .remote_unsupported_filters
            .contains(&"from:multiple".into()));
    }

    #[test]
    fn prepare_reports_unresolved_self_and_invalid_filters() {
        let db = fixture();
        let o = Options::default();
        for (query, needle) in [
            ("from:me", "from:me"),
            ("-from:me", "-from:me"),
            ("hasmy::tada:", "hasmy:"),
            ("is:saved", "is:saved"),
            ("-from:nobody", "nobody"),
            ("-in:missing", "missing"),
            ("with:nobody", "nobody"),
        ] {
            let team = if query.contains("nobody") || query.contains("missing") {
                "TFAKE"
            } else {
                "TNOSELF"
            };
            let err = db
                .with_read(|c| prepare(c, team, query, &o))
                .err()
                .unwrap_or_else(|| panic!("expected NotFound for {query}"));
            assert_eq!(err.kind().exit_code(), 3, "{query}: {err}");
            assert!(err.to_string().contains(needle), "{query}: {err}");
        }

        for bad in [
            "creator:alice",
            "with:me",
            "is:dm",
            "has:my::tada:",
            "hasmy:tada",
            "during:2024-13",
            "on:2023-02-29",
            "on:2024-03-05 after:2024-03-06",
        ] {
            let err = db
                .with_read(|c| prepare(c, "TFAKE", bad, &o))
                .err()
                .unwrap_or_else(|| panic!("expected error for {bad}"));
            assert_eq!(err.kind().exit_code(), 2, "{bad}: {err}");
        }
    }

    #[test]
    fn prepare_resolves_and_translates_remaining_slack_modifiers() {
        let db = fixture();
        let o = Options {
            limit: 10,
            ..Default::default()
        };
        let p = db
            .with_read(|c| {
                prepare(
                    c,
                    "TFAKE",
                    "-from:bob -from:me -in:#general -in:#random with:alice has:pin has::tada: hasmy::+1: is:saved foo:bar",
                    &o,
                )
            })
            .unwrap();
        assert_eq!(
            p.query
                .exclude_from
                .iter()
                .map(|f| (f.id.as_str(), f.label.as_str()))
                .collect::<Vec<_>>(),
            [("U0BOB", "Bob"), ("U0SELF", "Self User")]
        );
        assert_eq!(
            p.query
                .exclude_in
                .iter()
                .map(|f| (f.id.as_str(), f.label.as_str()))
                .collect::<Vec<_>>(),
            [("C0FAKE", "general"), ("C0RND", "random")]
        );
        assert_eq!(p.query.with.len(), 1);
        assert_eq!(p.query.with[0].id, "U0ALICE");
        assert!(p.query.has_pin);
        assert_eq!(p.query.has_reactions, ["tada"]);
        assert_eq!(p.query.has_my_reactions, ["+1"]);
        assert!(p.query.is_saved);
        assert_eq!(
            p.remote_query,
            r#""foo:bar" -from:<@U0BOB> -from:<@U0SELF> -in:<#C0FAKE> -in:<#C0RND> with:<@U0ALICE> has:pin has::tada: hasmy::+1: is:saved"#
        );
        assert!(p.remote_unsupported_filters.is_empty());

        // Multiple with: values are OR locally → omitted remotely as with:multiple.
        let multi = db
            .with_read(|c| prepare(c, "TFAKE", "with:alice with:bob", &o))
            .unwrap();
        assert_eq!(multi.query.with.len(), 2);
        assert!(multi.remote_query.is_empty());
        assert_eq!(multi.remote_unsupported_filters, ["with:multiple"]);
    }
}
