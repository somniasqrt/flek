//! Own-message edit/delete through the agent write guard (M4.5, M8.4.2): offline
//! ownership checks, documented requests, local state like live events, error
//! mapping. Mock Slack only; no Keychain.
mod support;
use flek_core::{
    agent::{ActionLog, AgentConfig},
    api::{self, Client},
    rtm::Event,
    store::{
        directory,
        messages::{self, MessageRow},
        Store,
    },
    sync::live,
    writes::{PreparedWrite, WriteReceipt, WriteSession},
    Error, Secret,
};
use serde_json::{json, Value};
use support::{field, has_field};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

const MINE: &str = "1700000000.000100";
const THEIRS: &str = "1700000000.000200";
const BOT: &str = "1700000000.000300";
const GONE: &str = "1700000000.000400";

fn row(ts: &str, user: &str, text: &str, raw: Value) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        user_id: Some(user.into()),
        text: text.into(),
        raw_json: raw.to_string(),
        ..Default::default()
    }
}

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.with_conn(|c| {
        directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U1")?;
        c.execute_batch(
            "INSERT INTO users (id, team_id, name, real_name, display_name, deleted) VALUES
               ('U1','T1','me','Me Myself','me',0),
               ('U2','T1','alice','Alice A','alice',0);
             INSERT INTO conversations (id, team_id, kind, name, label) VALUES
               ('C1','T1','public_channel','general','general'),
               ('C2','T1','public_channel','random','random');",
        )?;
        messages::upsert_messages(
            c,
            "C1",
            &[
                row(
                    MINE,
                    "U1",
                    "original words",
                    json!({"ts": MINE, "user": "U1"}),
                ),
                row(
                    THEIRS,
                    "U2",
                    "not mine",
                    json!({"ts": THEIRS, "user": "U2"}),
                ),
                row(
                    BOT,
                    "U1",
                    "bot post",
                    json!({"ts": BOT, "user": "U1", "bot_id": "B1"}),
                ),
                MessageRow {
                    deleted: true,
                    ..row(GONE, "U1", "removed", json!({"ts": GONE, "user": "U1"}))
                },
            ],
        )?;
        Ok(())
    })
    .unwrap();
    s
}

fn client(server: &MockServer) -> Client {
    Client::with_base_url(
        Secret::new("xoxc-fake-token"),
        Secret::new("xoxd-fake-cookie"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing()
}

fn session(server: &MockServer) -> WriteSession {
    WriteSession {
        client: client(server),
        team_id: "T1".into(),
        user_id: "U1".into(),
    }
}

fn edit_of(
    s: &Store,
    reference: &str,
    ts: Option<&str>,
    text: &str,
) -> Result<PreparedWrite, Error> {
    s.with_read(|c| PreparedWrite::own_message(c, "T1", false, reference, ts, Some(text)))
}

fn delete_of(s: &Store, reference: &str, ts: Option<&str>) -> Result<PreparedWrite, Error> {
    s.with_read(|c| PreparedWrite::own_message(c, "T1", true, reference, ts, None))
}

async fn run(
    p: &PreparedWrite,
    s: &Store,
    server: &MockServer,
    dir: &tempfile::TempDir,
) -> Result<WriteReceipt, Error> {
    let cfg = AgentConfig::default();
    p.run_guarded(
        &ActionLog::new(dir.path().join("log.db")),
        &cfg,
        true,
        s,
        || async { Ok(session(server)) },
    )
    .await
}

fn stored(s: &Store, ts: &str) -> (String, Option<String>, bool) {
    s.with_read(|c| {
        Ok(c.query_row(
            "SELECT text, edited_ts, deleted FROM messages WHERE conversation_id='C1' AND ts=?1",
            [ts],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? == 1)),
        )?)
    })
    .unwrap()
}

fn fts_hits(s: &Store, word: &str) -> i64 {
    s.with_read(|c| {
        Ok(c.query_row(
            "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH ?1",
            [word],
            |r| r.get(0),
        )?)
    })
    .unwrap()
}

