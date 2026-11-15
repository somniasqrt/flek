//! Agent write guard: policy matrix, dry run, rate limit, audit redaction and
//! prepared writes against a mock Slack. Never touches real Slack or the Keychain.
mod support;
use flek_core::{
    agent::{ActionDescriptor, ActionLog, AgentConfig, AllowRule},
    api::Client,
    store::{
        directory,
        messages::{self, MessageRow},
        Store,
    },
    writes::{MessageAction, MessageInput, PreparedWrite, WriteSession},
    Error, Secret,
};
use serde_json::{json, Value};
use support::{field, has_field};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

const TS: &str = "1700000000.000100";

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.with_conn(|c| {
        directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U1")?;
        c.execute_batch(
            "INSERT INTO users (id, team_id, name, real_name, display_name, deleted) VALUES
               ('U1','T1','me','Me Myself','me',0),
               ('U2','T1','alice','Alice A','alice',0),
               ('U3','T1','bob','Bob B','bob',0),
               ('U4','T1','gone','Gone G','gone',1);
             INSERT INTO conversations (id, team_id, kind, name, label) VALUES
               ('C1','T1','public_channel','general','general'),
               ('C2','T1','public_channel','random','random'),
               ('C3','T1','public_channel','general-help','general-help');",
        )?;
        messages::upsert_messages(
            c,
            "C1",
            &[MessageRow {
                ts: TS.into(),
                text: "root".into(),
                raw_json: "{}".into(),
                ..Default::default()
            }],
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

fn config() -> AgentConfig {
    AgentConfig::default()
}

fn rule(team: &str, channels: &[&str], actions: &[&str]) -> AllowRule {
    AllowRule {
        team: team.into(),
        channels: channels.iter().map(|s| s.to_string()).collect(),
        actions: actions.iter().map(|s| s.to_string()).collect(),
    }
}

fn action(name: &str, team: &str, targets: &[&str]) -> ActionDescriptor {
    ActionDescriptor {
        action: name.into(),
        team_id: team.into(),
        targets: targets.iter().map(|s| s.to_string()).collect(),
    }
}

fn text_input(text: &str) -> MessageInput {
    MessageInput {
        text: text.into(),
        ..Default::default()
    }
}

fn prepare(s: &Store, channel: &str, input: MessageInput, cfg: &AgentConfig) -> PreparedWrite {
    s.with_read(|c| PreparedWrite::message(c, "T1", channel, input, cfg))
        .unwrap()
}

fn prepare_err(s: &Store, channel: &str, input: MessageInput) -> Error {
    s.with_read(|c| PreparedWrite::message(c, "T1", channel, input, &config()))
        .err()
        .expect("preparation must fail")
}

fn kind_of(e: &Error) -> i32 {
    e.kind().exit_code()
}

fn session(server: &MockServer) -> WriteSession {
    WriteSession {
        client: client(server),
        team_id: "T1".into(),
        user_id: "U1".into(),
    }
}

// ---------------------------------------------------------------- policy

#[test]
fn policy_denies_by_default_and_yes_allows_any_valid_action() {
    let cfg = config();
    let a = action("send", "T1", &["C1"]);
    assert!(matches!(cfg.authorize(&a, false), Err(Error::Forbidden)));
    assert!(cfg.authorize(&a, true).is_ok());
    // --yes never makes an invalid or mass descriptor valid.
    for bad in [
        action("send", "T1", &[]),
        action("send", "T1", &["C1", "C2"]),
        action("send", "bad", &["C1"]),
        action("send", "T1", &["lowercase"]),
        action("Send", "T1", &["C1"]),
        action("", "T1", &["C1"]),
        action("dm", "T1", &["C1"]),
        action(
            "dm",
            "T1",
            &["U1", "U2", "U3", "U4", "U5", "U6", "U7", "U8", "U9"],
        ),
    ] {
        assert!(cfg.authorize(&bad, true).is_err(), "{:?}", bad.action);
    }
    assert!(cfg
        .authorize(&action("dm", "T1", &["U2", "U3"]), true)
        .is_ok());
}

#[test]
fn policy_matrix_for_allow_rules() {
    let mut cfg = config();
    cfg.allow = vec![
        rule("T1", &["C1"], &["send", "react"]),
        rule("T1", &["*"], &["pin"]),
        rule("T2", &["*"], &["*"]),
        rule("T1", &["U2", "U3"], &["dm"]),
    ];
    let allowed = |name: &str, team: &str, targets: &[&str]| {
        cfg.authorize(&action(name, team, targets), false).is_ok()
    };
    // exact team + channel + action
    assert!(allowed("send", "T1", &["C1"]));
    assert!(allowed("react", "T1", &["C1"]));
    // wrong channel, action, or team
    assert!(!allowed("send", "T1", &["C2"]));
    assert!(!allowed("unreact", "T1", &["C1"]));
    assert!(!allowed("schedule", "T1", &["C1"]));
    assert!(!allowed("send", "T3", &["C1"]));
    // wildcard channel for one action only
    assert!(allowed("pin", "T1", &["C2"]));
    assert!(!allowed("unpin", "T1", &["C2"]));
    // wildcard action and channel but only for its own team
    assert!(allowed("save", "T2", &["C9"]));
    assert!(!allowed("save", "T1", &["C9"]));
    // a rule never combines pieces of different rules
    assert!(!allowed("react", "T1", &["C2"]));
    // dm needs every recipient listed
    assert!(allowed("dm", "T1", &["U2"]));
    assert!(allowed("dm", "T1", &["U2", "U3"]));
    assert!(!allowed("dm", "T1", &["U2", "U7"]));
    // an invalid descriptor is rejected even by a wildcard rule
    assert!(!allowed("send", "T2", &[]));
    assert!(!allowed("send", "T2", &["C1", "C2"]));
    // channel-name globs are not wildcards: only the literal "*" is
    let mut glob = config();
    glob.allow = vec![rule("T1", &["C*"], &["send"])];
    assert!(glob
        .authorize(&action("send", "T1", &["C1"]), false)
        .is_err());
}

#[test]
fn config_loading_defaults_parsing_and_validation() {
    let dir = tempfile::tempdir().unwrap();
    // Missing file: safe defaults (deny without --yes, 10 writes/minute, no signature).
    let missing = AgentConfig::load(&dir.path().join("none.toml")).unwrap();
    assert_eq!(missing.writes_per_minute, 10);
    assert!(missing.signature.is_none() && missing.allow.is_empty());

    let file = dir.path().join("agent.toml");
    std::fs::write(
        &file,
        r#"
writes_per_minute = 3
signature = "via agent"

[[allow]]
team = "T1"
channels = ["C1", "C2"]
actions = ["send", "react"]
"#,
    )
    .unwrap();
    let cfg = AgentConfig::load(&file).unwrap();
    assert_eq!(cfg.writes_per_minute, 3);
    assert_eq!(cfg.signature.as_deref(), Some("via agent"));
    assert!(cfg
        .authorize(&action("react", "T1", &["C2"]), false)
        .is_ok());

    for (body, why) in [
        ("writes_per_minute = 0", "zero rate"),
        ("writes_per_minute = 61", "rate above 60"),
        ("signature = \"  \"", "blank signature"),
        (
            "signature = \"xoxc-leak\"",
            "signature that looks like a token",
        ),
        ("unknown = 1", "unknown key"),
        ("[[allow]]\nteam = \"T1\"", "rule without channels/actions"),
        (
            "[[allow]]\nteam=\"T1\"\nchannels=[]\nactions=[]\nextra=1",
            "unknown rule key",
        ),
    ] {
        std::fs::write(&file, body).unwrap();
        let err = AgentConfig::load(&file)
            .err()
            .unwrap_or_else(|| panic!("{why} must fail"));
        assert_eq!(kind_of(&err), 2, "{why}");
    }
    // Parser diagnostics must not quote file content.
    std::fs::write(&file, "signature = \"super-secret-sig\nbroken").unwrap();
    let msg = AgentConfig::load(&file).err().unwrap().to_string();
    assert!(!msg.contains("super-secret-sig"), "{msg}");
}

// --------------------------------------------------------------- dry run

#[tokio::test]
async fn preparing_and_dry_run_send_nothing() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
        .expect(0)
        .mount(&server)
        .await;
    let s = store();
    let mut cfg = config();
    cfg.signature = Some("via agent".into());
    let p = prepare(&s, "#general", text_input("hi @alice"), &cfg);
    let report = p.dry_run_report(&cfg, false);
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["authorized"], false);
    assert_eq!(report["write"]["request"]["method"], "chat.postMessage");
    let params = &report["write"]["request"]["params"];
    assert_eq!(params["channel"], "C1");
    assert_eq!(params["text"], "hi <@U2>\n\nvia agent");
    assert!(params["client_msg_id"].is_string());
    assert_eq!(p.dry_run_report(&cfg, true)["authorized"], true);
    // The report never carries credentials of any shape.
    let text = report.to_string();
    for needle in ["xox", "token", "cookie", "authorization"] {
        assert!(!text.to_lowercase().contains(needle), "{needle}");
    }
    // Every other prepared kind also prepares offline.
    for a in [
        MessageAction::React,
        MessageAction::Unreact,
        MessageAction::MarkRead,
        MessageAction::Pin,
        MessageAction::Unpin,
        MessageAction::Save,
    ] {
        s.with_read(|c| {
            PreparedWrite::message_action(c, "T1", a, "general", Some(TS), Some("tada"))
        })
        .unwrap();
    }
    s.with_read(|c| PreparedWrite::dm(c, "T1", &["alice".into()]))
        .unwrap();
    // Dry run leaves the action log untouched; the mock saw no request (verified on drop).
    let dir = tempfile::tempdir().unwrap();
    let log = ActionLog::new(dir.path().join("log.db"));
    let _ = p.dry_run_report(&cfg, true);
    assert!(log.entries(10).unwrap().is_empty());
    assert!(!dir.path().join("log.db").exists());
}

#[tokio::test]
async fn denied_and_rate_limited_writes_never_resolve_a_session_or_call_slack() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
        .expect(0)
        .mount(&server)
        .await;
    let s = store();
    let dir = tempfile::tempdir().unwrap();
    let log = ActionLog::new(dir.path().join("log.db"));
    let mut cfg = config();
    let p = prepare(&s, "general", text_input("hi"), &cfg);
    let touched = std::sync::atomic::AtomicBool::new(false);
    let denied = p
        .run_guarded(&log, &cfg, false, &s, || async {
            touched.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(session(&server))
        })
        .await
        .err()
        .unwrap();
    assert!(matches!(denied, Error::Forbidden));
    assert!(!touched.load(std::sync::atomic::Ordering::SeqCst));
    // A denial is not an executed write: it consumes no rate-limit slot, nothing is logged.
    assert!(log.entries(10).unwrap().is_empty());

    cfg.writes_per_minute = 1;
    log.reserve(&cfg, &p.action, true).unwrap();
    let limited = p
        .run_guarded(&log, &cfg, true, &s, || async {
            touched.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(session(&server))
        })
        .await
        .err()
        .unwrap();
    assert_eq!(kind_of(&limited), 6, "{limited}");
    assert!(!touched.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(log.entries(10).unwrap().len(), 1);
}

#[tokio::test]
async fn session_of_another_team_is_forbidden_before_any_request() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
        .expect(0)
        .mount(&server)
        .await;
    let s = store();
    let dir = tempfile::tempdir().unwrap();
    let log = ActionLog::new(dir.path().join("log.db"));
    let p = prepare(&s, "general", text_input("hi"), &config());
    let err = p
        .run_guarded(&log, &config(), true, &s, || async {
            Ok(WriteSession {
                team_id: "T9".into(),
                ..session(&server)
            })
        })
        .await
        .err()
        .unwrap();
    assert!(matches!(err, Error::Forbidden));
    assert_eq!(log.entries(1).unwrap()[0].outcome, "failed:8");
}

