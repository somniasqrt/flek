//! Desktop notification policy (M6.2.1): pure decision and SQLite collection.

use flek_core::notifications::{self, Candidate, Context, Target};
use flek_core::settings::Settings;
use flek_core::store::messages::{self, MessageRow};
use flek_core::store::presence::Presence;
use flek_core::store::Store;
use rusqlite::Connection;

const ME: &str = "U0ME";
const NOW: i64 = 1_000;

fn quiet() -> Presence {
    Presence {
        user_id: ME.into(),
        dnd_fresh: true,
        ..Default::default()
    }
}

fn candidate(kind: &str, preference: &str, text: &str) -> Candidate {
    Candidate {
        target: Target {
            team_id: "T1".into(),
            conversation_id: "C1".into(),
            ts: "500.000000".into(),
            thread_ts: None,
        },
        sender: Some("U0BOB".into()),
        kind: kind.into(),
        preference: preference.into(),
        text: text.into(),
        subtype: None,
        deleted: false,
        hidden: false,
        member: true,
        archived: false,
        last_read: None,
        conversation: "general".into(),
        author: String::new(),
    }
}

fn decide(message: &Candidate, settings: &Settings, presence: Option<&Presence>) -> bool {
    notifications::should_notify(
        message,
        settings,
        &Context {
            user_id: ME,
            focused_conversation: None,
            presence,
            now: NOW,
        },
    )
}

#[test]
fn default_preference_notifies_dms_and_mentions_only() {
    let s = Settings::default();
    let p = quiet();
    assert!(decide(&candidate("im", "default", "hi"), &s, Some(&p)));
    assert!(decide(&candidate("mpim", "default", "hi"), &s, Some(&p)));
    assert!(!decide(
        &candidate("public_channel", "default", "hi"),
        &s,
        Some(&p)
    ));
    for text in [
        "hey <@U0ME>",
        "<!here> standup",
        "<!channel>",
        "<!everyone>",
    ] {
        assert!(
            decide(&candidate("public_channel", "default", text), &s, Some(&p)),
            "{text}"
        );
    }
    // Someone else's mention is not mine.
    assert!(!decide(
        &candidate("public_channel", "default", "<@U0ME2>"),
        &s,
        Some(&p)
    ));
}

#[test]
fn explicit_preferences() {
    let s = Settings::default();
    let p = quiet();
    assert!(decide(
        &candidate("public_channel", "all", "hi"),
        &s,
        Some(&p)
    ));
    assert!(!decide(&candidate("im", "mentions", "hi"), &s, Some(&p)));
    assert!(decide(
        &candidate("im", "mentions", "<@U0ME>"),
        &s,
        Some(&p)
    ));
    assert!(!decide(&candidate("im", "muted", "<@U0ME>"), &s, Some(&p)));
}

#[test]
fn suppressed_cases() {
    let s = Settings::default();
    let p = quiet();
    let base = || candidate("im", "all", "<@U0ME>");
    assert!(decide(&base(), &s, Some(&p)));

    let off = Settings {
        notifications_enabled: false,
        ..Settings::default()
    };
    assert!(!decide(&base(), &off, Some(&p)), "disabled");

    let mut m = base();
    m.sender = Some(ME.into());
    assert!(!decide(&m, &s, Some(&p)), "own message");
    let mut m = base();
    m.deleted = true;
    assert!(!decide(&m, &s, Some(&p)), "deleted");
    let mut m = base();
    m.hidden = true;
    assert!(!decide(&m, &s, Some(&p)), "hidden by limit");
    let mut m = base();
    m.member = false;
    assert!(!decide(&m, &s, Some(&p)), "not a member");
    let mut m = base();
    m.archived = true;
    assert!(!decide(&m, &s, Some(&p)), "archived");
    let mut m = base();
    m.last_read = Some("500.000000".into());
    assert!(!decide(&m, &s, Some(&p)), "already read");
    let mut m = base();
    m.last_read = Some("499.000000".into());
    assert!(decide(&m, &s, Some(&p)), "read cursor before the message");
    let mut m = base();
    m.subtype = Some("channel_join".into());
    assert!(!decide(&m, &s, Some(&p)), "system subtype");
    for subtype in [
        "bot_message",
        "file_share",
        "thread_broadcast",
        "me_message",
    ] {
        let mut m = base();
        m.subtype = Some(subtype.into());
        assert!(decide(&m, &s, Some(&p)), "{subtype}");
    }

    let focused = notifications::should_notify(
        &base(),
        &s,
        &Context {
            user_id: ME,
            focused_conversation: Some("C1"),
            presence: Some(&p),
            now: NOW,
        },
    );
    assert!(!focused, "open and focused conversation");
}

