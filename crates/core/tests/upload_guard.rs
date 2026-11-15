//! M8.7 guard for uploads: offline preparation, dry run without file bytes,
//! deny-by-default before any session, rate limit, lazy session, audit categories.
use flek_core::{
    agent::{ActionLog, AgentConfig, AllowRule},
    api::Client,
    store::{directory, Store},
    writes::{PreparedUpload, WriteSession},
    Error, Secret,
};
use serde_json::{json, Value};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::watch;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

const SECRET_BYTES: &str = "TOP-SECRET-FILE-CONTENT";
const FILE_ID: &str = "F0GUARD1";
const TS: &str = "1700000100.000100";

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.with_conn(|c| {
        directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U1")?;
        c.execute_batch(
            "INSERT INTO users (id, team_id, name, real_name, display_name, deleted)
               VALUES ('U1','T1','me','Me','me',0);
             INSERT INTO conversations (id, team_id, kind, name, label) VALUES
               ('C1','T1','public_channel','general','general'),
               ('C2','T1','public_channel','random','random');",
        )?;
        Ok(())
    })
    .unwrap();
    s
}

fn file(dir: &tempfile::TempDir) -> PathBuf {
    let p = dir.path().join("report.txt");
    std::fs::write(&p, SECRET_BYTES).unwrap();
    p
}

fn prepare(
    s: &Store,
    p: &Path,
    channel: &str,
    thread: Option<&str>,
    comment: &str,
    cfg: &AgentConfig,
) -> Result<PreparedUpload, Error> {
    s.with_read(|c| {
        PreparedUpload::prepare(c, "T1", p, channel, thread.map(str::to_owned), comment, cfg)
    })
}

fn rule(channels: &[&str], actions: &[&str]) -> AllowRule {
    AllowRule {
        team: "T1".into(),
        channels: channels.iter().map(|s| s.to_string()).collect(),
        actions: actions.iter().map(|s| s.to_string()).collect(),
    }
}

fn session(server: &MockServer, team: &str) -> WriteSession {
    WriteSession {
        client: Client::with_base_url(
            Secret::new("xoxc-fake-token"),
            Secret::new("xoxd-fake-cookie"),
            &server.uri(),
        )
        .unwrap()
        .without_pacing(),
        team_id: team.into(),
        user_id: "U1".into(),
    }
}

async fn mount_happy(server: &MockServer, complete: ResponseTemplate, history_has_share: bool) {
    Mock::given(path("/files.getUploadURLExternal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "file_id": FILE_ID, "upload_url": format!("{}/upload/v1/x", server.uri())
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/upload/v1/x"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;
    Mock::given(path("/files.completeUploadExternal"))
        .respond_with(complete)
        .mount(server)
        .await;
    let messages = if history_has_share {
        json!([{"type":"message","user":"U1","ts":TS,"text":"",
            "files":[{"id":FILE_ID,"name":"report.txt","mimetype":"text/plain","size":23}]}])
    } else {
        json!([])
    };
    Mock::given(path("/conversations.history"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "has_more": false, "messages": messages})),
        )
        .mount(server)
        .await;
    Mock::given(path("/chat.getPermalink"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"ok": true, "permalink": "https://fake.slack.com/archives/C1/p1700000100000100"}),
        ))
        .mount(server)
        .await;
}

fn ok_body() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"ok": true, "files": [{"id": FILE_ID}]}))
}

fn never() -> watch::Receiver<bool> {
    watch::channel(false).1
}

#[allow(clippy::too_many_arguments)]
async fn run(
    p: &PreparedUpload,
    log: &ActionLog,
    cfg: &AgentConfig,
    yes: bool,
    s: &Store,
    server: &MockServer,
    team: &str,
    session_calls: &Cell<u32>,
) -> Result<flek_core::writes::UploadReceipt, Error> {
    p.run_guarded(log, cfg, yes, s, never(), Arc::new(|_| {}), || async {
        session_calls.set(session_calls.get() + 1);
        Ok(session(server, team))
    })
    .await
}

fn entries(log: &ActionLog) -> Vec<(String, String, Vec<String>)> {
    log.entries(50)
        .unwrap()
        .into_iter()
        .map(|e| (e.action, e.outcome, e.targets))
        .collect()
}