// ------------------------------------------------------------ rate limit

#[test]
fn rate_limit_is_shared_by_independent_connections() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.db");
    let mut cfg = config();
    cfg.writes_per_minute = 3;
    let a = action("react", "T1", &["C1"]);
    // Eight threads, each with its own ActionLog (own SQLite connection per call),
    // race for three slots: exactly three may win.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (path, cfg, a, barrier) = (path.clone(), cfg.clone(), a.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                ActionLog::new(path).reserve(&cfg, &a, true).map(|_| ())
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 3);
    for r in results.iter().filter_map(|r| r.as_ref().err()) {
        assert_eq!(kind_of(r), 6, "{r}");
    }
    assert_eq!(ActionLog::new(&path).entries(100).unwrap().len(), 3);
    // The limit is global to the log, not per action, team or channel.
    let other = action("pin", "T2", &["C9"]);
    assert_eq!(
        kind_of(
            &ActionLog::new(&path)
                .reserve(&cfg, &other, true)
                .err()
                .unwrap()
        ),
        6
    );
}

#[test]
fn rate_limit_window_slides_and_config_changes_apply() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.db");
    let log = ActionLog::new(&path);
    let mut cfg = config();
    cfg.writes_per_minute = 2;
    let a = action("react", "T1", &["C1"]);
    log.reserve(&cfg, &a, true).unwrap();
    log.reserve(&cfg, &a, true).unwrap();
    assert!(log.reserve(&cfg, &a, true).is_err());
    // Age one entry beyond the window: a slot frees up.
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute(
        "UPDATE actions SET started_at = started_at - 61 WHERE id = (SELECT MIN(id) FROM actions)",
        [],
    )
    .unwrap();
    log.reserve(&cfg, &a, true).unwrap();
    assert!(log.reserve(&cfg, &a, true).is_err());
    // Entries with future timestamps (clock rollback) still count.
    c.execute("UPDATE actions SET started_at = started_at + 7200", [])
        .unwrap();
    assert!(log.reserve(&cfg, &a, true).is_err());
    // A policy check precedes the limit and does not consume a slot.
    cfg.writes_per_minute = 60;
    let before = log.entries(100).unwrap().len();
    assert!(matches!(
        log.reserve(&cfg, &a, false).err().unwrap(),
        Error::Forbidden
    ));
    assert_eq!(log.entries(100).unwrap().len(), before);
}

