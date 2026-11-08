//! Stored conversation counters (migration 0007) always equal a real count.

use flek_core::store::directory::{self, set_read_markers};
use flek_core::store::messages::{self, MessageRow};
use flek_core::store::{migrations, Store};
use rusqlite::{params, Connection};

fn setup(c: &Connection) {
    c.execute_batch(
        "INSERT INTO teams (id, name, self_user_id) VALUES ('T1', 'Team', 'U0ME');
         INSERT INTO conversations (id, team_id, kind, name) VALUES
             ('C1', 'T1', 'public_channel', 'general'),
             ('C2', 'T1', 'public_channel', 'random');",
    )
    .unwrap();
}

fn msg(ts: &str, user: &str) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        user_id: Some(user.into()),
        text: "x".into(),
        raw_json: "{}".into(),
        ..Default::default()
    }
}

/// `(messages, unread, latest_ts)` counted from scratch, as before 0007.
fn recount(c: &Connection, id: &str) -> (i64, Option<i64>, Option<String>) {
    c.query_row(
        "SELECT
            (SELECT COUNT(*) FROM messages m WHERE m.conversation_id = cv.id AND m.deleted = 0),
            CASE WHEN last_read IS NULL THEN NULL ELSE (
                SELECT COUNT(*) FROM messages m
                WHERE m.conversation_id = cv.id AND m.deleted = 0 AND m.ts > last_read
                  AND (m.thread_ts IS NULL OR m.thread_ts = m.ts OR m.subtype = 'thread_broadcast')
                  AND m.user_id IS NOT (SELECT self_user_id FROM teams WHERE teams.id = cv.team_id)
            ) END,
            (SELECT MAX(ts) FROM messages m WHERE m.conversation_id = cv.id)
         FROM conversations cv WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

fn stored(c: &Connection, id: &str) -> (i64, Option<i64>, Option<String>) {
    let rows = directory::list_conversations(c, "T1").unwrap();
    let r = rows.into_iter().find(|r| r.id == id).unwrap();
    (r.messages, r.unread, r.latest_ts)
}

fn check(c: &Connection, step: &str) {
    for id in ["C1", "C2"] {
        assert_eq!(stored(c, id), recount(c, id), "{id} after {step}");
    }
}

#[test]
fn counters_follow_every_change() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            setup(c);
            check(c, "setup");

            messages::upsert_messages(
                c,
                "C1",
                &[
                    msg("100.000000", "U0BOB"),
                    msg("200.000000", "U0BOB"),
                    msg("300.000000", "U0ME"),
                ],
            )?;
            check(c, "insert without a read marker");
            assert_eq!(stored(c, "C1").1, None);

            set_read_markers(c, &[("C1".into(), Some("150.000000".into()))])?;
            check(c, "read marker set");
            assert_eq!(stored(c, "C1"), (3, Some(1), Some("300.000000".into())));

            // A thread reply is counted as a message but not as unread;
            // a broadcast is unread.
            let mut reply = msg("400.000000", "U0BOB");
            reply.thread_ts = Some("200.000000".into());
            let mut broadcast = msg("500.000000", "U0BOB");
            broadcast.thread_ts = Some("200.000000".into());
            broadcast.subtype = Some("thread_broadcast".into());
            messages::upsert_messages(c, "C1", &[reply, broadcast])?;
            check(c, "reply and broadcast");
            assert_eq!(stored(c, "C1").1, Some(2));

            // Re-upserting the same rows (edits) changes nothing.
            messages::upsert_messages(c, "C1", &[msg("200.000000", "U0BOB")])?;
            check(c, "edit");

            messages::mark_deleted(c, "C1", &["200.000000".into()])?;
            check(c, "deleted flag");
            assert_eq!(stored(c, "C1"), (4, Some(1), Some("500.000000".into())));

            set_read_markers(c, &[("C1".into(), Some("450.000000".into()))])?;
            check(c, "marker moved");

            c.execute(
                "UPDATE teams SET self_user_id = 'U0BOB' WHERE id = 'T1'",
                [],
            )?;
            check(c, "self user changed");

            c.execute("DELETE FROM reactions", [])?;
            c.execute("DELETE FROM messages WHERE ts = '100.000000'", [])?;
            check(c, "row deleted");

            set_read_markers(c, &[("C1".into(), None)])?;
            check(c, "marker cleared");

            messages::upsert_messages(c, "C2", &[msg("050.000000", "U0BOB")])?;
            check(c, "other conversation");
            Ok(())
        })
        .unwrap();
}

#[test]
fn migration_fills_counters_for_existing_data() {
    let mut c = Connection::open_in_memory().unwrap();
    c.pragma_update(None, "foreign_keys", false).unwrap();
    migrations::migrate_to(&mut c, 6).unwrap();
    setup(&c);
    for (ts, user) in [
        ("100.000000", "U0BOB"),
        ("200.000000", "U0BOB"),
        ("300.000000", "U0ME"),
    ] {
        c.execute(
            "INSERT INTO messages (conversation_id, ts, user_id, text) VALUES ('C1', ?1, ?2, 'x')",
            params![ts, user],
        )
        .unwrap();
    }
    c.execute(
        "UPDATE conversations SET last_read = '150.000000' WHERE id = 'C1'",
        [],
    )
    .unwrap();

    migrations::migrate(&mut c).unwrap();
    check(&c, "migration");
    assert_eq!(stored(&c, "C1"), (3, Some(1), Some("300.000000".into())));
}
