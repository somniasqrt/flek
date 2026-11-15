//! M8.5 channel/status mutations behind the M8.7 write guard: policy denial before
//! any session, dry-run reports, exact request fields and confirmed DB updates.
//! Fake Slack (wiremock) and temporary SQLite only; no Keychain, no live calls.
mod support;
use flek_core::{
    agent::{ActionDescriptor, ActionLog, AgentConfig, AllowRule},
    api::{Client, CustomStatus},
    store::Store,
    sync::{
        live,
        management::{self, Operation, Outcome},
    },
    views,
    writes::WriteSession,
    Error, Secret,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use support::field;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.with_conn(|c| {
        c.execute_batch(
            "INSERT INTO teams(id,name,self_user_id) VALUES ('T1','Fake','U1');
             INSERT INTO users (id, team_id, name, real_name, display_name, deleted) VALUES
               ('U1','T1','me','Me Myself','me',0),
               ('U2','T1','alice','Alice A','alice',0),
               ('U3','T1','bob','Bob B','bob',0),
               ('U4','T1','gone','Gone G','gone',1);
             INSERT INTO conversations (id, team_id, kind, name, label, is_member, raw_json) VALUES
               ('C1','T1','public_channel','general','general',1,'{\"is_general\":true}'),
               ('C2','T1','public_channel','random','random',1,'{\"topic\":{\"value\":\"old\"}}'),
               ('C4','T1','public_channel','lounge','lounge',0,'{}'),
               ('G1','T1','private_channel','secret','secret',1,'{}'),
               ('D1','T1','im','','alice',1,'{}');
             UPDATE conversations SET topic = 'old' WHERE id = 'C2';
             INSERT INTO messages (conversation_id, ts, user_id, text, raw_json)
               VALUES ('C2','1700000000.000100','U2','history','{}');
             INSERT INTO conversation_members(conversation_id, user_id) VALUES ('C2','U1'),('C2','U3');",
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

fn prepare(s: &Store, op: Operation) -> management::Prepared {
    s.with_read(|c| management::prepare(c, "T1", op)).unwrap()
}

fn prepare_err(s: &Store, op: Operation) -> Error {
    s.with_read(|c| management::prepare(c, "T1", op))
        .err()
        .expect("preparation must fail")
}

fn rule(channels: &[&str], actions: &[&str]) -> AllowRule {
    AllowRule {
        team: "T1".into(),
        channels: channels.iter().map(|s| s.to_string()).collect(),
        actions: actions.iter().map(|s| s.to_string()).collect(),
    }
}

fn descriptor(action: &str, target: &str) -> ActionDescriptor {
    ActionDescriptor {
        action: action.into(),
        team_id: "T1".into(),
        targets: vec![target.into()],
    }
}

fn status(text: &str, emoji: &str, expiration: i64) -> CustomStatus {
    CustomStatus {
        status_text: text.into(),
        status_emoji: emoji.into(),
        status_expiration: expiration,
    }
}

fn ch(c: &str) -> String {
    c.to_owned()
}

/// Every M8.5 operation with its expected guard action, target, method and form.
fn all_operations() -> Vec<(Operation, &'static str, String, &'static str, Value)> {
    let set = status("Focus", ":books:", 4_000_000_000);
    let set_profile = serde_json::to_string(&set).unwrap();
    let clear_profile = serde_json::to_string(&status("", "", 0)).unwrap();
    vec![
        (
            Operation::Create {
                name: "Ops Room".into(),
                private: false,
            },
            "channel-create",
            "new:ops-room".into(),
            "conversations.create",
            json!({"name":"ops-room","is_private":"false"}),
        ),
        (
            Operation::Create {
                name: "ops_room".into(),
                private: true,
            },
            "channel-create",
            "new:ops_room".into(),
            "conversations.create",
            json!({"name":"ops_room","is_private":"true"}),
        ),
        (
            Operation::Invite {
                channel: "random".into(),
                person: "alice".into(),
            },
            "channel-invite",
            ch("C2"),
            "conversations.invite",
            json!({"channel":"C2","users":"U2"}),
        ),
        (
            Operation::Kick {
                channel: "#random".into(),
                person: "bob".into(),
            },
            "channel-kick",
            ch("C2"),
            "conversations.kick",
            json!({"channel":"C2","user":"U3"}),
        ),
        (
            Operation::Join {
                channel: "lounge".into(),
            },
            "channel-join",
            ch("C4"),
            "conversations.join",
            json!({"channel":"C4"}),
        ),
        (
            Operation::Leave {
                channel: "secret".into(),
            },
            "channel-leave",
            ch("G1"),
            "conversations.leave",
            json!({"channel":"G1"}),
        ),
        (
            Operation::Topic {
                channel: "random".into(),
                text: "new topic".into(),
            },
            "channel-topic",
            ch("C2"),
            "conversations.setTopic",
            json!({"channel":"C2","topic":"new topic"}),
        ),
        (
            Operation::Purpose {
                channel: "random".into(),
                text: "".into(),
            },
            "channel-purpose",
            ch("C2"),
            "conversations.setPurpose",
            json!({"channel":"C2","purpose":""}),
        ),
        (
            Operation::Rename {
                channel: "random".into(),
                name: "Off Topic".into(),
            },
            "channel-rename",
            ch("C2"),
            "conversations.rename",
            json!({"channel":"C2","name":"off-topic"}),
        ),
        (
            Operation::Archive {
                channel: "random".into(),
            },
            "channel-archive",
            ch("C2"),
            "conversations.archive",
            json!({"channel":"C2"}),
        ),
        (
            Operation::SetStatus(set),
            "status-set",
            ch("U1"),
            "users.profile.set",
            json!({"profile":set_profile}),
        ),
        (
            Operation::ClearStatus,
            "status-clear",
            ch("U1"),
            "users.profile.set",
            json!({"profile":clear_profile}),
        ),
    ]
}

async fn mount(server: &MockServer, rpc: &str, fields: Vec<support::FormField>, body: Value) {
    let mut mock = Mock::given(method("POST")).and(path(format!("/{rpc}")));
    for f in fields {
        mock = mock.and(f);
    }
    mock.respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}

async fn run(
    s: &Store,
    server: &MockServer,
    log: &ActionLog,
    op: Operation,
) -> flek_core::Result<Outcome> {
    let plan = prepare(s, op);
    plan.run_guarded(
        log,
        &AgentConfig::default(),
        true,
        s,
        &live::bus(),
        || async { Ok(session(server)) },
    )
    .await
}

fn log_in(dir: &tempfile::TempDir) -> ActionLog {
    ActionLog::new(dir.path().join("log.db"))
}

fn conversation(s: &Store, id: &str) -> views::Conversation {
    s.with_read(|c| views::conversation(c, id))
        .unwrap()
        .unwrap()
}

fn members(s: &Store, id: &str) -> Vec<String> {
    s.with_read(|c| {
        let mut stmt = c.prepare(
            "SELECT user_id FROM conversation_members WHERE conversation_id = ?1 ORDER BY 1",
        )?;
        let rows = stmt
            .query_map([id], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(rows)
    })
    .unwrap()
}

fn channel_json(id: &str, name: &str) -> Value {
    json!({
        "id": id, "name": name, "is_channel": true, "is_private": false,
        "is_archived": false, "created": 1_700_000_000, "creator": "U1",
        "num_members": 1, "topic": {"value": ""}, "purpose": {"value": ""}
    })
}

// ---------------------------------------------------------------- policy

#[tokio::test]
async fn every_m85_action_is_denied_before_any_session_and_leaves_no_log() {
    let s = store();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);
    for (op, name, ..) in all_operations() {
        let session_requested = AtomicBool::new(false);
        let plan = prepare(&s, op);
        let err = plan
            .run_guarded(
                &log,
                &AgentConfig::default(),
                false,
                &s,
                &live::bus(),
                || async {
                    session_requested.store(true, Ordering::SeqCst);
                    Ok(session(&server))
                },
            )
            .await
            .err()
            .unwrap_or_else(|| panic!("{name} must be denied"));
        assert!(matches!(err, Error::Forbidden), "{name}");
        assert_eq!(err.kind().exit_code(), 8, "{name}");
        assert!(!session_requested.load(Ordering::SeqCst), "{name}");
    }
    assert!(!dir.path().join("log.db").exists());
    // Reads of the log never create it either.
    assert!(log.entries(10).unwrap().is_empty());
}

#[tokio::test]
async fn allow_rules_authorize_exact_team_action_and_target_only() {
    let s = store();
    let cfg = AgentConfig {
        allow: vec![
            rule(&["C2"], &["channel-topic", "channel-invite"]),
            rule(&["U1"], &["status-set"]),
            rule(&["new:ops-room"], &["channel-create"]),
            rule(&["*"], &["channel-join"]),
        ],
        ..Default::default()
    };
    let authorized = |op: Operation| {
        prepare(&s, op).dry_run_report(&cfg, false)["authorized"]
            .as_bool()
            .unwrap()
    };
    let topic = |c: &str| Operation::Topic {
        channel: c.into(),
        text: "t".into(),
    };
    assert!(authorized(topic("random")));
    assert!(!authorized(topic("secret")));
    assert!(!authorized(Operation::Purpose {
        channel: "random".into(),
        text: "p".into()
    }));
    assert!(authorized(Operation::Invite {
        channel: "random".into(),
        person: "alice".into()
    }));
    assert!(!authorized(Operation::Kick {
        channel: "random".into(),
        person: "bob".into()
    }));
    assert!(authorized(Operation::Join {
        channel: "lounge".into()
    }));
    assert!(!authorized(Operation::Leave {
        channel: "secret".into()
    }));
    assert!(authorized(Operation::SetStatus(status("x", "", 0))));
    assert!(!authorized(Operation::ClearStatus));
    assert!(authorized(Operation::Create {
        name: "Ops Room".into(),
        private: false
    }));
    assert!(!authorized(Operation::Create {
        name: "other".into(),
        private: false
    }));
    // A rule of another team never applies.
    let other_team = AgentConfig {
        allow: vec![AllowRule {
            team: "T2".into(),
            channels: vec!["*".into()],
            actions: vec!["*".into()],
        }],
        ..Default::default()
    };
    for (op, ..) in all_operations() {
        assert!(
            !prepare(&s, op).dry_run_report(&other_team, false)["authorized"]
                .as_bool()
                .unwrap()
        );
    }
}

#[test]
fn descriptor_targets_are_validated_per_action_even_with_yes() {
    let cfg = AgentConfig::default();
    let good = [
        ("channel-create", "new:ops-room"),
        ("channel-create", "new:ops_room"),
        ("channel-create", "new:Новий-канал"),
        ("status-set", "U1"),
        ("status-clear", "W1"),
        ("channel-invite", "G1"),
        ("channel-archive", "C1"),
    ];
    for (action, target) in good {
        assert!(
            cfg.authorize(&descriptor(action, target), true).is_ok(),
            "{action} {target}"
        );
    }
    let bad = [
        ("channel-create", "C1"),
        ("channel-create", "new:"),
        ("channel-create", "new:bad name"),
        ("channel-create", "new:a/b"),
        ("status-set", "C1"),
        ("status-clear", "new:x"),
        ("channel-topic", "U1"),
        ("channel-kick", "new:x"),
        ("channel-invite", "c1"),
    ];
    for (action, target) in bad {
        assert!(
            cfg.authorize(&descriptor(action, target), true).is_err(),
            "{action} {target}"
        );
    }
    // No mass writes: exactly one target for every channel/status action.
    let mut two = descriptor("channel-invite", "C1");
    two.targets.push("C2".into());
    assert!(cfg.authorize(&two, true).is_err());
}

// ---------------------------------------------------------------- dry run

#[test]
fn dry_run_reports_use_the_m84_shape_for_every_action() {
    let s = store();
    let cfg = AgentConfig::default();
    for (op, name, target, method, params) in all_operations() {
        let plan = prepare(&s, op);
        let expected = json!({
            "dry_run": true, "authorized": false,
            "write": {
                "action": {"action": name, "team_id": "T1", "targets": [target]},
                "request": {"method": method, "params": params},
                "no_op": false, "warning": null
            }
        });
        assert_eq!(plan.dry_run_report(&cfg, false), expected, "{name}");
        let mut allowed = expected;
        allowed["authorized"] = json!(true);
        assert_eq!(
            plan.dry_run_report(&cfg, true),
            allowed,
            "{name} with --yes"
        );
        // Credentials are transport-owned and never part of a prepared form.
        let text = plan.dry_run_report(&cfg, true).to_string().to_lowercase();
        for needle in ["xox", "cookie", "token", "authorization"] {
            assert!(!text.contains(needle), "{name}: {needle}");
        }
    }
}

#[test]
fn dry_run_never_touches_the_database() {
    let s = store();
    let before: i64 = s
        .with_read(|c| Ok(c.query_row("SELECT total_changes()", [], |r| r.get(0))?))
        .unwrap();
    for (op, ..) in all_operations() {
        prepare(&s, op).dry_run_report(&AgentConfig::default(), true);
    }
    assert_eq!(conversation(&s, "C2").topic.as_deref(), Some("old"));
    assert_eq!(members(&s, "C2"), ["U1", "U3"]);
    let after: i64 = s
        .with_read(|c| Ok(c.query_row("SELECT total_changes()", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(before, after);
}

// ------------------------------------------------------------ resolution

#[test]
fn ambiguous_person_reports_candidates_and_never_guesses() {
    let s = store();
    // Prefix-only matches are not exact, even when a single person matches.
    let err = prepare_err(
        &s,
        Operation::Invite {
            channel: "random".into(),
            person: "ali".into(),
        },
    );
    match &err {
        Error::Ambiguous { candidates } => {
            assert!(candidates.iter().any(|c| c.id == "U2"), "{candidates:?}");
        }
        other => panic!("expected ambiguous, got {other:?}"),
    }
    assert_eq!(err.kind().exit_code(), 4);

    // Two people sharing a display name are genuinely ambiguous.
    s.with_conn(|c| {
        c.execute(
            "INSERT INTO users (id, team_id, name, real_name, display_name, deleted)
             VALUES ('U5','T1','alice2','Alice Two','alice',0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    match prepare_err(
        &s,
        Operation::Kick {
            channel: "random".into(),
            person: "alice".into(),
        },
    ) {
        Error::Ambiguous { candidates } => {
            let ids: Vec<_> = candidates.iter().map(|c| c.id.as_str()).collect();
            assert!(ids.contains(&"U2") && ids.contains(&"U5"), "{ids:?}");
        }
        other => panic!("expected ambiguous, got {other:?}"),
    }
}

#[test]
fn preparation_rejects_unsafe_or_unresolvable_requests_offline() {
    let s = store();
    let code = |op| prepare_err(&s, op).kind().exit_code();
    let invite = |channel: &str, person: &str| Operation::Invite {
        channel: channel.into(),
        person: person.into(),
    };
    assert_eq!(code(invite("nowhere", "alice")), 3);
    assert_eq!(code(invite("random", "nobody")), 3);
    // deleted user, DM as a channel
    assert_eq!(code(invite("random", "gone")), 2);
    assert_eq!(code(invite("D1", "alice")), 2);
    // self-kick points at leave
    assert_eq!(
        code(Operation::Kick {
            channel: "random".into(),
            person: "me".into()
        }),
        2
    );
    // known general channel cannot be left or archived
    assert_eq!(
        code(Operation::Leave {
            channel: "general".into()
        }),
        8
    );
    assert_eq!(
        code(Operation::Archive {
            channel: "general".into()
        }),
        8
    );
    for name in ["", "bad!", "has.dot"] {
        assert_eq!(
            code(Operation::Create {
                name: name.into(),
                private: false
            }),
            2,
            "{name:?}"
        );
        assert_eq!(
            code(Operation::Rename {
                channel: "random".into(),
                name: name.into()
            }),
            2,
            "{name:?}"
        );
    }
    assert_eq!(
        code(Operation::SetStatus(status(&"x".repeat(101), "", 0))),
        2
    );
    assert_eq!(code(Operation::SetStatus(status("ok", "calendar", 0))), 2);
    assert_eq!(code(Operation::SetStatus(status("ok", "", -1))), 2);
    // No stored own profile: asks to sync instead of guessing.
    s.with_conn(|c| {
        c.execute("DELETE FROM users WHERE id = 'U1'", [])?;
        Ok(())
    })
    .unwrap();
    assert_eq!(code(Operation::ClearStatus), 3);
}

#[test]
fn status_expiration_accepts_forward_times_only() {
    use chrono::TimeZone;
    let now = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
    let t = now.timestamp();
    let exp = |s: &str| management::status_expiration(s, now);
    assert_eq!(exp("0").unwrap(), 0);
    assert_eq!(exp("never").unwrap(), 0);
    assert_eq!(exp("30m").unwrap(), t + 1800);
    assert_eq!(exp("12h").unwrap(), t + 12 * 3600);
    assert_eq!(exp("7d").unwrap(), t + 7 * 86400);
    assert_eq!(exp(&(t + 60).to_string()).unwrap(), t + 60);
    for bad in [
        "",
        "-5",
        "yesterday-ish",
        "m",
        "1.5h",
        &(t - 1).to_string(),
        &t.to_string(),
        "99999999999999999999d",
        "9223372036854775807d",
    ] {
        assert_eq!(exp(bad).unwrap_err().kind().exit_code(), 2, "{bad:?}");
    }
}

// ------------------------------------------------------------- execution

#[tokio::test]
async fn invite_and_kick_send_one_user_and_update_membership_idempotently() {
    let s = store();
    let server = MockServer::start().await;
    mount(
        &server,
        "conversations.invite",
        vec![field("channel", "C2"), field("users", "U2")],
        json!({"ok":true,"channel":channel_json("C2","random")}),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);
    let invite = || Operation::Invite {
        channel: "random".into(),
        person: "alice".into(),
    };
    let out = run(&s, &server, &log, invite()).await.unwrap();
    assert_eq!(out.conversation_id.as_deref(), Some("C2"));
    assert_eq!(out.user_id.as_deref(), Some("U2"));
    assert!(!out.not_stored_locally && out.warning.is_none());
    assert_eq!(members(&s, "C2"), ["U1", "U2", "U3"]);
    // History is untouched.
    let n: i64 = s
        .with_read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id='C2'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(n, 1);

    server.reset().await;
    mount(
        &server,
        "conversations.invite",
        vec![field("channel", "C2"), field("users", "U2")],
        json!({"ok":true}),
    )
    .await;
    run(&s, &server, &log, invite()).await.unwrap();
    assert_eq!(members(&s, "C2"), ["U1", "U2", "U3"], "idempotent");

    server.reset().await;
    mount(
        &server,
        "conversations.kick",
        vec![field("channel", "C2"), field("user", "U3")],
        json!({"ok":true}),
    )
    .await;
    run(
        &s,
        &server,
        &log,
        Operation::Kick {
            channel: "random".into(),
            person: "bob".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(members(&s, "C2"), ["U1", "U2"]);
    // A membership delta never marks a partial member list as complete.
    let complete: i64 = s
        .with_read(|c| {
            Ok(
                c.query_row("SELECT COUNT(*) FROM conversation_details_sync", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(complete, 0);
    let entries = log.entries(10).unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|e| (e.action.as_str(), e.outcome.as_str()))
            .collect::<Vec<_>>(),
        [
            ("channel-kick", "accepted"),
            ("channel-invite", "accepted"),
            ("channel-invite", "accepted")
        ]
    );
    assert!(entries.iter().all(|e| e.targets == ["C2"]));
}

#[tokio::test]
async fn leave_clears_own_membership_but_keeps_history() {
    let s = store();
    let server = MockServer::start().await;
    mount(
        &server,
        "conversations.leave",
        vec![field("channel", "C2")],
        json!({"ok":true}),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    run(
        &s,
        &server,
        &log_in(&dir),
        Operation::Leave {
            channel: "random".into(),
        },
    )
    .await
    .unwrap();
    assert!(!conversation(&s, "C2").is_member);
    assert_eq!(members(&s, "C2"), ["U3"]);
    let n: i64 = s
        .with_read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id='C2'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn topic_purpose_rename_and_archive_update_columns_and_raw_metadata() {
    let s = store();
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);

    mount(
        &server,
        "conversations.setTopic",
        vec![field("channel", "C2"), field("topic", "Ship it")],
        json!({"ok":true,"channel":{"topic":{"value":"Ship it"}}}),
    )
    .await;
    run(
        &s,
        &server,
        &log,
        Operation::Topic {
            channel: "random".into(),
            text: "Ship it".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(conversation(&s, "C2").topic.as_deref(), Some("Ship it"));

    server.reset().await;
    mount(
        &server,
        "conversations.setPurpose",
        vec![field("channel", "C2"), field("purpose", "Chat")],
        json!({"ok":true,"purpose":"Chat"}),
    )
    .await;
    run(
        &s,
        &server,
        &log,
        Operation::Purpose {
            channel: "random".into(),
            text: "Chat".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(conversation(&s, "C2").purpose.as_deref(), Some("Chat"));

    server.reset().await;
    mount(
        &server,
        "conversations.rename",
        vec![field("channel", "C2"), field("name", "off-topic")],
        json!({"ok":true,"channel":{"id":"C2","name":"off-topic"}}),
    )
    .await;
    run(
        &s,
        &server,
        &log,
        Operation::Rename {
            channel: "random".into(),
            name: "Off Topic".into(),
        },
    )
    .await
    .unwrap();
    let renamed = conversation(&s, "C2");
    assert_eq!(renamed.name.as_deref(), Some("off-topic"));
    assert_eq!(renamed.label, "off-topic");

    server.reset().await;
    mount(
        &server,
        "conversations.archive",
        vec![field("channel", "C2")],
        json!({"ok":true}),
    )
    .await;
    run(
        &s,
        &server,
        &log,
        Operation::Archive {
            channel: "off-topic".into(),
        },
    )
    .await
    .unwrap();
    assert!(conversation(&s, "C2").is_archived);

    // Raw metadata stays consistent with the indexed columns for a later reparse.
    let raw: Value = s
        .with_read(|c| {
            let text: String = c.query_row(
                "SELECT raw_json FROM conversations WHERE id='C2'",
                [],
                |r| r.get(0),
            )?;
            Ok(serde_json::from_str(&text).unwrap())
        })
        .unwrap();
    assert_eq!(raw["topic"]["value"], "Ship it");
    assert_eq!(raw["purpose"]["value"], "Chat");
    assert_eq!(raw["name"], "off-topic");
    assert_eq!(raw["is_archived"], true);
    // History survives archiving.
    let n: i64 = s
        .with_read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id='C2'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(log.entries(10).unwrap().len(), 4);
}

#[tokio::test]
async fn empty_topic_clears_the_stored_value() {
    let s = store();
    let server = MockServer::start().await;
    mount(
        &server,
        "conversations.setTopic",
        vec![field("channel", "C2"), field("topic", "")],
        json!({"ok":true,"channel":{"topic":{"value":""}}}),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    run(
        &s,
        &server,
        &log_in(&dir),
        Operation::Topic {
            channel: "random".into(),
            text: "".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(conversation(&s, "C2").topic.as_deref().unwrap_or(""), "");
}

#[tokio::test]
async fn create_sends_the_normalized_name_and_stores_own_membership() {
    let s = store();
    let server = MockServer::start().await;
    mount(
        &server,
        "conversations.create",
        vec![field("name", "ops-room"), field("is_private", "true")],
        json!({"ok":true,"channel":{
            "id":"G9","name":"ops-room","is_channel":false,"is_group":true,"is_private":true,
            "is_archived":false,"created":1_700_000_000,"creator":"U1",
            "topic":{"value":""},"purpose":{"value":""}}}),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);
    let out = run(
        &s,
        &server,
        &log,
        Operation::Create {
            name: "Ops Room".into(),
            private: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(out.conversation_id.as_deref(), Some("G9"));
    assert!(!out.not_stored_locally);
    let c = conversation(&s, "G9");
    assert!(c.is_member && !c.is_archived);
    assert_eq!(c.kind, "private");
    assert_eq!(c.name.as_deref(), Some("ops-room"));
    assert_eq!(members(&s, "G9"), ["U1"]);
    let entries = log.entries(5).unwrap();
    assert_eq!(entries[0].action, "channel-create");
    assert_eq!(entries[0].targets, ["new:ops-room"]);
    assert_eq!(entries[0].outcome, "accepted");
}

#[tokio::test]
async fn join_stores_membership_and_survives_a_failed_catch_up() {
    let s = store();
    let server = MockServer::start().await;
    mount(
        &server,
        "conversations.join",
        vec![field("channel", "C4")],
        json!({"ok":true,"channel":{"id":"C4","name":"lounge","is_channel":true,
            "is_member":true,"is_archived":false,"topic":{"value":""},"purpose":{"value":""}}}),
    )
    .await;
    // The history catch-up fails: confirmed membership must be retained.
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        &s,
        &server,
        &log_in(&dir),
        Operation::Join {
            channel: "lounge".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(out.conversation_id.as_deref(), Some("C4"));
    assert_eq!(out.warning.as_deref(), Some("catch_up_failed"));
    assert!(!out.not_stored_locally);
    assert!(conversation(&s, "C4").is_member);
    assert_eq!(members(&s, "C4"), ["U1"]);
}

#[tokio::test]
async fn status_set_and_clear_send_the_profile_and_store_the_confirmed_one() {
    let s = store();
    let server = MockServer::start().await;
    let set = status("Focus", ":books:", 4_000_000_000);
    mount(
        &server,
        "users.profile.set",
        vec![field("profile", serde_json::to_string(&set).unwrap())],
        json!({"ok":true,"profile":{"status_text":"Focus","status_emoji":":books:",
            "status_expiration":4_000_000_000i64}}),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);
    let out = run(&s, &server, &log, Operation::SetStatus(set))
        .await
        .unwrap();
    assert_eq!(out.user_id.as_deref(), Some("U1"));
    assert!(out.conversation_id.is_none() && !out.not_stored_locally);
    let me = s.with_read(|c| views::user(c, "U1")).unwrap().unwrap();
    assert_eq!(me.status_text.as_deref(), Some("Focus"));
    assert_eq!(me.status_emoji.as_deref(), Some(":books:"));
    assert_eq!(me.status_expiration, Some(4_000_000_000));
    assert_eq!(me.real_name.as_deref(), Some("Me Myself"));

    server.reset().await;
    mount(
        &server,
        "users.profile.set",
        vec![field(
            "profile",
            serde_json::to_string(&status("", "", 0)).unwrap(),
        )],
        json!({"ok":true,"profile":{"status_text":"","status_emoji":"","status_expiration":0}}),
    )
    .await;
    run(&s, &server, &log, Operation::ClearStatus)
        .await
        .unwrap();
    let me = s.with_read(|c| views::user(c, "U1")).unwrap().unwrap();
    assert!(me.status_text.as_deref().unwrap_or("").is_empty());
    assert!(me.status_emoji.as_deref().unwrap_or("").is_empty());
    let entries = log.entries(5).unwrap();
    assert_eq!(entries[0].action, "status-clear");
    assert_eq!(entries[0].targets, ["U1"]);
    assert_eq!(entries[1].action, "status-set");
}

// ------------------------------------------------------------- failures

#[tokio::test]
async fn slack_rejections_are_typed_leave_the_db_alone_and_consume_a_slot() {
    let s = store();
    let server = MockServer::start().await;
    let cases = [
        ("name_taken", 2),
        ("not_in_channel", 8),
        ("channel_not_found", 3),
        ("user_is_restricted", 8),
    ];
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);
    for (code, exit) in cases {
        server.reset().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":code})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let err = run(
            &s,
            &server,
            &log,
            Operation::Rename {
                channel: "random".into(),
                name: "taken".into(),
            },
        )
        .await
        .expect_err("rejected");
        assert_eq!(err.kind().exit_code(), exit, "{code}");
        assert!(err.to_string().contains(code));
    }
    assert_eq!(conversation(&s, "C2").name.as_deref(), Some("random"));
    assert_eq!(conversation(&s, "C2").topic.as_deref(), Some("old"));
    let outcomes: Vec<_> = log
        .entries(10)
        .unwrap()
        .into_iter()
        .map(|e| e.outcome)
        .collect();
    assert_eq!(outcomes, ["failed:8", "failed:3", "failed:8", "failed:2"]);
}

#[tokio::test]
async fn rate_limit_applies_and_session_identity_must_match_the_plan() {
    let s = store();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);
    let op = || Operation::Archive {
        channel: "random".into(),
    };
    for bad in [("T9", "U1"), ("T1", "U9")] {
        let plan = prepare(&s, op());
        let err = plan
            .run_guarded(
                &log,
                &AgentConfig::default(),
                true,
                &s,
                &live::bus(),
                || async {
                    Ok(WriteSession {
                        client: client(&server),
                        team_id: bad.0.into(),
                        user_id: bad.1.into(),
                    })
                },
            )
            .await
            .err()
            .unwrap();
        assert!(matches!(err, Error::Forbidden), "{bad:?}");
    }
    assert!(!conversation(&s, "C2").is_archived);

    // The two attempts above already used two slots of the one-minute window.
    let cfg = AgentConfig {
        writes_per_minute: 2,
        ..Default::default()
    };
    let err = prepare(&s, op())
        .run_guarded(&log, &cfg, true, &s, &live::bus(), || async {
            Ok(session(&server))
        })
        .await
        .err()
        .unwrap();
    assert_eq!(err.kind().exit_code(), 6);
}