// ------------------------------------------------------ mentions/prepare

#[test]
fn mentions_convert_only_exact_explicit_handles() {
    let s = store();
    let m = |t: &str| s.with_read(|c| flek_core::writes::mention_text(c, "T1", t));
    assert_eq!(m("hi @alice, ok?").unwrap(), "hi <@U2>, ok?");
    assert_eq!(m("(@bob) and @alice.").unwrap(), "(<@U3>) and <@U2>.");
    // Slack markup typed by the user is escaped, never interpreted.
    assert_eq!(m("<!channel> a & b").unwrap(), "&lt;!channel&gt; a &amp; b");
    assert_eq!(m("<@U3> x").unwrap(), "&lt;@U3&gt; x");
    // Not mentions: emails, mid-word at-signs, code spans, a lone at-sign.
    assert_eq!(m("mail me@alice.com").unwrap(), "mail me@alice.com");
    assert_eq!(m("`@alice` code").unwrap(), "`@alice` code");
    assert_eq!(m("at 5 @ home").unwrap(), "at 5 @ home");
    // Unknown, prefix-only, group and deleted handles are refused, not guessed.
    for bad in ["@nobody", "@ali", "@channel", "@here", "@gone"] {
        assert!(m(bad).is_err(), "{bad}");
    }
}