#[test]
fn dnd_snooze_and_unknown_presence_are_quiet() {
    let s = Settings::default();
    let m = candidate("im", "all", "hi");
    assert!(!decide(&m, &s, None), "no presence row");
    let stale = Presence {
        dnd_fresh: false,
        ..quiet()
    };
    assert!(!decide(&m, &s, Some(&stale)), "stale DND");
    let snoozed = Presence {
        snooze_enabled: true,
        snooze_end: NOW + 60,
        ..quiet()
    };
    assert!(!decide(&m, &s, Some(&snoozed)), "snoozed");
    let snooze_over = Presence {
        snooze_enabled: true,
        snooze_end: NOW,
        ..quiet()
    };
    assert!(decide(&m, &s, Some(&snooze_over)), "snooze ended");
    let scheduled = Presence {
        dnd_enabled: true,
        dnd_start: NOW - 10,
        dnd_end: NOW + 10,
        ..quiet()
    };
    assert!(!decide(&m, &s, Some(&scheduled)), "scheduled DND");
}

fn setup(c: &Connection) {
    c.execute_batch(
        "INSERT INTO teams (id, name, self_user_id) VALUES ('T1', 'Team', 'U0ME');
         INSERT INTO conversations (id, team_id, kind, name, is_member) VALUES
             ('D1', 'T1', 'im', NULL, 1),
             ('C1', 'T1', 'public_channel', 'general', 1),
             ('C2', 'T1', 'public_channel', 'loud', 1);
         UPDATE conversations SET notification = 'all', notification_explicit = 1 WHERE id = 'C2';
         INSERT INTO users (id, team_id, name) VALUES ('U0BOB', 'T1', 'bob');",
    )
    .unwrap();
}

fn row(ts: &str, user: &str, text: &str) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        user_id: Some(user.into()),
        text: text.into(),
        raw_json: "{}".into(),
        ..Default::default()
    }
}

fn collect(c: &Connection, after: i64, since: &str) -> (i64, notifications::Batch) {
    let p = quiet();
    notifications::collect(
        c,
        after,
        since,
        "T1",
        &Settings::default(),
        &Context {
            user_id: ME,
            focused_conversation: None,
            presence: Some(&p),
            now: NOW,
        },
    )
    .unwrap()
}

#[test]
fn collect_advances_cursor_skips_backfill_and_coalesces() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            setup(c);
            // Older than the startup cutoff: history, never notified.
            messages::upsert_messages(c, "D1", &[row("100.000000", "U0BOB", "old")])?;
            let start = notifications::watermark(c)?;
            let (cursor, batch) = collect(c, 0, "200.000000");
            assert_eq!(batch.count, 0, "backfill before the cutoff");
            assert_eq!(cursor, start);

            messages::upsert_messages(c, "D1", &[row("300.000000", "U0BOB", "first")])?;
            messages::upsert_messages(c, "C1", &[row("301.000000", "U0BOB", "plain")])?;
            messages::upsert_messages(c, "C2", &[row("302.000000", "U0BOB", "loud  \n text")])?;
            messages::upsert_messages(c, "D1", &[row("303.000000", ME, "mine")])?;
            messages::upsert_messages(c, "C1", &[row("299.000000", "U0BOB", "<@U0ME> ping")])?;
            let (next, batch) = collect(c, cursor, "200.000000");
            // DM, the explicit 'all' channel and a mention in a default channel;
            // a plain message in a never-configured channel (DB default 'all') and
            // my own message are skipped.
            assert_eq!(batch.count, 3);
            let latest = batch.latest.unwrap();
            assert_eq!(latest.target.conversation_id, "C2");
            assert_eq!(latest.target.ts, "302.000000");
            assert_eq!(latest.text, "loud text", "whitespace collapsed");
            assert!(!latest.author.is_empty());
            assert!(next > cursor);

            // Re-upserting (edit / RTM echo) does not create a new identity.
            messages::upsert_messages(c, "D1", &[row("300.000000", "U0BOB", "first, edited")])?;
            let (again, batch) = collect(c, next, "200.000000");
            assert_eq!(batch.count, 0, "edits and echoes never re-notify");
            assert_eq!(again, next);
            Ok(())
        })
        .unwrap();
}

#[test]
fn badge_counts_unread_except_muted() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            setup(c);
            c.execute_batch(
                "UPDATE conversations SET last_read = '0' WHERE team_id = 'T1';
                 UPDATE conversations SET notification = 'muted' WHERE id = 'C1';",
            )?;
            messages::upsert_messages(c, "D1", &[row("300.000000", "U0BOB", "a")])?;
            messages::upsert_messages(c, "C1", &[row("301.000000", "U0BOB", "b")])?;
            messages::upsert_messages(c, "C2", &[row("302.000000", "U0BOB", "c")])?;
            assert_eq!(notifications::badge_count(c, "T1")?, 2);
            Ok(())
        })
        .unwrap();
}