#[test]
fn preparation_resolves_offline_and_validates_everything_locally() {
    let dir = tempfile::tempdir().unwrap();
    let s = store();
    let cfg = AgentConfig::default();
    let p = prepare(
        &s,
        &file(&dir),
        "general",
        Some("1700000000.000001"),
        "hi",
        &cfg,
    )
    .unwrap();
    assert_eq!(p.action.action, "upload");
    assert_eq!(p.action.team_id, "T1");
    // A thread reply stays one target: the destination channel.
    assert_eq!(p.action.targets, vec!["C1".to_string()]);
    assert_eq!(p.upload.size, SECRET_BYTES.len() as u64);
    assert_eq!(p.upload.filename, "report.txt");
    assert_eq!(p.upload.thread_ts.as_deref(), Some("1700000000.000001"));
    assert_eq!(p.upload.comment, "hi");
    p.action.validate().unwrap();

    let err = |r: Result<PreparedUpload, Error>| r.err().expect("must fail").kind().exit_code();
    let f = file(&dir);
    assert_eq!(
        err(prepare(&s, &f, "nope", None, "", &cfg)),
        3,
        "unknown channel"
    );
    assert!(
        matches!(err(prepare(&s, &f, "gen", None, "", &cfg)), 3 | 4),
        "no prefix matching"
    );
    assert_eq!(err(prepare(&s, &f, "general", Some("bad"), "", &cfg)), 2);
    assert_eq!(
        err(prepare(&s, dir.path(), "general", None, "", &cfg)),
        2,
        "directory"
    );
    assert!(prepare(&s, &dir.path().join("missing"), "general", None, "", &cfg).is_err());
    let empty = dir.path().join("empty");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(err(prepare(&s, &empty, "general", None, "", &cfg)), 2);
    let big = dir.path().join("big.bin");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(100 * 1024 * 1024 + 1)
        .unwrap();
    assert_eq!(
        err(prepare(&s, &big, "general", None, "", &cfg)),
        2,
        "over 100 MiB"
    );
    let limit = dir.path().join("limit.bin");
    std::fs::File::create(&limit)
        .unwrap()
        .set_len(100 * 1024 * 1024)
        .unwrap();
    assert!(prepare(&s, &limit, "general", None, "", &cfg).is_ok());
    let long = "x".repeat(40_001);
    assert_eq!(err(prepare(&s, &f, "general", None, &long, &cfg)), 2);
    // Nothing is read: preparation of a 100 MiB sparse file is a stat.
}

#[test]
fn signature_is_appended_to_the_comment_like_message_text() {
    let dir = tempfile::tempdir().unwrap();
    let s = store();
    let cfg = AgentConfig {
        signature: Some("via agent".into()),
        ..Default::default()
    };
    let with = prepare(&s, &file(&dir), "general", None, "note", &cfg).unwrap();
    assert_eq!(with.upload.comment, "note\n\nvia agent");
    let bare = prepare(&s, &file(&dir), "general", None, "", &cfg).unwrap();
    assert_eq!(bare.upload.comment, "via agent");
    let none = prepare(
        &s,
        &file(&dir),
        "general",
        None,
        "",
        &AgentConfig::default(),
    )
    .unwrap();
    assert_eq!(none.upload.comment, "");
}

#[test]
fn dry_run_report_shows_metadata_and_never_the_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let s = store();
    let cfg = AgentConfig::default();
    let p = prepare(
        &s,
        &file(&dir),
        "#general",
        Some("1700000000.000001"),
        "look",
        &cfg,
    )
    .unwrap();
    let denied = p.dry_run_report(&cfg, false);
    assert_eq!(
        denied,
        json!({"dry_run": true, "authorized": false, "write": {
            "action": {"action": "upload", "team_id": "T1", "targets": ["C1"]},
            "upload": {"filename": "report.txt", "size": 23, "channel": "C1",
                "thread_ts": "1700000000.000001", "comment": "look"}}})
    );
    let text = denied.to_string();
    assert!(!text.contains(SECRET_BYTES) && !text.contains(dir.path().to_str().unwrap()));
    assert_eq!(p.dry_run_report(&cfg, true)["authorized"], true);
    let allowed = AgentConfig {
        allow: vec![rule(&["C1"], &["upload"])],
        ..Default::default()
    };
    assert_eq!(p.dry_run_report(&allowed, false)["authorized"], true);
    // Rules for other channels/actions/teams do not apply.
    for cfg in [
        AgentConfig {
            allow: vec![rule(&["C2"], &["upload"])],
            ..Default::default()
        },
        AgentConfig {
            allow: vec![rule(&["C1"], &["send"])],
            ..Default::default()
        },
        AgentConfig {
            allow: vec![AllowRule {
                team: "T2".into(),
                ..rule(&["C1"], &["upload"])
            }],
            ..Default::default()
        },
    ] {
        assert_eq!(p.dry_run_report(&cfg, false)["authorized"], false);
    }
    // A wildcard action rule covers uploads too.
    let wild = AgentConfig {
        allow: vec![rule(&["*"], &["*"])],
        ..Default::default()
    };
    assert_eq!(p.dry_run_report(&wild, false)["authorized"], true);
}