#[test]
fn preparation_accepts_only_own_stored_messages_offline() {
    let s = store();
    let kind =
        |r: Result<PreparedWrite, Error>| r.err().expect("must be refused").kind().exit_code();
    // Someone else's, a bot's (even under my user id), deleted, unknown, wrong channel.
    assert_eq!(kind(edit_of(&s, "general", Some(THEIRS), "x")), 8);
    assert_eq!(kind(delete_of(&s, "general", Some(THEIRS))), 8);
    assert_eq!(kind(edit_of(&s, "general", Some(BOT), "x")), 8);
    assert_eq!(kind(delete_of(&s, "general", Some(GONE))), 3);
    assert_eq!(kind(delete_of(&s, "general", Some("1700000000.999999"))), 3);
    assert_eq!(kind(delete_of(&s, "random", Some(MINE))), 3);
    assert_eq!(kind(delete_of(&s, "nope", Some(MINE))), 3);
    // Malformed ts and empty replacement text are usage errors.
    assert_eq!(kind(delete_of(&s, "general", Some("123"))), 2);
    assert_eq!(kind(edit_of(&s, "general", Some(MINE), "  \n")), 2);
    assert_eq!(kind(edit_of(&s, "general", None, "x")), 2);
    // Unlike a bare "general", no prefix guessing.
    assert_eq!(kind(delete_of(&s, "gener", Some(MINE))), 4);
}

#[test]
fn prepared_form_is_exact_and_credential_free() {
    let s = store();
    let edit = edit_of(&s, "#general", Some(MINE), "hi @alice & <b>").unwrap();
    assert_eq!(edit.action.action, "edit");
    assert_eq!(edit.action.targets, ["C1"]);
    assert_eq!(edit.request.method, "chat.update");
    assert_eq!(
        serde_json::to_value(&edit.request.params).unwrap(),
        json!({"channel":"C1","ts":MINE,"text":"hi <@U2> &amp; &lt;b&gt;"})
    );
    let del = delete_of(&s, "general", Some(MINE)).unwrap();
    assert_eq!(del.action.action, "delete");
    assert_eq!(del.request.method, "chat.delete");
    assert_eq!(
        serde_json::to_value(&del.request.params).unwrap(),
        json!({"channel":"C1","ts":MINE})
    );
    // A permalink resolves to the same target.
    let link = format!(
        "https://fake.slack.com/archives/C1/p{}",
        MINE.replace('.', "")
    );
    let by_link = delete_of(&s, &link, None).unwrap();
    assert_eq!(by_link.request.params["ts"], MINE);
    // The dry-run report needs nothing but the prepared form.
    let report = del.dry_run_report(&AgentConfig::default(), false);
    assert_eq!(report["authorized"], false);
    assert_eq!(report["write"]["request"]["method"], "chat.delete");
}

#[tokio::test]
async fn denied_edit_or_delete_never_reaches_slack_or_the_session() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .expect(0)
        .mount(&server)
        .await;
    let s = store();
    let dir = tempfile::tempdir().unwrap();
    let log = ActionLog::new(dir.path().join("log.db"));
    let cfg = AgentConfig::default();
    for p in [
        edit_of(&s, "general", Some(MINE), "new").unwrap(),
        delete_of(&s, "general", Some(MINE)).unwrap(),
    ] {
        let denied = p
            .run_guarded(&log, &cfg, false, &s, || async {
                panic!("session must not be resolved when denied")
            })
            .await
            .err()
            .unwrap();
        assert!(matches!(denied, Error::Forbidden));
    }
    assert!(log.entries(10).unwrap().is_empty());
    assert_eq!(stored(&s, MINE).0, "original words");
}

