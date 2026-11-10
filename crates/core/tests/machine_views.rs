//! Machine interface fixtures contain only invented identities and data.
use flek_core::api::{Conversation, User};
use flek_core::store::{
    directory, files,
    messages::{self, MessageRow},
    Store,
};
use flek_core::{
    error::ErrorKind,
    references::{self, Resolution},
    views, Error,
};
use serde_json::json;

fn seed(c: &mut rusqlite::Connection) -> flek_core::Result<()> {
    directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U1")?;
    let raw = json!({"id":"U1","name":"alice","real_name":"Alice Example","tz":"Europe/Kyiv","profile":{"display_name":"Ali","email":"alice@example.test"}});
    let mut user: User = serde_json::from_value(raw.clone())?;
    user.raw = raw;
    directory::upsert_users(c, "T1", &[user])?;
    let raw = json!([
        {"id":"C1","name":"general","is_channel":true,"topic":{"value":"Topic"},"purpose":{"value":"Purpose"}},
        {"id":"C2","name":"general-help","is_channel":true},
        {"id":"C3","name":"general-news","is_channel":true},
        {"id":"G1","name":"private","is_private":true},
        {"id":"G2","name":"mpdm-alice--bob-1","is_mpim":true},
        {"id":"D1","is_im":true,"user":"U1"}
    ]);
    let convs: Vec<Conversation> = serde_json::from_value(raw)?;
    directory::upsert_conversations(c, "T1", "U0", &convs)?;
    messages::upsert_messages(
        c,
        "C1",
        &[
            MessageRow {
                ts: "1700000000.123456".into(),
                thread_ts: Some("1700000000.123456".into()),
                user_id: Some("U1".into()),
                text: "Hi <@U1> <#C1|old> <https://example.test|site> &amp; &lt;literal&gt; &#33;"
                    .into(),
                reply_count: 1,
                edited_ts: Some("1700000001.000000".into()),
                raw_json: "{}".into(),
                reactions: vec![("wave".into(), "U2".into()), ("wave".into(), "U1".into())],
                files: vec![files::FileRow {
                    id: "F1".into(),
                    name: Some("note.txt".into()),
                    mimetype: Some("text/plain".into()),
                    size: Some(12),
                    raw_json: "{}".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            MessageRow {
                ts: "1700000002.000100".into(),
                thread_ts: Some("1700000000.123456".into()),
                text: "Reply".into(),
                raw_json: "{}".into(),
                ..Default::default()
            },
            MessageRow {
                ts: "1700000003.000000".into(),
                text: "Deleted".into(),
                deleted: true,
                raw_json: "{}".into(),
                ..Default::default()
            },
            MessageRow {
                ts: "1700000004.000000".into(),
                hidden_by_limit: true,
                raw_json: "{}".into(),
                ..Default::default()
            },
        ],
    )?;
    files::set_local_path(c, "F1", "/tmp/fake/note.txt")?;
    messages::set_sync_state(
        c,
        "C1",
        &messages::SyncState {
            oldest_ts: Some("1700000000.123456".into()),
            newest_ts: Some("1700000004.000000".into()),
            complete: true,
        },
    )?;
    c.execute(
        "UPDATE sync_state SET updated_at=1700000010 WHERE conversation_id='C1'",
        [],
    )?;
    directory::set_read_markers(c, &[("C1".into(), Some("1700000000.000000".into()))])?;
    Ok(())
}

#[test]
fn complete_views_are_stable_and_resolved_from_db() {
    let store = Store::open_in_memory().unwrap();
    store.with_conn(|c| {
        seed(c)?;
        let message=serde_json::to_value(views::message(c,"C1","1700000000.123456")?.unwrap())?;
        assert_eq!(message,json!({
            "conversation_id":"C1","conversation":"general","ts":"1700000000.123456","datetime":"2023-11-14T22:13:20.123456Z",
            "thread_ts":null,"reply_count":1,"user_id":"U1","user":"Ali",
            "text":"Hi <@U1> <#C1|old> <https://example.test|site> &amp; &lt;literal&gt; &#33;",
            "text_resolved":"Hi @Ali #general site (https://example.test) & <literal> !",
            "edited":true,"deleted":false,"hidden_by_limit":false,
            "files":[{"id":"F1","name":"note.txt","mimetype":"text/plain","size":12,"local_path":"/tmp/fake/note.txt"}],
            "reactions":[{"name":"wave","count":2,"users":["U1","U2"]}],
            "permalink":"https://fake.slack.com/archives/C1/p1700000000123456"
        }));
        let reply=views::message(c,"C1","1700000002.000100")?.unwrap();
        assert_eq!(reply.permalink.as_deref(),Some("https://fake.slack.com/archives/C1/p1700000002000100?thread_ts=1700000000.123456&cid=C1"));
        assert!(reply.user.is_none()); assert!(reply.files.is_empty()); assert!(reply.reactions.is_empty());
        assert!(views::message(c,"C1","1700000003.000000")?.unwrap().deleted);
        assert!(views::message(c,"C1","1700000004.000000")?.unwrap().hidden_by_limit);
        assert!(views::message(c,"C1","999.000000")?.is_none());
        assert_eq!(serde_json::to_value(views::conversation(c,"C1")?.unwrap())?,json!({
            "id":"C1","name":"general","label":"general","kind":"channel","is_archived":false,"is_member":true,"topic":"Topic","purpose":"Purpose",
            "unread_count":1,"last_read":"1700000000.000000","latest_ts":"1700000004.000000","synced_at":"2023-11-14T22:13:30Z","history_complete":true
        }));
        let unsynced=views::conversation(c,"G1")?.unwrap();
        assert!(unsynced.synced_at.is_none());assert!(unsynced.unread_count.is_none());assert!(!unsynced.history_complete);assert_eq!(unsynced.kind,"private");
        assert_eq!(serde_json::to_value(views::user(c,"U1")?.unwrap())?,json!({"id":"U1","name":"alice","display_name":"Ali","real_name":"Alice Example","is_bot":false,"deleted":false,"tz":"Europe/Kyiv"}));
        assert!(views::user(c,"U404")?.is_none());
        assert_eq!(views::resolve_text(c,"<@U404> <#C404|fallback> &amp;lt; &lt;@U1&gt; <https://a.test>")?,"@U404 #fallback &lt; <@U1> https://a.test");
        c.execute("UPDATE teams SET url=NULL",[])?;
        assert!(views::message(c,"C1","1700000002.000100")?.unwrap().permalink.is_none());
        Ok(())
    }).unwrap();
}

#[test]
fn references_rank_and_never_guess() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            seed(c)?;
            for (input, id) in [
                ("C1", "C1"),
                ("G1", "G1"),
                ("D1", "D1"),
                ("#general", "C1"),
                ("general", "C1"),
                ("GENERAL", "C1"),
                ("general-h", "C2"),
                ("@alice", "D1"),
                ("alice", "D1"),
                ("Ali", "D1"),
                ("Alice Example", "D1"),
                ("alice@example.test", "D1"),
                ("https://fake.slack.com/archives/C1/p1700000000123456", "C1"),
            ] {
                assert_eq!(
                    references::resolve(c, "T1", input, references::Match::AllowPrefix)?
                        .required(input)?
                        .id,
                    id,
                    "{input}"
                );
            }
            let Resolution::Ambiguous(candidates) =
                references::resolve(c, "T1", "gen", references::Match::AllowPrefix)?
            else {
                panic!("must not guess")
            };
            assert_eq!(
                candidates.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
                ["C1", "C2", "C3"]
            );
            for input in ["missing", "@general", "#alice", ""] {
                assert!(matches!(
                    references::resolve(c, "T1", input, references::Match::AllowPrefix)?,
                    Resolution::NotFound
                ));
            }
            assert!(matches!(
                references::resolve(c, "T2", "C1", references::Match::AllowPrefix)?,
                Resolution::NotFound
            ));
            // Two equally exact labels are ambiguous; an exact id beats both.
            c.execute(
                "UPDATE conversations SET label='C1' WHERE id IN ('C2','C3')",
                [],
            )?;
            assert_eq!(
                references::resolve(c, "T1", "C1", references::Match::AllowPrefix)?
                    .required("C1")?
                    .id,
                "C1"
            );
            c.execute(
                "UPDATE conversations SET label='same' WHERE id IN ('C2','C3')",
                [],
            )?;
            assert!(matches!(
                references::resolve(c, "T1", "same", references::Match::AllowPrefix)?,
                Resolution::Ambiguous(_)
            ));
            let parsed =
                references::message_reference(c, "T1", "#general", Some("1700000000.123456"))?;
            assert_eq!(parsed.conversation_id, "C1");
            assert!(parsed.thread_ts.is_none());
            assert!(references::message_reference(c, "T1", "general", Some("bad")).is_err());
            Ok(())
        })
        .unwrap();
}

