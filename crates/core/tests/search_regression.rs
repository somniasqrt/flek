//! M6.6.3 regression: the page-first `search_page` rewrite must return exactly
//! what the original single-query implementation returned (rows, order, snippets,
//! pagination), on a synthetic DB with ties, deletions, other teams and filters.

use flek_core::store::search::{self, Hit, Query, Sort};
use flek_core::store::Store;
use flek_core::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection};

#[allow(dead_code)]
#[path = "../benches/support/mod.rs"]
mod budget_fixture;

/// Verbatim copy of `search_page` before the page-first snippet rewrite (M6.6.3).
#[allow(clippy::too_many_arguments)]
fn legacy_search_page(
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
        "
            SELECT m.conversation_id, COALESCE(c.label, c.name, c.id), m.ts, m.thread_ts,
                COALESCE(u.display_name, u.name, m.author_name), {snippet}, m.hidden_by_limit, m.user_id
            FROM messages m
            JOIN conversations c ON c.id = m.conversation_id
            LEFT JOIN users u ON u.id = m.user_id
        "
    );
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
                "
                    m.user_id IN (
                        SELECT id
                        FROM users
                        WHERE lower(name) = {p}
                        OR lower(display_name) = {p}
                        OR lower(real_name) = {p}
                        OR lower(id) = {p})
                    OR lower(m.author_name) = {p}
                "
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

const WORDS: [&str; 8] = [
    "release",
    "grades",
    "звіт",
    "таблиця",
    "deploy",
    "meeting",
    "report",
    "лекція",
];

/// Deterministic DB: 3 conversations (+1 in another team), many equal `ts` values
/// across conversations (tie-break coverage), deleted rows, links, files.
fn synthetic() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team'), ('T2', 'Other');
                 INSERT INTO users (id, team_id, name, display_name) VALUES
                    ('U1', 'T1', 'alice', 'Alice'), ('U2', 'T1', 'bob', 'Bob');
                 INSERT INTO conversations (id, team_id, kind, name, label) VALUES
                    ('C1', 'T1', 'public_channel', 'general', 'general'),
                    ('C2', 'T1', 'public_channel', 'random', 'random'),
                    ('C3', 'T1', 'public_channel', 'dev', 'dev'),
                    ('C9', 'T2', 'public_channel', 'general', 'general');",
            )?;
            let tx = c.transaction()?;
            let mut seed: u64 = 7;
            for i in 0..2400u64 {
                let conv = ["C1", "C2", "C3", "C9"][(i % 4) as usize];
                // Pairs of conversations share timestamps.
                let ts = format!("{}.000000", 1_700_000_000 + (i / 2) * 60);
                let mut text = String::new();
                for _ in 0..(3 + i % 9) {
                    seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                    text.push_str(WORDS[(seed >> 33) as usize % WORDS.len()]);
                    text.push(' ');
                }
                if i % 7 == 0 {
                    text.push_str("<https://example.invalid/x> ");
                }
                if i % 50 == 0 {
                    text.push_str("needle ");
                }
                let user = if i % 3 == 0 { "U1" } else { "U2" };
                tx.execute(
                    "INSERT INTO messages (conversation_id, ts, user_id, text, deleted, hidden_by_limit, raw_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, '{}')",
                    params![conv, ts, user, text, i % 31 == 0, i % 41 == 0],
                )?;
                if i % 40 == 0 {
                    tx.execute(
                        "INSERT INTO files (id, name, mimetype, size) VALUES (?1, 'f.txt', 'text/plain', 1)",
                        [format!("F{i}")],
                    )?;
                    tx.execute(
                        "INSERT INTO message_files (conversation_id, ts, file_id) VALUES (?1, ?2, ?3)",
                        params![conv, ts, format!("F{i}")],
                    )?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .unwrap();
    store
}

fn key(h: &[Hit]) -> String {
    serde_json::to_string(h).unwrap()
}

#[test]
fn page_first_search_matches_the_original_query() {
    let store = synthetic();
    let queries = [
        "release",
        "needle",
        "grades report",
        "звіт лекція",
        "\"release grades\"",
        "таблиц*",
        "release -deploy",
        "release from:alice",
        "release in:#dev",
        "from:bob in:#general deploy has:link",
        "release has:file",
        "release before:2023-11-15 after:2023-11-14",
        "from:alice",
        "in:#random has:link",
        "has:file",
        "before:2023-11-15",
        "nomatchterm",
    ];
    let mut compared = 0;
    for text in queries {
        let q = search::parse(text).unwrap();
        for sort in [Sort::Relevance, Sort::Recent] {
            for (limit, offset) in [(50, 0), (7, 0), (7, 7), (7, 21), (25, 100), (50, 5000)] {
                let (new, old) = store
                    .with_conn(|c| {
                        Ok((
                            search::search_page(c, "T1", &q, sort, limit, offset, ("[", "]"))?,
                            legacy_search_page(c, "T1", &q, sort, limit, offset, ("[", "]"))?,
                        ))
                    })
                    .unwrap();
                assert_eq!(
                    key(&new),
                    key(&old),
                    "query {text:?} sort {sort:?} limit {limit} offset {offset}"
                );
                compared += new.len();
            }
        }
    }
    assert!(compared > 1000, "fixture produced too few hits: {compared}");
}

#[test]
fn paging_covers_every_hit_once_in_the_same_order() {
    let store = synthetic();
    let q = search::parse("release").unwrap();
    for sort in [Sort::Relevance, Sort::Recent] {
        let all = store
            .with_conn(|c| search::search_page(c, "T1", &q, sort, 100_000, 0, ("[", "]")))
            .unwrap();
        assert!(all.len() > 100);
        let mut paged = Vec::new();
        let mut offset = 0;
        loop {
            let page = store
                .with_conn(|c| search::search_page(c, "T1", &q, sort, 37, offset, ("[", "]")))
                .unwrap();
            if page.is_empty() {
                break;
            }
            offset += page.len();
            paged.extend(page);
        }
        assert_eq!(key(&paged), key(&all), "sort {sort:?}");
        // Other-team rows never appear.
        assert!(all.iter().all(|h| h.conversation_id != "C9"));
    }
}

/// Before/after timing on the 100k-message budget fixture:
/// `cargo test --release -p flek-core --test search_regression -- --ignored --nocapture`.
#[test]
#[ignore]
fn timing_original_vs_page_first_100k() {
    use std::time::Instant;
    let fixture = budget_fixture::Fixture::new().unwrap();
    let store = fixture.open().unwrap();
    let team = budget_fixture::TEAM;
    let p50 = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    for text in ["needle", "grades", "звіт дедлайн", "таблиц*", "has:link"] {
        let q = search::parse(text).unwrap();
        for sort in [Sort::Relevance, Sort::Recent] {
            let (mut old, mut new) = (Vec::new(), Vec::new());
            for _ in 0..10 {
                let t = Instant::now();
                let a = store
                    .with_read(|c| legacy_search_page(c, team, &q, sort, 50, 0, ("[", "]")))
                    .unwrap();
                old.push(t.elapsed().as_secs_f64() * 1000.0);
                let t = Instant::now();
                let b = store
                    .with_read(|c| search::search_page(c, team, &q, sort, 50, 0, ("[", "]")))
                    .unwrap();
                new.push(t.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(key(&a), key(&b), "{text:?} {sort:?}");
            }
            println!(
                "{text:<14} {sort:?}: original p50 {:.1} ms, page-first p50 {:.1} ms",
                p50(old),
                p50(new)
            );
        }
    }
}