#[tokio::test]
async fn denied_upload_touches_no_session_network_log_or_file_read() {
    let server = MockServer::start().await;
    mount_happy(&server, ok_body(), true).await;
    let dir = tempfile::tempdir().unwrap();
    let s = store();
    let log = ActionLog::new(dir.path().join("log.db"));
    let cfg = AgentConfig::default();
    let p = prepare(&s, &file(&dir), "general", None, "", &cfg).unwrap();
    // Even a vanished source must not matter before the policy answers.
    std::fs::remove_file(dir.path().join("report.txt")).unwrap();
    let calls = Cell::new(0);
    let err = run(&p, &log, &cfg, false, &s, &server, "T1", &calls)
        .await
        .err()
        .unwrap();
    assert!(matches!(err, Error::Forbidden));
    assert_eq!(err.kind().exit_code(), 8);
    assert_eq!(calls.get(), 0);
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(entries(&log).is_empty());
}

#[tokio::test]
async fn allowed_upload_runs_once_and_logs_accepted_without_payloads() {
    let server = MockServer::start().await;
    mount_happy(&server, ok_body(), true).await;
    let dir = tempfile::tempdir().unwrap();
    let s = store();
    let log = ActionLog::new(dir.path().join("log.db"));
    let cfg = AgentConfig {
        allow: vec![rule(&["C1"], &["upload"])],
        ..Default::default()
    };
    let p = prepare(&s, &file(&dir), "general", None, "private comment", &cfg).unwrap();
    let calls = Cell::new(0);
    let r = run(&p, &log, &cfg, false, &s, &server, "T1", &calls)
        .await
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!((r.action.as_str(), r.file_id.as_str()), ("upload", FILE_ID));
    assert_eq!(r.channel.as_deref(), Some("C1"));
    assert_eq!(r.ts.as_deref(), Some(TS));
    assert!(r.message.is_some() && !r.not_stored_locally && !r.completion_unknown);
    assert!(r.permalink.as_deref().unwrap().contains("fake.slack.com"));
    assert_eq!(
        entries(&log),
        vec![("upload".into(), "accepted".into(), vec!["C1".to_string()])]
    );
    let raw = std::fs::read(dir.path().join("log.db")).unwrap();
    let raw = String::from_utf8_lossy(&raw);
    for secret in [
        SECRET_BYTES,
        "private comment",
        "report.txt",
        "xox",
        "ticket",
    ] {
        assert!(!raw.contains(secret), "audit log contains {secret}");
    }
    let receipt: Value = serde_json::to_value(&r).unwrap();
    assert!(!receipt.to_string().contains("xox"));
}

#[tokio::test]
async fn session_is_resolved_after_policy_and_after_local_snapshot() {
    let server = MockServer::start().await;
    mount_happy(&server, ok_body(), true).await;
    let dir = tempfile::tempdir().unwrap();
    let s = store();
    let log = ActionLog::new(dir.path().join("log.db"));
    let cfg = AgentConfig::default();
    let p = prepare(&s, &file(&dir), "general", None, "", &cfg).unwrap();
    std::fs::remove_file(dir.path().join("report.txt")).unwrap();
    let calls = Cell::new(0);
    // Authorized (yes) but the source disappeared: fails locally, no credentials.
    let err = run(&p, &log, &cfg, true, &s, &server, "T1", &calls)
        .await
        .err()
        .unwrap();
    assert!(matches!(err, Error::Io(_)), "{err:?}");
    assert_eq!(calls.get(), 0);
    assert!(server.received_requests().await.unwrap().is_empty());
    // The attempt keeps its slot and is logged by category only.
    let e = entries(&log);
    assert_eq!(e.len(), 1);
    assert!(e[0].1.starts_with("failed:"));
}