#[test]
fn message_preparation_validations() {
    let s = store();
    // Exact channel only: a prefix or fuzzy name never resolves to a write target.
    // Prefix matches are only suggestions (ambiguous), never a resolution.
    assert_eq!(kind_of(&prepare_err(&s, "gen", text_input("x"))), 4);
    assert_eq!(kind_of(&prepare_err(&s, "nope", text_input("x"))), 3);
    assert_eq!(
        prepare(&s, "#general", text_input("x"), &config())
            .action
            .targets,
        vec!["C1"]
    );
    assert_eq!(
        prepare(&s, "general-help", text_input("x"), &config())
            .action
            .targets,
        vec!["C3"]
    );
    for input in [
        text_input("   "),
        MessageInput {
            thread_ts: Some("bad".into()),
            ..text_input("x")
        },
        MessageInput {
            broadcast: true,
            ..text_input("x")
        },
        MessageInput {
            blocks: Some(vec![json!({"type":"divider"}); 51]),
            ..text_input("x")
        },
        MessageInput {
            blocks: Some(vec![json!("notobject")]),
            ..text_input("x")
        },
        MessageInput {
            post_at: Some(chrono::Utc::now().timestamp() - 5),
            ..text_input("x")
        },
        MessageInput {
            post_at: Some(chrono::Utc::now().timestamp() + 121 * 86400),
            ..text_input("x")
        },
    ] {
        assert_eq!(kind_of(&prepare_err(&s, "general", input)), 2);
    }
    // Blocks without text are valid; the optional signature adds a context block.
    let mut cfg = config();
    cfg.signature = Some("via agent".into());
    let p = prepare(
        &s,
        "general",
        MessageInput {
            blocks: Some(vec![json!({"type":"divider"})]),
            thread_ts: Some(TS.into()),
            broadcast: true,
            ..Default::default()
        },
        &cfg,
    );
    let params = &p.request.params;
    assert_eq!(params["thread_ts"], TS);
    assert_eq!(params["reply_broadcast"], "true");
    let blocks: Vec<Value> = serde_json::from_str(&params["blocks"]).unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[1]["elements"][0]["text"], "via agent");
    // Scheduled messages use their own action name and method, and no client_msg_id.
    let at = chrono::Utc::now().timestamp() + 3600;
    let p = prepare(
        &s,
        "general",
        MessageInput {
            post_at: Some(at),
            ..text_input("later")
        },
        &config(),
    );
    assert_eq!(p.action.action, "schedule");
    assert_eq!(p.request.method, "chat.scheduleMessage");
    assert_eq!(p.request.params["post_at"], at.to_string());
    assert!(!p.request.params.contains_key("client_msg_id"));
}

