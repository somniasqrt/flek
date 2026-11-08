//! Full-text search.

use std::time::Instant;

use flek_core::store::messages::{self, MessageRow};
use flek_core::store::search::{self, Sort};
use flek_core::store::{migrations, Store};
use rusqlite::params;

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team'), ('T2', 'Other');
                 INSERT INTO users (id, team_id, name, display_name) VALUES ('U1', 'T1', 'alice', 'Alice'), ('U2', 'T1', 'bob', 'Bob');
                 INSERT INTO conversations (id, team_id, kind, name, label) VALUES
                    ('C1', 'T1', 'public_channel', 'general', 'general'),
                    ('C2', 'T1', 'public_channel', 'random', 'random'),
                    ('C9', 'T2', 'public_channel', 'general', 'general');",
            )?;
            Ok(())
        })
        .unwrap();
    store
}

fn msg(ts: &str, user: &str, text: &str) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        user_id: Some(user.into()),
        text: text.into(),
        raw_json: "{}".into(),
        ..Default::default()
    }
}

fn find(store: &Store, q: &str) -> Vec<String> {
    let q = search::parse(q).unwrap();
    store
        .with_conn(|c| search::search(c, "T1", &q, Sort::Recent, 50, ("[", "]")))
        .unwrap()
        .into_iter()
        .map(|h| format!("{}:{}", h.conversation, h.ts))
        .collect()
}