#[tokio::test]
async fn edit_sends_one_escaped_update_and_stores_it_like_the_live_event() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.update"))
        .and(field("channel", "C1"))
        .and(field("ts", MINE))
        .and(field("text", "fresh &amp; <@U2>"))
        .and(support::has_field("text"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "channel": "C1", "ts": MINE, "text": "fresh &amp; <@U2>",
            "message": {"text": "fresh &amp; <@U2>", "user": "U1",
                        "edited": {"user": "U1", "ts": "1700000500.000000"}}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    assert_eq!(fts_hits(&s, "original"), 1);
    let dir = tempfile::tempdir().unwrap();
    let p = edit_of(&s, "general", Some(MINE), "fresh & @alice").unwrap();
    let receipt = run(&p, &s, &server, &dir).await.unwrap();
    assert_eq!(receipt.action, "edit");
    assert_eq!(receipt.ts.as_deref(), Some(MINE));
    assert!(!receipt.not_stored_locally);
    let message = receipt.message.expect("re-read from SQLite");
    assert!(message.edited);
    assert_eq!(message.text, "fresh &amp; <@U2>");
    assert_eq!(
        stored(&s, MINE),
        (
            "fresh &amp; <@U2>".into(),
            Some("1700000500.000000".into()),
            false
        )
    );
    // FTS follows the edit; neighbours are untouched.
    assert_eq!(fts_hits(&s, "original"), 0);
    assert_eq!(fts_hits(&s, "fresh"), 1);
    assert_eq!(stored(&s, THEIRS).0, "not mine");
    let log = ActionLog::new(dir.path().join("log.db"))
        .entries(5)
        .unwrap();
    assert_eq!(
        (log[0].action.as_str(), log[0].outcome.as_str()),
        ("edit", "accepted")
    );
}

#[tokio::test]
async fn edit_without_a_version_in_the_response_is_provisional_and_idempotent_with_the_echo() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.update"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "channel": "C1", "ts": MINE, "text": "second"})),
        )
        .mount(&server)
        .await;
    let s = store();
    let dir = tempfile::tempdir().unwrap();
    let p = edit_of(&s, "general", Some(MINE), "second").unwrap();
    run(&p, &s, &server, &dir).await.unwrap();
    let (text, edited, _) = stored(&s, MINE);
    assert_eq!(text, "second");
    assert!(edited.is_some(), "the (edited) marker shows at once");

    // A stale history copy of the same revision must not undo the confirmed edit.
    s.with_conn(|c| {
        messages::upsert_messages(
            c,
            "C1",
            &[row(
                MINE,
                "U1",
                "original words",
                json!({"ts": MINE, "user": "U1"}),
            )],
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(stored(&s, MINE).0, "second");

    // The authoritative echo with a newer revision wins, twice in a row.
    let echo = |text: &str| {
        let raw = json!({"ts": MINE, "user": "U1", "text": text,
            "edited": {"user": "U1", "ts": "1700000900.000000"}});
        let mut m: api::Message = serde_json::from_value(raw.clone()).unwrap();
        m.raw = raw;
        Event::MessageChanged {
            channel: "C1".into(),
            message: m,
        }
    };
    live::apply(&s, echo("third")).await.unwrap();
    live::apply(&s, echo("third")).await.unwrap();
    assert_eq!(
        stored(&s, MINE),
        ("third".into(), Some("1700000900.000000".into()), false)
    );
    assert_eq!(fts_hits(&s, "second"), 0);
}

#[tokio::test]
async fn mismatched_response_identity_never_redirects_a_local_write() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.update"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"ok": true, "channel": "C1", "ts": THEIRS, "text": "hijack"}),
            ),
        )
        .mount(&server)
        .await;
    let s = store();
    let dir = tempfile::tempdir().unwrap();
    let p = edit_of(&s, "general", Some(MINE), "mine now").unwrap();
    let receipt = run(&p, &s, &server, &dir).await.unwrap();
    // Remote acceptance is success; local state is left for sync.
    assert!(receipt.not_stored_locally);
    assert!(receipt.message.is_none());
    assert_eq!(stored(&s, MINE).0, "original words");
    assert_eq!(stored(&s, THEIRS).0, "not mine");
}