#[test]
fn permalink_parser_validates_authority_path_and_timestamp() {
    let url =
        "https://fake.slack.com/archives/C1/p1700000002000100?thread_ts=1700000000.123456&cid=C1";
    let reference = references::parse_permalink(url).unwrap();
    assert_eq!(
        (
            reference.conversation_id.as_str(),
            reference.ts.as_str(),
            reference.thread_ts.as_deref()
        ),
        ("C1", "1700000002.000100", Some("1700000000.123456"))
    );
    for invalid in [
        "http://fake.slack.com/archives/C1/p1700000002000100",
        "https://slack.com.evil.test/archives/C1/p1700000002000100",
        "https://fake.slack.com/archives/C1/p170000000200010",
        "https://fake.slack.com/archives/C1/p170000000200010x",
        "https://fake.slack.com/archives/C1/p1700000002000100/extra",
        "https://fake.slack.com/archives/C1/p1700000002000100?thread_ts=bad",
    ] {
        assert!(references::parse_permalink(invalid).is_none(), "{invalid}");
    }
}

#[test]
fn typed_errors_have_stable_exit_codes() {
    let cases = [
        (Error::Store("test".into()), ErrorKind::Internal, 1),
        (Error::Usage("test".into()), ErrorKind::Usage, 2),
        (Error::NotFound("missing".into()), ErrorKind::NotFound, 3),
        (
            Error::Ambiguous { candidates: vec![] },
            ErrorKind::Ambiguous,
            4,
        ),
        (Error::NoCredentials, ErrorKind::Auth, 5),
        (
            Error::Slack {
                code: "invalid_auth".into(),
                needed: None,
            },
            ErrorKind::Auth,
            5,
        ),
        (
            Error::Slack {
                code: "token_revoked".into(),
                needed: None,
            },
            ErrorKind::Auth,
            5,
        ),
        (
            Error::Slack {
                code: "ratelimited".into(),
                needed: None,
            },
            ErrorKind::RateLimited,
            6,
        ),
        (Error::HttpStatus(429), ErrorKind::RateLimited, 6),
        (Error::HttpStatus(503), ErrorKind::Network, 7),
        (
            Error::Slack {
                code: "channel_not_found".into(),
                needed: None,
            },
            ErrorKind::Network,
            7,
        ),
        (Error::Forbidden, ErrorKind::Forbidden, 8),
    ];
    for (error, kind, code) in cases {
        assert_eq!(error.kind(), kind);
        assert_eq!(kind.exit_code(), code);
        assert!(serde_json::to_value(kind).unwrap().is_string());
    }
}

