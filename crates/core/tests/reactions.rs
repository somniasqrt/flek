//! Reactions, edits and deletions.

use flek_core::api::Message;
use flek_core::store::messages::{self, MessageRow};
use flek_core::store::Store;
use serde_json::json;

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
                 INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');",
            )?;
            Ok(())
        })
        .unwrap();
    store
}

fn row(ts: &str, text: &str, reactions: &[(&str, &str)]) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        text: text.into(),
        raw_json: "{}".into(),
        reactions: reactions
            .iter()
            .map(|(n, u)| (n.to_string(), u.to_string()))
            .collect(),
        ..Default::default()
    }
}

#[test]
fn reactions_follow_the_latest_version() {
    let store = store();
    store
        .with_conn(|c| {
            messages::upsert_messages(
                c,
                "C1",
                &[row(
                    "1.000000",
                    "hi",
                    &[("wave", "U1"), ("wave", "U2"), ("tada", "U1")],
                )],
            )?;
            assert_eq!(
                messages::reactions(c, "C1", "1.000000")?,
                [("wave".to_string(), 2), ("tada".to_string(), 1)]
            );
            // U2 removed their reaction.
            messages::upsert_messages(
                c,
                "C1",
                &[row("1.000000", "hi", &[("wave", "U1"), ("tada", "U1")])],
            )?;
            assert_eq!(
                messages::reactions(c, "C1", "1.000000")?,
                [("tada".to_string(), 1), ("wave".to_string(), 1)]
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn hidden_version_keeps_reactions() {
    let store = store();
    store
        .with_conn(|c| {
            messages::upsert_messages(c, "C1", &[row("1.000000", "hi", &[("wave", "U1")])])?;
            let mut hidden = row("1.000000", "", &[]);
            hidden.hidden_by_limit = true;
            messages::upsert_messages(c, "C1", &[hidden])?;
            assert_eq!(messages::reactions(c, "C1", "1.000000")?.len(), 1);
            Ok(())
        })
        .unwrap();
}

#[test]
fn deleted_messages_disappear_from_views_and_can_return() {
    let store = store();
    store
        .with_conn(|c| {
            messages::upsert_messages(
                c,
                "C1",
                &[row("1.000000", "a", &[]), row("2.000000", "b", &[])],
            )?;
            assert_eq!(messages::mark_deleted(c, "C1", &["1.000000".into()])?, 1);
            assert_eq!(messages::recent(c, "C1", 10)?.len(), 1);
            // Seen again in Slack (e.g. a wrong guess) → visible again.
            messages::upsert_messages(c, "C1", &[row("1.000000", "a", &[])])?;
            assert_eq!(messages::recent(c, "C1", 10)?.len(), 2);
            Ok(())
        })
        .unwrap();
}

#[test]
fn api_message_parses_reactions_and_edits() {
    let raw = json!({
        "ts": "1.000000", "user": "U1", "text": "fixed typo",
        "edited": {"user": "U1", "ts": "2.000000"},
        "reactions": [{"name": "+1", "users": ["U2", "U3"], "count": 2}]
    });
    let m: Message = serde_json::from_value(raw).unwrap();
    assert_eq!(m.edited.unwrap().ts.as_deref(), Some("2.000000"));
    assert_eq!(m.reactions[0].users, ["U2", "U3"]);
}