fn seed(store: &Store) {
    store
        .with_conn(|c| {
            // 2024-01-10, 2024-02-10, 2024-03-10 (UTC)
            messages::upsert_messages(
                c,
                "C1",
                &[
                    msg(
                        "1704844800.000001",
                        "U1",
                        "Таблиця оцінок тут <https://docs.example/sheet>",
                    ),
                    msg(
                        "1707523200.000001",
                        "U2",
                        "оцінки оновлено, дивіться таблицю",
                    ),
                    msg("1710028800.000001", "U1", "Grades sheet draft"),
                ],
            )?;
            messages::upsert_messages(
                c,
                "C2",
                &[msg(
                    "1707523200.000002",
                    "U2",
                    "lunch grades? no, lunch plans",
                )],
            )?;
            messages::upsert_messages(
                c,
                "C9",
                &[msg("1707523200.000003", "U9", "grades in another team")],
            )?;
            c.execute(
                "INSERT INTO files (id, name) VALUES ('F1', 'sheet.xlsx');
                 ",
                [],
            )?;
            c.execute(
                "INSERT INTO message_files VALUES ('C1', '1710028800.000001', 'F1')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn finds_words_with_filters() {
    let store = store();
    seed(&store);
    assert_eq!(
        find(&store, "grades"),
        ["general:1710028800.000001", "random:1707523200.000002"]
    );
    assert_eq!(
        find(&store, "grades in:#general"),
        ["general:1710028800.000001"]
    );
    assert_eq!(
        find(&store, "from:bob grades"),
        ["random:1707523200.000002"]
    );
    assert_eq!(find(&store, "таблиця"), ["general:1704844800.000001"]);
    assert_eq!(
        find(&store, "таблиц*"),
        ["general:1707523200.000001", "general:1704844800.000001"]
    );
    assert_eq!(
        find(&store, "оцін* has:link"),
        ["general:1704844800.000001"]
    );
    assert_eq!(find(&store, "has:file"), ["general:1710028800.000001"]);
    assert_eq!(
        find(&store, "after:2024-01-31 before:2024-03-01"),
        ["random:1707523200.000002", "general:1707523200.000001"]
    );
    assert_eq!(find(&store, "lunch -grades").len(), 0);
    assert_eq!(
        find(&store, r#""lunch plans""#),
        ["random:1707523200.000002"]
    );
}

#[test]
fn snippet_marks_matches() {
    let store = store();
    seed(&store);
    let q = search::parse("draft").unwrap();
    let hits = store
        .with_conn(|c| search::search(c, "T1", &q, Sort::Relevance, 5, ("<b>", "</b>")))
        .unwrap();
    assert_eq!(hits[0].snippet, "Grades sheet <b>draft</b>");
    assert_eq!(hits[0].author.as_deref(), Some("Alice"));
}

#[test]
fn index_follows_edits_and_deletions() {
    let store = store();
    seed(&store);
    store
        .with_conn(|c| {
            messages::upsert_messages(
                c,
                "C1",
                &[msg("1710028800.000001", "U1", "Final spreadsheet")],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(find(&store, "draft").len(), 0);
    assert_eq!(find(&store, "spreadsheet"), ["general:1710028800.000001"]);
    store
        .with_conn(|c| messages::mark_deleted(c, "C1", &["1710028800.000001".into()]))
        .unwrap();
    assert_eq!(find(&store, "spreadsheet").len(), 0);
}

#[test]
fn upgrade_from_v4_keeps_reactions_and_indexes_old_messages() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flek.db");
    {
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        migrations::migrate_to(&mut conn, 4).unwrap();
        conn.execute_batch(
            "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
             INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');
             INSERT INTO messages (conversation_id, ts, text) VALUES ('C1', '1.000000', 'archived wisdom');
             INSERT INTO reactions VALUES ('C1', '1.000000', 'brain', 'U1');",
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.schema_version().unwrap(),
        migrations::latest_version()
    );
    store
        .with_conn(|c| {
            assert_eq!(messages::reactions(c, "C1", "1.000000")?.len(), 1);
            let q = search::parse("wisdom").unwrap();
            assert_eq!(
                search::search(c, "T1", &q, Sort::Relevance, 5, ("", ""))?.len(),
                1
            );
            // Foreign keys are back on.
            let fk: bool = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
            assert!(fk);
            Ok(())
        })
        .unwrap();
}

/// M1.9.4 budget: < 50 ms on 100k messages. Run with
/// `cargo test --release -p flek-core --test search -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_100k_messages() {
    let store = store();
    let words = [
        "оцінки",
        "таблиця",
        "звіт",
        "дедлайн",
        "grades",
        "sheet",
        "report",
        "deadline",
        "meeting",
        "lunch",
        "release",
        "bug",
        "fix",
        "deploy",
        "review",
        "design",
        "проєкт",
        "зустріч",
        "питання",
        "відповідь",
    ];
    store
        .with_conn(|c| {
            let tx = c.transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO messages (conversation_id, ts, user_id, text, raw_json) VALUES (?1, ?2, ?3, ?4, '{}')",
                )?;
                let mut seed: u64 = 42;
                for i in 0..100_000u64 {
                    let mut text = String::new();
                    for _ in 0..12 {
                        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                        text.push_str(words[(seed >> 33) as usize % words.len()]);
                        text.push(' ');
                    }
                    if i % 10_000 == 0 {
                        text.push_str("needle");
                    }
                    let conv = if i % 2 == 0 { "C1" } else { "C2" };
                    let user = if i % 3 == 0 { "U1" } else { "U2" };
                    stmt.execute(params![conv, format!("{}.000000", 1_600_000_000 + i * 60), user, text])?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .unwrap();

    for query in [
        "needle",
        "grades",
        "звіт дедлайн",
        "from:alice in:#general deploy",
        "репорт*",
        "has:link",
    ] {
        let q = search::parse(query).unwrap();
        let start = Instant::now();
        let hits = store
            .with_conn(|c| search::search(c, "T1", &q, Sort::Relevance, 50, ("[", "]")))
            .unwrap();
        let elapsed = start.elapsed();
        println!(
            "{query:<34} {:>3} hits  {:>6.1} ms",
            hits.len(),
            elapsed.as_secs_f64() * 1000.0
        );
        assert!(elapsed.as_millis() < 50, "{query} took {elapsed:?}");
    }
}