#[test]
fn exact_mode_rejects_prefixes_with_candidates() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            seed(c)?;
            for query in ["general-h", "@alic", "gen"] {
                let Resolution::Ambiguous(candidates) =
                    references::resolve(c, "T1", query, references::Match::Exact)?
                else {
                    panic!("strict mode guessed {query}");
                };
                assert!(!candidates.is_empty());
                if query == "general-h" {
                    assert_eq!(candidates.len(), 1);
                    assert_eq!(candidates[0].id, "C2");
                }
            }
            for (query, id) in [
                ("C1", "C1"),
                ("#general", "C1"),
                ("GENERAL", "C1"),
                ("@alice", "D1"),
                ("Alice Example", "D1"),
                ("alice@example.test", "D1"),
            ] {
                assert_eq!(
                    references::resolve(c, "T1", query, references::Match::Exact)?
                        .required(query)?
                        .id,
                    id
                );
            }
            assert_eq!(
                references::resolve(c, "T1", "general-h", references::Match::AllowPrefix)?
                    .required("general-h")?
                    .id,
                "C2"
            );
            assert!(matches!(
                references::resolve(c, "T1", "missing", references::Match::Exact)?,
                Resolution::NotFound
            ));
            let message = references::resolve(c, "T1", "missing", references::Match::Exact)?
                .required("missing")
                .unwrap_err()
                .to_string();
            assert_eq!(
                message,
                "no conversation matches \"missing\"; try `flek channels`"
            );
            Ok(())
        })
        .unwrap();
}
