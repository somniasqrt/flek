//! Links and files catalogue (M5.1.4): extraction, maintenance on message
//! writes, migration backfill and keyset pagination.

use flek_core::reads::{materials, MaterialSort};
use flek_core::store::materials::links;
use flek_core::store::messages::{self, MessageRow};
use flek_core::store::{migrations, Store};
use rusqlite::{params, Connection};

#[test]
fn links_come_from_text_blocks_and_unfurls_once_each() {
    let found = links(
        "see <https://a.example/x?y=1&amp;z=2|label> and https://b.example/p). \
         also ftp://nope.example and https://user:pw@c.example/",
        Some(
            r#"[{"type":"rich_text","elements":[{"type":"rich_text_section","elements":[{"type":"link","url":"https://d.example/"}]}]}]"#,
        ),
        Some(
            r#"{"attachments":[{"from_url":"https://a.example/x?y=1&z=2","title":"Unfurl title"}]}"#,
        ),
    );
    let keys: Vec<_> = found.keys().map(String::as_str).collect();
    assert!(keys.contains(&"https://a.example/x?y=1&z=2"), "{keys:?}");
    assert!(
        keys.contains(&"https://b.example/p"),
        "trailing ')' and '.' trimmed: {keys:?}"
    );
    assert!(keys.contains(&"https://d.example/"), "{keys:?}");
    assert!(!keys.iter().any(|k| k.starts_with("ftp:")), "{keys:?}");
    assert!(
        !keys.iter().any(|k| k.contains('@')),
        "credentials in URL refused: {keys:?}"
    );
    assert_eq!(found["https://a.example/x?y=1&z=2"], "Unfurl title");
}

fn setup(c: &Connection) {
    c.execute_batch(
        "INSERT INTO teams (id, name, self_user_id) VALUES ('T1', 'Team', 'U0ME'), ('T2', 'Other', 'U0ME');
         INSERT INTO conversations (id, team_id, kind, name) VALUES
             ('C1', 'T1', 'public_channel', 'general'),
             ('C9', 'T2', 'public_channel', 'elsewhere');",
    )
    .unwrap();
}

fn row(ts: &str, text: &str) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        user_id: Some("U0BOB".into()),
        text: text.into(),
        raw_json: "{}".into(),
        ..Default::default()
    }
}

fn keys(c: &Connection, query: &str, sort: MaterialSort) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = None;
    loop {
        let page = materials(c, "T1", query, sort, 2, cursor.as_deref()).unwrap();
        assert!(page.items.len() <= 2);
        out.extend(page.items.into_iter().map(|m| m.key));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    out
}

#[test]
fn catalogue_follows_writes_and_pages_without_gaps() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            setup(c);
            messages::upsert_messages(
                c,
                "C1",
                &[
                    row("100.000000", "first https://one.example/"),
                    row(
                        "200.000000",
                        "two links https://two.example/ https://three.example/",
                    ),
                    row("300.000000", "no link here"),
                    row("400.000000", "last https://four.example/"),
                ],
            )?;
            messages::upsert_messages(
                c,
                "C9",
                &[row("150.000000", "https://other-team.example/")],
            )?;

            let newest = keys(c, "", MaterialSort::Newest);
            assert_eq!(newest.len(), 4, "{newest:?}");
            assert_eq!(newest[0], "https://four.example/");
            assert_eq!(newest[3], "https://one.example/");
            assert!(
                !newest.iter().any(|k| k.contains("other-team")),
                "team isolation"
            );
            let mut oldest = keys(c, "", MaterialSort::Oldest);
            assert_eq!(
                oldest.first().map(String::as_str),
                Some("https://one.example/")
            );
            oldest.sort();
            let mut sorted = newest.clone();
            sorted.sort();
            assert_eq!(
                oldest, sorted,
                "both orders cover the same rows exactly once"
            );

            // Free words match labels/URLs and the message text, so a word from a
            // two-link message returns both of its shares.
            assert_eq!(
                keys(c, "first", MaterialSort::Newest),
                ["https://one.example/"]
            );
            assert_eq!(keys(c, "two", MaterialSort::Newest).len(), 2);

            // An edit replaces the message's links instead of adding to them.
            messages::upsert_messages(
                c,
                "C1",
                &[row("400.000000", "edited https://five.example/")],
            )?;
            let after = keys(c, "", MaterialSort::Newest);
            assert!(
                after.contains(&"https://five.example/".to_string()),
                "{after:?}"
            );
            assert!(
                !after.contains(&"https://four.example/".to_string()),
                "{after:?}"
            );

            // A cursor from another query is refused.
            let page = materials(c, "T1", "", MaterialSort::Newest, 1, None)?;
            let cursor = page.next_cursor.unwrap();
            assert!(materials(c, "T1", "three", MaterialSort::Newest, 1, Some(&cursor)).is_err());
            assert!(materials(c, "T1", "", MaterialSort::Oldest, 1, Some(&cursor)).is_err());
            Ok(())
        })
        .unwrap();
}

#[test]
fn migration_backfills_existing_history() {
    let mut c = Connection::open_in_memory().unwrap();
    c.pragma_update(None, "foreign_keys", false).unwrap();
    migrations::migrate_to(&mut c, 22).unwrap();
    setup(&c);
    for (ts, text) in [
        ("100.000000", "old https://old.example/a"),
        ("200.000000", "plain"),
    ] {
        c.execute(
            "INSERT INTO messages (conversation_id, ts, user_id, text) VALUES ('C1', ?1, 'U0BOB', ?2)",
            params![ts, text],
        )
        .unwrap();
    }
    migrations::migrate(&mut c).unwrap();
    assert_eq!(
        keys(&c, "", MaterialSort::Newest),
        ["https://old.example/a"]
    );
}