#[tokio::test]
async fn wrong_team_session_is_refused_before_any_request() {
    let server = MockServer::start().await;
    mount_happy(&server, ok_body(), true).await;
    let dir = tempfile::tempdir().unwrap();
    let s = store();
    let log = ActionLog::new(dir.path().join("log.db"));
    let cfg = AgentConfig::default();
    let p = prepare(&s, &file(&dir), "general", None, "", &cfg).unwrap();
    let calls = Cell::new(0);
    let err = run(&p, &log, &cfg, true, &s, &server, "T2", &calls)
        .await
        .err()
        .unwrap();
    assert!(matches!(err, Error::Forbidden));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn rate_limit_is_shared_with_other_writes_and_precedes_the_session() {
    let server = MockServer::start().await;
    mount_happy(&server, ok_body(), true).await;
    let dir = tempfile::tempdir().unwrap();
    let s = store();
    let log = ActionLog::new(dir.path().join("log.db"));
    let cfg = AgentConfig {
        writes_per_minute: 1,
        ..Default::default()
    };
    let p = prepare(&s, &file(&dir), "general", None, "", &cfg).unwrap();
    let calls = Cell::new(0);
    run(&p, &log, &cfg, true, &s, &server, "T1", &calls)
        .await
        .unwrap();
    let before = server.received_requests().await.unwrap().len();
    let err = run(&p, &log, &cfg, true, &s, &server, "T1", &calls)
        .await
        .err()
        .unwrap();
    assert_eq!(err.kind().exit_code(), 6, "{err:?}");
    assert_eq!(calls.get(), 1);
    assert_eq!(server.received_requests().await.unwrap().len(), before);
    assert_eq!(entries(&log).len(), 1);
}

#[tokio::test]
async fn ambiguous_completion_leaves_the_log_entry_pending_and_is_not_an_error() {
    let server = MockServer::start().await;
    mount_happy(&server, ResponseTemplate::new(502), false).await;
    let dir = tempfile::tempdir().unwrap();
    let s = store();
    let log = ActionLog::new(dir.path().join("log.db"));
    let cfg = AgentConfig::default();
    let p = prepare(&s, &file(&dir), "general", None, "", &cfg).unwrap();
    let calls = Cell::new(0);
    let r = run(&p, &log, &cfg, true, &s, &server, "T1", &calls)
        .await
        .unwrap();
    assert!(r.completion_unknown && r.not_stored_locally && r.ts.is_none() && r.message.is_none());
    assert_eq!(r.file_id, FILE_ID);
    assert_eq!(
        r.warning,
        Some("completion_unconfirmed_check_history_before_retrying")
    );
    // Never recorded as accepted: "pending" means check history before retrying.
    assert_eq!(
        entries(&log),
        vec![("upload".into(), "pending".into(), vec!["C1".to_string()])]
    );
    let completes = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == "/files.completeUploadExternal")
        .count();
    assert_eq!(completes, 1);
}

#[tokio::test]
async fn definitive_refusal_is_a_failed_log_category() {
    let server = MockServer::start().await;
    mount_happy(
        &server,
        ResponseTemplate::new(200).set_body_json(json!({"ok": false, "error": "not_in_channel"})),
        false,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let s = store();
    let log = ActionLog::new(dir.path().join("log.db"));
    let cfg = AgentConfig::default();
    let p = prepare(&s, &file(&dir), "general", None, "", &cfg).unwrap();
    let calls = Cell::new(0);
    let err = run(&p, &log, &cfg, true, &s, &server, "T1", &calls)
        .await
        .err()
        .unwrap();
    assert!(matches!(&err, Error::Slack { code, .. } if code == "not_in_channel"));
    let e = entries(&log);
    assert_eq!(e.len(), 1);
    assert!(e[0].1.starts_with("failed:") && !e[0].1.contains("not_in_channel"));
}