#[tokio::test]
async fn delete_tombstones_locally_and_the_echo_is_idempotent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.delete"))
        .and(field("channel", "C1"))
        .and(field("ts", MINE))
        .and(has_field("token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "channel": "C1", "ts": MINE})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    let dir = tempfile::tempdir().unwrap();
    let p = delete_of(&s, "general", Some(MINE)).unwrap();
    let receipt = run(&p, &s, &server, &dir).await.unwrap();
    assert_eq!(
        (receipt.action.as_str(), receipt.ts.as_deref()),
        ("delete", Some(MINE))
    );
    assert!(!receipt.not_stored_locally);
    assert!(stored(&s, MINE).2);
    // RTM echo of the same deletion changes nothing and fails nothing.
    live::apply(
        &s,
        Event::MessageDeleted {
            channel: "C1".into(),
            ts: MINE.into(),
        },
    )
    .await
    .unwrap();
    assert!(stored(&s, MINE).2);
    // A late edit event cannot bring the message back.
    let raw = json!({"ts": MINE, "user": "U1", "text": "zombie",
        "edited": {"user": "U1", "ts": "1700000999.000000"}});
    let mut m: api::Message = serde_json::from_value(raw.clone()).unwrap();
    m.raw = raw;
    live::apply(
        &s,
        Event::MessageChanged {
            channel: "C1".into(),
            message: m,
        },
    )
    .await
    .unwrap();
    assert!(stored(&s, MINE).2);
    // The deleted message can no longer be targeted.
    assert!(delete_of(&s, "general", Some(MINE)).is_err());
}

#[tokio::test]
async fn slack_refusals_map_to_typed_errors_and_leave_the_database_alone() {
    let server = MockServer::start().await;
    for (endpoint, code) in [
        ("chat.update", "edit_window_closed"),
        ("chat.delete", "cant_delete_message"),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/{endpoint}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"ok": false, "error": code})),
            )
            .mount(&server)
            .await;
    }
    let s = store();
    let dir = tempfile::tempdir().unwrap();
    for p in [
        edit_of(&s, "general", Some(MINE), "late").unwrap(),
        delete_of(&s, "general", Some(MINE)).unwrap(),
    ] {
        let err = run(&p, &s, &server, &dir).await.err().unwrap();
        assert_eq!(err.kind().exit_code(), 8, "{err}");
    }
    assert_eq!(stored(&s, MINE), ("original words".into(), None, false));
    let outcomes: Vec<_> = ActionLog::new(dir.path().join("log.db"))
        .entries(5)
        .unwrap()
        .into_iter()
        .map(|e| e.outcome)
        .collect();
    assert_eq!(outcomes, ["failed:8", "failed:8"]);
    // Other documented codes.
    assert_eq!(
        Error::Slack {
            code: "message_not_found".into(),
            needed: None
        }
        .kind()
        .exit_code(),
        3
    );
    assert_eq!(
        Error::Slack {
            code: "msg_too_long".into(),
            needed: None
        }
        .kind()
        .exit_code(),
        2
    );
}

#[test]
fn last_own_message_comes_from_the_database_by_scope() {
    let s = store();
    s.with_conn(|c| {
        messages::upsert_messages(
            c,
            "C1",
            &[
                MessageRow {
                    thread_ts: Some("1700000000.000100".into()),
                    ..row("1700000001.000000", "U1", "my reply", json!({}))
                },
                MessageRow {
                    thread_ts: Some("1700000000.000100".into()),
                    ..row("1700000002.000000", "U2", "their reply", json!({}))
                },
            ],
        )?;
        Ok(())
    })
    .unwrap();
    let last = |thread: Option<&str>| {
        s.with_read(|c| messages::last_own_message(c, "T1", "U1", "C1", thread))
            .unwrap()
    };
    // Timeline scope skips thread replies, bot posts and tombstones.
    assert_eq!(last(None).as_deref(), Some(MINE));
    assert_eq!(last(Some(MINE)).as_deref(), Some("1700000001.000000"));
    assert_eq!(last(Some(THEIRS)), None);
    assert!(s
        .with_read(|c| messages::last_own_message(c, "T1", "U2", "C1", None))
        .unwrap()
        .is_some());
}
