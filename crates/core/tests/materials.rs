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

#[test]
fn materials_support_exclusions_with_pin_reactions_and_saved() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name, self_user_id) VALUES ('T1', 'Team', 'U0ME');
                 INSERT INTO users (id, team_id, name, display_name) VALUES
                    ('U0ME', 'T1', 'me', 'Me'),
                    ('U0BOB', 'T1', 'bob', 'Bob'),
                    ('U0ALICE', 'T1', 'alice', 'Alice');
                 INSERT INTO conversations (id, team_id, kind, name, label, dm_user_id) VALUES
                    ('C1', 'T1', 'public_channel', 'general', 'general', NULL),
                    ('C2', 'T1', 'public_channel', 'random', 'random', NULL),
                    ('D1', 'T1', 'im', NULL, 'Bob', 'U0BOB');",
            )?;

            let msg = |ts: &str, user: &str, text: &str| MessageRow {
                ts: ts.into(),
                user_id: Some(user.into()),
                text: text.into(),
                raw_json: "{}".into(),
                ..Default::default()
            };

            let mut m1 = msg("100.000000", "U0BOB", "bob https://bob.example/");
            m1.reactions.push(("tada".into(), "U0BOB".into()));

            let mut m2 = msg("200.000000", "U0ALICE", "alice https://alice.example/");
            m2.thread_ts = Some("200.000000".into());
            m2.reply_count = 1;
            m2.reactions
                .push(("tada::skin-tone-2".into(), "U0ME".into()));

            let mut m2_reply = msg("201.000000", "U0BOB", "reply https://reply.example/");
            m2_reply.thread_ts = Some("200.000000".into());

            messages::upsert_messages(c, "C1", &[m1, m2, m2_reply])?;
            messages::upsert_messages(
                c,
                "C2",
                &[msg("300.000000", "U0ME", "self https://self.example/")],
            )?;
            messages::upsert_messages(
                c,
                "D1",
                &[msg("400.000000", "U0ME", "dm https://dm.example/")],
            )?;

            c.execute(
                "INSERT INTO pins (conversation_id, ts, pinned_by, pinned_at)
                 VALUES ('C1', '200.000000', 'U0ME', 1)",
                [],
            )?;
            c.execute_batch(
                "INSERT INTO saved_messages (conversation_id, ts, user_id) VALUES
                    ('C1', '200.000000', 'U0ME'),
                    ('C2', '300.000000', 'U0BOB');",
            )?;

            // -from:bob excludes Bob's links, keeps Alice's and Self's.
            assert_eq!(
                keys(c, "-from:bob", MaterialSort::Newest),
                [
                    "https://dm.example/",
                    "https://self.example/",
                    "https://alice.example/",
                ]
            );
            // -in:#general excludes C1, keeps C2 and DM (even with name IS NULL).
            assert_eq!(
                keys(c, "-in:#general", MaterialSort::Newest),
                ["https://dm.example/", "https://self.example/"]
            );
            // has:pin matches only pinned message's link.
            assert_eq!(
                keys(c, "has:pin", MaterialSort::Newest),
                ["https://alice.example/"]
            );
            // is:saved matches only current user's saved rows (not U0BOB's saved row).
            assert_eq!(
                keys(c, "is:saved", MaterialSort::Newest),
                ["https://alice.example/"]
            );
            // has::tada: matches both exact and skin-tone; hasmy::tada: only U0ME's.
            assert_eq!(
                keys(c, "has::tada:", MaterialSort::Newest),
                ["https://alice.example/", "https://bob.example/"]
            );
            assert_eq!(
                keys(c, "hasmy::tada:", MaterialSort::Newest),
                ["https://alice.example/"]
            );
            // with:bob matches DM with bob and thread where bob replied, not bob's plain channel message.
            assert_eq!(
                keys(c, "with:bob", MaterialSort::Newest),
                [
                    "https://dm.example/",
                    "https://reply.example/",
                    "https://alice.example/",
                ]
            );
            // is:thread uses shared SQL predicate.
            assert_eq!(
                keys(c, "is:thread", MaterialSort::Newest),
                ["https://reply.example/", "https://alice.example/"]
            );
            Ok(())
        })
        .unwrap();
}
