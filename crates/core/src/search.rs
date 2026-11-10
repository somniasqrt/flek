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
    pub in_conversations: Vec<Filter>,
    pub has_link: bool,
    pub has_file: bool,
    /// Query dates are preserved verbatim; normalized bounds are separate.
    pub before: Option<String>,
    pub after: Option<String>,
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
    let mut parsed = local::parse(input).map_err(|e| Error::Usage(e.to_string()))?;
    let mut from = Vec::new();
    for (name, reference) in parsed.from.iter_mut().zip(&parsed.from_references) {
        let u = references::resolve_user(c, team, reference, references::Match::AllowPrefix)?;
        *name = u.id.clone();
        from.push(Filter {
            id: u.id,
            label: u.display_name.unwrap_or(u.name),
        });
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
        in_conversations,
        has_link: parsed.has_link,
        has_file: parsed.has_file,
        before: parsed.before_date.clone(),
        after: parsed.after_date.clone(),
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
            // Unknown colon expressions are literal text locally, not Slack filters.
            if term.contains(':') && !term.starts_with('"') {
                terms.push(format!("\"{}\"", term.replace('"', "")));
            } else {
                terms.push(term.clone());
            }
        }
    }
    // Repeated local from/in filters are OR; repeated Slack filters are not
    // assumed equivalent. Broaden the remote query and reapply locally.
    for (key, values) in [("from", &q.from), ("in", &q.in_conversations)] {
        if values.len() > 1 {
            unsupported.push(format!("{key}:multiple"));
        } else if let Some(id) = values.first() {
            terms.push(if key == "from" {
                format!("from:<@{id}>")
            } else {
                format!("in:<#{id}>")
            });
        }
    }
    if q.has_link {
        terms.push("has:link".into());
    }
    if q.has_file {
        terms.push("has:file".into());
    }
    if let Some(date) = &q.before_date {
        terms.push(format!("before:{date}"));
    }
    if let Some(date) = &q.after_date {
        terms.push(format!("after:{date}"));
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
            "INSERT OR IGNORE INTO conversations
             (id, team_id, kind, name, label, is_member, is_archived, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7)",
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
        let owner: String =
            c.query_row("SELECT team_id FROM conversations WHERE id=?1", [id], |r| {
                r.get(0)
            })?;
        if owner != team {
            continue;
        }
        if let Some(user) = hit.user {
            let known: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE id=?1)",
                [&user.id],
                |r| r.get(0),
            )?;
            if !known {
                crate::store::directory::upsert_users(c, team, &[user])?;
            }
        }
        let exists = c
            .query_row(
                "SELECT 1 FROM messages WHERE conversation_id=?1 AND ts=?2",
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
