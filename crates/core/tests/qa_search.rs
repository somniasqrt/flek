//! QA: FTS5 search with Unicode text, edits/deletes and hostile queries.

mod qa_common;

use qa_common::*;
use serde_json::json;

use flek_core::store::search::{self, Sort};
use flek_core::store::Store;

fn find(store: &Store, q: &str) -> Vec<String> {
    let q = search::parse(q).unwrap();
    store
        .with_conn(|c| search::search(c, "T1", &q, Sort::Recent, 50, ("[", "]")))
        .unwrap()
        .into_iter()
        .map(|h| h.ts)
        .collect()
}

fn seeded() -> Store {
    let store = live_store();
    upsert(
        &store,
        &[
            row("2.000000", "Привіт, Світе! Їжак під ґанком"),
            row("3.000000", "Зустріч о 10:00 в кімнаті \"Олімп\""),
            row("4.000000", "apple banana"),
            row("5.000000", "apple cherry"),
            row("6.000000", "use C++ and foo:bar at mail@example.com"),
        ],
    );
    store
}

#[test]
fn cyrillic_is_case_insensitive() {
    let store = seeded();
    assert_eq!(find(&store, "привіт"), vec!["2.000000"]);
    assert_eq!(find(&store, "СВІТЕ"), vec!["2.000000"]);
    assert_eq!(find(&store, "їжак"), vec!["2.000000"]);
    assert_eq!(find(&store, "Ґанком"), vec!["2.000000"]);
    assert_eq!(find(&store, "олімп"), vec!["3.000000"]);
}

#[test]
fn cyrillic_prefix_search() {
    let store = seeded();
    assert_eq!(find(&store, "зустр*"), vec!["3.000000"]);
}

#[test]
fn exclusion_and_phrase() {
    let store = seeded();
    assert_eq!(find(&store, "apple -banana"), vec!["5.000000"]);
    assert_eq!(find(&store, "\"apple cherry\""), vec!["5.000000"]);
    assert!(find(&store, "\"cherry apple\"").is_empty());
}

/// User-facing refusals (`Error::Store`, e.g. "a query needs at least one
/// word") are fine; a raw SQLite/FTS5 syntax error is not.
#[test]
fn hostile_queries_never_hit_an_fts_syntax_error() {
    let store = seeded();
    let queries = [
        "\"unbalanced",
        "\"",
        "\"\"",
        "*",
        "* *",
        "-",
        "--",
        "-*",
        "foo:bar",
        ":",
        "text:apple",
        "rowid:1",
        "messages_fts:apple",
        "AND",
        "OR",
        "NOT",
        "apple AND",
        "NOT apple",
        "(",
        ")",
        "(apple",
        "NEAR(apple banana)",
        "^apple",
        "'",
        "C++",
        "mail@example.com",
        "http://example.com/a?b=c&d=e",
        "10:00",
        "💥",
        "\\",
        "{text}:apple",
        "apple*cherry",
        "from:",
        "in:",
        "in:#",
        "from:@nobody",
        "has:",
        "has:nothing",
    ];
    let mut failures = Vec::new();
    for q in queries {
        let r = search::parse(q).and_then(|parsed| {
            store.with_conn(|c| search::search(c, "T1", &parsed, Sort::Relevance, 20, ("[", "]")))
        });
        if let Err(e @ flek_core::Error::Sqlite(_)) = r {
            failures.push(format!("{q:?}: {e}"));
        }
    }
    assert!(
        failures.is_empty(),
        "queries failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn punctuation_terms_find_their_message() {
    let store = seeded();
    assert_eq!(find(&store, "10:00"), vec!["3.000000"]);
    assert_eq!(find(&store, "mail@example.com"), vec!["6.000000"]);
}

#[tokio::test]
async fn live_edit_and_delete_leave_no_stale_hits() {
    let store = seeded();
    ev(
        &store,
        json!({"type": "message", "subtype": "message_changed", "channel": "C1", "ts": "20.000000",
               "message": {"ts": "4.000000", "user": "U1", "text": "pear plum", "edited": {"ts": "20.000000"}}}),
    )
    .await;
    assert!(find(&store, "banana").is_empty());
    assert_eq!(find(&store, "plum"), vec!["4.000000"]);
    ev(
        &store,
        json!({"type": "message", "subtype": "message_deleted", "channel": "C1", "ts": "21.000000",
               "deleted_ts": "5.000000"}),
    )
    .await;
    assert!(
        find(&store, "cherry").is_empty(),
        "deleted message is still found"
    );
}

#[test]
fn search_is_scoped_to_the_team() {
    let store = seeded();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T2', 'Other');
                 INSERT INTO conversations (id, team_id, kind, name, label) VALUES ('C2', 'T2', 'public_channel', 'general', 'general');",
            )?;
            flek_core::store::messages::upsert_messages(c, "C2", &[row("7.000000", "apple from other team")])
        })
        .unwrap();
    let hits = find(&store, "apple");
    assert!(!hits.contains(&"7.000000".to_string()));
    assert_eq!(hits.len(), 2);
}

// FINDING: a NUL character in the query reaches FTS5 unescaped and fails
// with a raw "SQLite: unterminated string" instead of being searched/refused.
#[test]
fn nul_in_query_is_not_an_sql_error() {
    let store = seeded();
    let r = search::parse("apple\u{0}cherry").and_then(|parsed| {
        store.with_conn(|c| search::search(c, "T1", &parsed, Sort::Relevance, 20, ("[", "]")))
    });
    assert!(
        !matches!(r, Err(flek_core::Error::Sqlite(_))),
        "{:?}",
        r.err()
    );
}