#[test]
fn signature_is_off_by_default_and_never_resolves_mentions() {
    let s = store();
    let plain = prepare(&s, "general", text_input("hello"), &config());
    assert_eq!(plain.request.params["text"], "hello");
    let mut cfg = config();
    cfg.signature = Some("sent via <agent> & co".into());
    let signed = prepare(&s, "general", text_input("hello"), &cfg);
    assert_eq!(
        signed.request.params["text"],
        "hello\n\nsent via &lt;agent&gt; &amp; co"
    );
}

#[test]
fn message_action_references() {
    let s = store();
    let make = |a, reference: &str, ts: Option<&str>, emoji: Option<&str>| {
        s.with_read(|c| PreparedWrite::message_action(c, "T1", a, reference, ts, emoji))
    };
    let p = make(MessageAction::React, "#general", Some(TS), Some(":tada:")).unwrap();
    assert_eq!(p.request.method, "reactions.add");
    assert_eq!(p.request.params["name"], "tada");
    assert_eq!(p.request.params["timestamp"], TS);
    assert_eq!(p.action.action, "react");
    let permalink = "https://fake.slack.com/archives/C1/p1700000000000100";
    let p = make(MessageAction::Pin, permalink, None, None).unwrap();
    assert_eq!(p.request.method, "pins.add");
    assert_eq!(p.action.targets, vec!["C1"]);
    assert_eq!(
        make(MessageAction::Unpin, "general", Some(TS), None)
            .unwrap()
            .request
            .method,
        "pins.remove"
    );
    let save = make(MessageAction::Save, "general", Some(TS), None).unwrap();
    assert_eq!(save.request.method, "stars.add");
    assert_eq!(save.warning, Some("legacy_star_only_later_api_unavailable"));
    let mark = make(MessageAction::MarkRead, "general", Some(TS), None).unwrap();
    assert_eq!(mark.request.method, "conversations.mark");
    assert_eq!(mark.request.params["ts"], TS);
    // Invalid input.
    assert_eq!(
        kind_of(
            &make(MessageAction::React, "general", Some(TS), None)
                .err()
                .unwrap()
        ),
        2
    );
    assert!(make(
        MessageAction::React,
        "general",
        Some(TS),
        Some("bad emoji!")
    )
    .is_err());
    assert!(make(MessageAction::Pin, "general", Some("12345"), None).is_err());
    assert!(make(MessageAction::Pin, "general", None, None).is_err());
    assert_eq!(
        kind_of(
            &make(MessageAction::Pin, "nope", Some(TS), None)
                .err()
                .unwrap()
        ),
        3
    );
    // A permalink for a channel unknown locally is refused.
    assert!(make(
        MessageAction::Pin,
        "https://fake.slack.com/archives/C9/p1700000000000100",
        None,
        None
    )
    .is_err());
    // Marking something already read is flagged as a no-op.
    s.with_conn(|c| {
        c.execute(
            "UPDATE conversations SET last_read = ?1 WHERE id = 'C1'",
            [TS],
        )?;
        Ok(())
    })
    .unwrap();
    assert!(
        make(MessageAction::MarkRead, "general", Some(TS), None)
            .unwrap()
            .no_op
    );
}

#[test]
fn dm_preparation() {
    let s = store();
    let dm = |people: &[&str]| {
        let people: Vec<String> = people.iter().map(|s| s.to_string()).collect();
        s.with_read(|c| PreparedWrite::dm(c, "T1", &people))
    };
    let p = dm(&["alice", "@bob", "alice"]).unwrap();
    assert_eq!(p.action.action, "dm");
    assert_eq!(p.action.targets, vec!["U2", "U3"]);
    assert_eq!(p.request.method, "conversations.open");
    assert_eq!(p.request.params["users"], "U2,U3");
    assert!(dm(&[]).is_err());
    assert!(dm(&["gone"]).is_err());
    assert!(dm(&["nobody"]).is_err());
    assert!(dm(&["a"; 9]).is_err());
}

// ------------------------------------------------- execution (wiremock)

fn log_in(dir: &tempfile::TempDir) -> ActionLog {
    ActionLog::new(dir.path().join("log.db"))
}

#[tokio::test]
async fn send_persists_returns_ts_permalink_and_logs_acceptance() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .and(field("channel", "C1"))
        .and(field("text", "hello <@U2>\n\nvia agent"))
        .and(field("thread_ts", TS))
        .and(has_field("client_msg_id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok":true,"channel":"C1","ts":"1700000200.000300",
            "message":{"type":"message","user":"U1","text":"hello <@U2>\n\nvia agent",
                "ts":"1700000200.000300","thread_ts":TS}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    let mut cfg = config();
    cfg.signature = Some("via agent".into());
    cfg.allow = vec![rule("T1", &["C1"], &["send"])];
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);
    let p = prepare(
        &s,
        "general",
        MessageInput {
            thread_ts: Some(TS.into()),
            ..text_input("hello @alice")
        },
        &cfg,
    );
    // Allowed by rule: no --yes needed.
    let receipt = p
        .run_guarded(&log, &cfg, false, &s, || async { Ok(session(&server)) })
        .await
        .unwrap();
    assert_eq!(receipt.action, "send");
    assert_eq!(receipt.ts.as_deref(), Some("1700000200.000300"));
    assert!(receipt.permalink.is_some());
    assert!(!receipt.not_stored_locally);
    let stored = s
        .with_read(|c| flek_core::views::message(c, "C1", "1700000200.000300"))
        .unwrap()
        .unwrap();
    assert!(stored.text.contains("hello"));
    let entries = log.entries(10).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, "send");
    assert_eq!(entries[0].targets, vec!["C1"]);
    assert_eq!(entries[0].outcome, "accepted");
    assert!(entries[0].finished_at.is_some());
}

#[tokio::test]
async fn permalink_falls_back_to_slack_and_survives_its_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"channel":"C2","ts":"1700000300.000100"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat.getPermalink"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":"internal_error"})),
        )
        .mount(&server)
        .await;
    let s = store();
    // Without a cached workspace URL no local permalink can be built.
    s.with_conn(|c| {
        c.execute("UPDATE teams SET url = NULL", [])?;
        Ok(())
    })
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let p = prepare(&s, "random", text_input("x"), &config());
    let receipt = p
        .run_guarded(&log_in(&dir), &config(), true, &s, || async {
            Ok(session(&server))
        })
        .await
        .unwrap();
    // Remote acceptance is success even when the permalink lookup fails.
    assert_eq!(receipt.ts.as_deref(), Some("1700000300.000100"));
    assert_eq!(
        receipt.warning,
        Some("permalink_unavailable_message_accepted")
    );
}

#[tokio::test]
async fn failed_write_is_logged_by_category_without_error_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":false,"error":"channel_not_found secret-detail"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);
    let p = prepare(&s, "general", text_input("body-secret-777"), &config());
    let err = p
        .run_guarded(&log, &config(), true, &s, || async { Ok(session(&server)) })
        .await
        .err()
        .unwrap();
    assert_eq!(kind_of(&err), 7);
    let e = &log.entries(1).unwrap()[0];
    assert_eq!(e.outcome, "failed:7");
    // A failed attempt keeps its slot (no blind retry loops).
    let mut tight = config();
    tight.writes_per_minute = 1;
    assert_eq!(
        kind_of(&log.reserve(&tight, &p.action, true).err().unwrap()),
        6
    );
}

#[tokio::test]
async fn react_pin_save_mark_schedule_dm_execute_documented_calls() {
    let server = MockServer::start().await;
    for (endpoint, expected) in [
        (
            "reactions.add",
            vec![("channel", "C1"), ("timestamp", TS), ("name", "tada")],
        ),
        (
            "reactions.remove",
            vec![("channel", "C1"), ("timestamp", TS), ("name", "tada")],
        ),
        ("pins.add", vec![("channel", "C1"), ("timestamp", TS)]),
        ("pins.remove", vec![("channel", "C1"), ("timestamp", TS)]),
        ("stars.add", vec![("channel", "C1"), ("timestamp", TS)]),
        ("conversations.mark", vec![("channel", "C1"), ("ts", TS)]),
    ] {
        let mut m = Mock::given(method("POST")).and(path(format!("/{endpoint}")));
        for (k, v) in expected {
            m = m.and(field(k, v));
        }
        m.respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
            .expect(1)
            .mount(&server)
            .await;
    }
    let at = chrono::Utc::now().timestamp() + 600;
    Mock::given(method("POST"))
        .and(path("/chat.scheduleMessage"))
        .and(field("channel", "C2"))
        .and(field("post_at", at.to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"scheduled_message_id":"Q123","post_at":at})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .and(field("users", "U2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"channel":{"id":"D9","is_im":true,"user":"U2"}})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let s = store();
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);
    let mut cfg = config();
    cfg.writes_per_minute = 60;
    let run = |p: PreparedWrite| {
        let (s, log, cfg, server) = (&s, &log, &cfg, &server);
        async move {
            p.run_guarded(log, cfg, true, s, || async { Ok(session(server)) })
                .await
                .unwrap()
        }
    };
    let ma = |a, e: Option<&str>| {
        s.with_read(|c| PreparedWrite::message_action(c, "T1", a, "general", Some(TS), e))
            .unwrap()
    };

    let r = run(ma(MessageAction::React, Some("tada"))).await;
    assert_eq!((r.action.as_str(), r.ts.as_deref()), ("react", Some(TS)));
    run(ma(MessageAction::Unreact, Some("tada"))).await;
    run(ma(MessageAction::Pin, None)).await;
    let pinned = |s: &Store| -> i64 {
        s.with_read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM pins WHERE conversation_id='C1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    };
    assert_eq!(pinned(&s), 1);
    run(ma(MessageAction::Unpin, None)).await;
    assert_eq!(pinned(&s), 0);
    let saved = run(ma(MessageAction::Save, None)).await;
    assert_eq!(
        saved.warning,
        Some("legacy_star_only_later_api_unavailable")
    );
    run(ma(MessageAction::MarkRead, None)).await;

    let sched = run(prepare(
        &s,
        "random",
        MessageInput {
            post_at: Some(at),
            ..text_input("later")
        },
        &cfg,
    ))
    .await;
    assert_eq!(sched.scheduled_message_id.as_deref(), Some("Q123"));
    assert_eq!(sched.post_at, Some(at));

    let dm = run(s
        .with_read(|c| PreparedWrite::dm(c, "T1", &["alice".into()]))
        .unwrap())
    .await;
    assert_eq!(dm.channel.as_deref(), Some("D9"));
    assert!(!dm.not_stored_locally);

    let actions: Vec<_> = log
        .entries(100)
        .unwrap()
        .into_iter()
        .rev()
        .map(|e| e.action)
        .collect();
    assert_eq!(
        actions,
        [
            "react",
            "unreact",
            "pin",
            "unpin",
            "save",
            "mark-read",
            "schedule",
            "dm"
        ]
    );
}

#[tokio::test]
async fn idempotent_slack_codes_are_success() {
    let server = MockServer::start().await;
    for (endpoint, code) in [
        ("reactions.add", "already_reacted"),
        ("reactions.remove", "no_reaction"),
        ("pins.add", "already_pinned"),
        ("pins.remove", "no_pin"),
        ("stars.add", "already_starred"),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/{endpoint}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":code})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    let s = store();
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);
    let mut cfg = config();
    cfg.writes_per_minute = 60;
    for (a, e) in [
        (MessageAction::React, Some("tada")),
        (MessageAction::Unreact, Some("tada")),
        (MessageAction::Pin, None),
        (MessageAction::Unpin, None),
        (MessageAction::Save, None),
    ] {
        let p = s
            .with_read(|c| PreparedWrite::message_action(c, "T1", a, "general", Some(TS), e))
            .unwrap();
        p.run_guarded(&log, &cfg, true, &s, || async { Ok(session(&server)) })
            .await
            .unwrap();
    }
}

// ------------------------------------------------------------- redaction

#[tokio::test]
async fn audit_database_never_contains_payloads_or_credentials() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok":true,"channel":"C1","ts":"1700000400.000100"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat.getPermalink"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"ok":true,"permalink":"https://fake.slack.com/archives/C1/p1700000400000100"}),
        ))
        .mount(&server)
        .await;
    let s = store();
    let dir = tempfile::tempdir().unwrap();
    let log = log_in(&dir);
    let mut cfg = config();
    cfg.signature = Some("sig-marker-555".into());
    let p = prepare(
        &s,
        "general",
        MessageInput {
            blocks: Some(vec![
                json!({"type":"section","text":{"type":"mrkdwn","text":"block-secret-999"}}),
            ]),
            ..text_input("body-secret-777 hello @alice")
        },
        &cfg,
    );
    p.run_guarded(&log, &cfg, true, &s, || async { Ok(session(&server)) })
        .await
        .unwrap();
    // A failing write too.
    let p2 = prepare(&s, "random", text_input("body-secret-888"), &config());
    let _ = p2
        .run_guarded(&log, &config(), true, &s, || async {
            Err(Error::Slack {
                code: "invalid_auth xoxc-secret-token".into(),
                needed: None,
            })
        })
        .await;

    let mut bytes = Vec::new();
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        bytes.extend(std::fs::read(entry.unwrap().path()).unwrap());
    }
    let dump = String::from_utf8_lossy(&bytes);
    for secret in [
        "body-secret-777",
        "body-secret-888",
        "block-secret-999",
        "sig-marker-555",
        "xoxc",
        "xoxd",
        "fake-token",
        "fake-cookie",
        "hello",
        "secret-token",
    ] {
        assert!(!dump.contains(secret), "audit log leaked {secret}");
    }
    // Entries expose only these keys.
    let entries = serde_json::to_value(log.entries(10).unwrap()).unwrap();
    for e in entries.as_array().unwrap() {
        let mut keys: Vec<_> = e.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "action",
                "finished_at",
                "id",
                "outcome",
                "started_at",
                "targets",
                "team_id"
            ]
        );
    }
    // Unknown error text maps to its stable category only.
    assert_eq!(entries[0]["outcome"], "failed:7");
    assert_eq!(entries[1]["outcome"], "accepted");
}

#[cfg(unix)]
#[test]
fn audit_log_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("log.db");
    ActionLog::new(&path)
        .reserve(&config(), &action("react", "T1", &["C1"]), true)
        .unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let dmode = std::fs::metadata(path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dmode, 0o700);
}

#[test]
fn reading_the_log_needs_no_storage_and_orders_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("none.db");
    let log = ActionLog::new(&path);
    assert!(log.entries(10).unwrap().is_empty());
    assert!(!path.exists());
    let mut cfg = config();
    cfg.writes_per_minute = 60;
    for name in ["react", "pin", "save"] {
        log.reserve(&cfg, &action(name, "T1", &["C1"]), true)
            .unwrap();
    }
    let names: Vec<_> = log
        .entries(2)
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    assert_eq!(names, ["save", "pin"]);
    // Entries that never finished stay visible as pending.
    assert_eq!(log.entries(1).unwrap()[0].outcome, "pending");
}

#[test]
fn write_request_refuses_credentials_and_unlisted_methods() {
    // Only documented write methods may execute; auth fields are transport-owned.
    let server_rt = tokio::runtime::Runtime::new().unwrap();
    server_rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
            .expect(0)
            .mount(&server)
            .await;
        let c = client(&server);
        let s = store();
        let mut p = s
            .with_read(|c| {
                PreparedWrite::message_action(
                    c,
                    "T1",
                    MessageAction::Pin,
                    "general",
                    Some(TS),
                    None,
                )
            })
            .unwrap();
        p.request
            .params
            .insert("token".into(), "xoxc-attacker".into());
        assert!(matches!(
            p.request.execute(&c).await.err().unwrap(),
            Error::Forbidden
        ));
        p.request.params.remove("token");
        p.request.method = "chat.delete".into();
        assert!(matches!(
            p.request.execute(&c).await.err().unwrap(),
            Error::Forbidden
        ));
    });
}
