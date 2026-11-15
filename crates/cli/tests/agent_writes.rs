//! Guarded write commands without credentials: every path here stops before a
//! session is resolved (denied, dry run, rate limited, invalid), so the real
//! Keychain and Slack are never touched.
use assert_cmd::Command;
use flek_core::{
    agent::{ActionDescriptor, ActionLog, AgentConfig},
    store::{directory, Store},
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const TS: &str = "1700000000.123456";

struct Env {
    dir: tempfile::TempDir,
}
impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Store::open(&dir.path().join("fixture.db")).unwrap();
        db.with_conn(|c| {
            directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U1")?;
            c.execute_batch(
                "INSERT INTO users (id, team_id, name, real_name, display_name, deleted) VALUES
                   ('U1','T1','me','Me','me',0), ('U2','T1','alice','Alice','alice',0);
                 INSERT INTO conversations (id, team_id, kind, name, label) VALUES
                   ('C1','T1','public_channel','general','general'),
                   ('C2','T1','public_channel','random','random');",
            )?;
            Ok(())
        })
        .unwrap();
        Self { dir }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    fn config(&self, body: &str) {
        std::fs::write(self.path("agent.toml"), body).unwrap();
    }
    fn log(&self) -> ActionLog {
        ActionLog::new(self.path("log.db"))
    }
    fn cmd(&self) -> Command {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("flek"));
        cmd.arg("--db")
            .arg(self.path("fixture.db"))
            .args(["--team", "T1", "--json", "--agent-config"])
            .arg(self.path("agent.toml"))
            .arg("--agent-log")
            .arg(self.path("log.db"))
            .env("HOME", self.dir.path())
            .env("RUST_LOG", "trace")
            .env_remove("XDG_CONFIG_HOME");
        cmd
    }
    /// Run to success and return the single JSON object on stdout.
    fn ok(&self, args: &[&str]) -> Value {
        let out = self
            .cmd()
            .args(args)
            .assert()
            .success()
            .get_output()
            .clone();
        assert!(
            out.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout.iter().filter(|&&b| b == b'\n').count(), 1);
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["schema_version"], 1);
        v
    }
    /// Run to failure with the given exit code; return the typed error object.
    fn fail(&self, args: &[&str], code: i32) -> Value {
        let out = self
            .cmd()
            .args(args)
            .assert()
            .code(code)
            .get_output()
            .clone();
        assert!(out.stdout.is_empty(), "failures emit no stdout object");
        let v: Value = serde_json::from_slice(&out.stderr).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["ok"], false);
        v["error"].clone()
    }
}

fn scrub(mut v: Value) -> Value {
    if let Some(p) = v.pointer_mut("/write/request/params/client_msg_id") {
        assert!(p.as_str().is_some_and(|s| !s.is_empty()));
        *p = json!("<id>");
    }
    v
}

fn write(action: &str, targets: Value, method: &str, params: Value, authorized: bool) -> Value {
    json!({
        "schema_version": 1, "dry_run": true, "authorized": authorized,
        "write": {
            "action": {"action": action, "team_id": "T1", "targets": targets},
            "request": {"method": method, "params": params},
            "no_op": false, "warning": null
        }
    })
}

#[test]
fn writes_are_denied_by_default_before_any_session_or_log_entry() {
    let env = Env::new();
    let at = (chrono::Utc::now().timestamp() + 3600).to_string();
    for args in [
        &["send", "general", "hi"][..],
        &["react", "general", TS, "--emoji", "tada"],
        &["unreact", "general", TS, "--emoji", "tada"],
        &["mark-read", "general", TS],
        &["pin", "general", TS],
        &["unpin", "general", TS],
        &["save", "general", TS],
        &["dm", "alice"],
        &["schedule", "general", "hi", "--at", &at],
    ] {
        let err = env.fail(args, 8);
        assert_eq!(err["code"], "forbidden", "{args:?}");
        assert!(err["message"].as_str().unwrap().contains("agent.toml"));
    }
    // Denials are not writes: nothing logged, no log storage created.
    assert!(!env.path("log.db").exists());
}

#[test]
fn allow_rules_do_not_leak_across_channels_actions_or_teams() {
    let env = Env::new();
    env.config(
        r#"
[[allow]]
team = "T1"
channels = ["C1"]
actions = ["send"]
"#,
    );
    let authorized = |args: &[&str]| {
        let mut a = vec!["--dry-run"];
        a.extend_from_slice(args);
        env.ok(&a)["authorized"].clone()
    };
    assert_eq!(authorized(&["send", "general", "hi"]), true);
    assert_eq!(authorized(&["send", "random", "hi"]), false);
    assert_eq!(authorized(&["react", "general", TS, "--emoji", "x"]), false);
    assert_eq!(authorized(&["dm", "alice"]), false);
    // --yes authorizes the individual write regardless of rules.
    assert_eq!(authorized(&["--yes", "send", "random", "hi"]), true);
    // Dry run never creates the log.
    assert!(!env.path("log.db").exists());
}

#[test]
fn dry_run_contract_snapshots_for_every_write_command() {
    let env = Env::new();
    env.config("");
    let at = chrono::Utc::now().timestamp() + 3600;
    let at_s = at.to_string();
    let cases: Vec<(Vec<&str>, Value)> = vec![
        (
            vec!["send", "#general", "hello @alice & <b>"],
            write(
                "send",
                json!(["C1"]),
                "chat.postMessage",
                json!({"channel":"C1","client_msg_id":"<id>","text":"hello <@U2> &amp; &lt;b&gt;"}),
                false,
            ),
        ),
        (
            vec!["react", "general", TS, "--emoji", ":tada:"],
            write(
                "react",
                json!(["C1"]),
                "reactions.add",
                json!({"channel":"C1","name":"tada","timestamp":TS}),
                false,
            ),
        ),
        (
            vec!["unreact", "general", TS, "--emoji", "tada"],
            write(
                "unreact",
                json!(["C1"]),
                "reactions.remove",
                json!({"channel":"C1","name":"tada","timestamp":TS}),
                false,
            ),
        ),
        (
            vec!["mark-read", "general", TS],
            write(
                "mark-read",
                json!(["C1"]),
                "conversations.mark",
                json!({"channel":"C1","ts":TS}),
                false,
            ),
        ),
        (
            vec![
                "pin",
                "https://fake.slack.com/archives/C1/p1700000000123456",
            ],
            write(
                "pin",
                json!(["C1"]),
                "pins.add",
                json!({"channel":"C1","timestamp":TS}),
                false,
            ),
        ),
        (
            vec!["unpin", "general", TS],
            write(
                "unpin",
                json!(["C1"]),
                "pins.remove",
                json!({"channel":"C1","timestamp":TS}),
                false,
            ),
        ),
        (
            vec!["dm", "alice"],
            write(
                "dm",
                json!(["U2"]),
                "conversations.open",
                json!({"users":"U2","return_im":"true"}),
                false,
            ),
        ),
        (
            vec!["schedule", "random", "later", "--at", &at_s],
            write(
                "schedule",
                json!(["C2"]),
                "chat.scheduleMessage",
                json!({"channel":"C2","post_at":at_s,"text":"later"}),
                false,
            ),
        ),
    ];
    for (args, expected) in cases {
        let mut a = vec!["--dry-run"];
        a.extend(args.iter().copied());
        assert_eq!(scrub(env.ok(&a)), expected, "{args:?}");
    }
    // `save` carries the documented legacy-star warning.
    let mut save = write(
        "save",
        json!(["C1"]),
        "stars.add",
        json!({"channel":"C1","timestamp":TS}),
        false,
    );
    save["write"]["warning"] = json!("legacy_star_only_later_api_unavailable");
    assert_eq!(env.ok(&["--dry-run", "save", "general", TS]), save);
    assert!(!env.path("log.db").exists());
}

#[test]
fn dry_run_human_output_is_the_same_report_and_has_no_secrets() {
    let env = Env::new();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("flek"));
    let out = cmd
        .arg("--db")
        .arg(env.path("fixture.db"))
        .args(["--team", "T1", "--dry-run", "--agent-config"])
        .arg(env.path("agent.toml"))
        .args(["send", "general", "hi"])
        .assert()
        .success()
        .get_output()
        .clone();
    let text = String::from_utf8(out.stdout).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["write"]["request"]["method"], "chat.postMessage");
    let lower = text.to_lowercase();
    for needle in ["xox", "cookie", "token"] {
        assert!(!lower.contains(needle));
    }
}

#[test]
fn message_input_sources_signature_and_blocks() {
    let env = Env::new();
    env.config("signature = \"via agent\"\n");
    // piped stdin, explicit --stdin, '-', and --file all supply text
    let run_stdin = |args: &[&str], input: &str| -> Value {
        let out = env
            .cmd()
            .args(["--dry-run"])
            .args(args)
            .write_stdin(input)
            .assert()
            .success()
            .get_output()
            .clone();
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let text = |v: &Value| {
        v["write"]["request"]["params"]["text"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        text(&run_stdin(&["send", "general"], "from pipe\n")),
        "from pipe\n\n\nvia agent"
    );
    assert_eq!(
        text(&run_stdin(&["send", "general", "-"], "dash")),
        "dash\n\nvia agent"
    );
    assert_eq!(
        text(&run_stdin(&["send", "general", "--stdin"], "flag")),
        "flag\n\nvia agent"
    );
    let file = env.path("body.txt");
    std::fs::write(&file, "from file").unwrap();
    assert_eq!(
        text(&run_stdin(
            &["send", "general", "--file", file.to_str().unwrap()],
            ""
        )),
        "from file\n\nvia agent"
    );
    // blocks inline, from @file; thread options; signature block appended
    let v = env.ok(&[
        "--dry-run",
        "send",
        "general",
        "see blocks",
        "--thread",
        TS,
        "--broadcast",
        "--blocks",
        r#"[{"type":"divider"}]"#,
    ]);
    let params = &v["write"]["request"]["params"];
    assert_eq!(params["thread_ts"], TS);
    assert_eq!(params["reply_broadcast"], "true");
    let blocks: Vec<Value> = serde_json::from_str(params["blocks"].as_str().unwrap()).unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0], json!({"type":"divider"}));
    assert_eq!(blocks[1]["elements"][0]["text"], "via agent");
    let bf = env.path("blocks.json");
    std::fs::write(&bf, r#"[{"type":"divider"}]"#).unwrap();
    let arg = format!("@{}", bf.display());
    env.ok(&["--dry-run", "send", "general", "x", "--blocks", &arg]);
    // Without a signature configured the text is exactly what was given.
    env.config("");
    assert_eq!(text(&run_stdin(&["send", "general", "plain"], "")), "plain");
}

#[test]
fn invalid_input_is_a_typed_error_that_never_echoes_content() {
    let env = Env::new();
    for (args, code, exit) in [
        (
            vec!["send", "general", "x", "--blocks", "not-json-SECRET"],
            "usage",
            2,
        ),
        (
            vec!["send", "general", "x", "--blocks", r#"{"a":1}"#],
            "usage",
            2,
        ),
        (vec!["send", "general", "x", "--thread", "bad"], "usage", 2),
        (vec!["send", "general", "x", "--broadcast"], "usage", 2),
        (vec!["send", "general", "  "], "usage", 2),
        (vec!["send", "nope", "x"], "not_found", 3),
        (vec!["send", "gen", "x"], "ambiguous", 4),
        (vec!["send", "general", "@nobody"], "not_found", 3),
        (vec!["react", "general", TS], "usage", 2),
        (vec!["react", "general", "--emoji", "x"], "usage", 2),
        (vec!["schedule", "general", "x", "--at", "100"], "usage", 2),
        (
            vec!["schedule", "general", "x", "--at", "tomorrow"],
            "usage",
            2,
        ),
        (vec!["dm"], "usage", 2),
    ] {
        let mut a = vec!["--dry-run"];
        a.extend(args.iter().copied());
        let err = env.fail(&a, exit);
        assert_eq!(err["code"], code, "{args:?}");
        assert!(!err["message"].to_string().contains("SECRET"));
    }
}

#[test]
fn invalid_config_is_a_usage_error_that_does_not_quote_the_file() {
    let env = Env::new();
    env.config("writes_per_minute = 0\nsignature = \"leaky-secret\"\n");
    let err = env.fail(&["--dry-run", "send", "general", "hi"], 2);
    assert_eq!(err["code"], "usage");
    assert!(!err["message"].to_string().contains("leaky-secret"));
    env.config("garbage = [[[ leaky-secret");
    let err = env.fail(&["send", "general", "hi", "--yes"], 2);
    assert!(!err["message"].to_string().contains("leaky-secret"));
    assert!(!env.path("log.db").exists());
}

#[test]
fn rate_limit_is_enforced_across_processes_before_the_session() {
    let env = Env::new();
    env.config("writes_per_minute = 2\n");
    let cfg = AgentConfig::load(&env.path("agent.toml")).unwrap();
    let a = ActionDescriptor {
        action: "react".into(),
        team_id: "T1".into(),
        targets: vec!["C1".into()],
    };
    // Slots consumed by another process (here: an independent core connection).
    env.log().reserve(&cfg, &a, true).unwrap();
    env.log().reserve(&cfg, &a, true).unwrap();
    for args in [&["send", "general", "hi"][..], &["pin", "general", TS]] {
        let mut a = vec!["--yes"];
        a.extend_from_slice(args);
        let err = env.fail(&a, 6);
        assert_eq!(err["code"], "rate_limited");
    }
    // Dry runs are not limited and leave the log as it was.
    env.ok(&["--dry-run", "--yes", "send", "general", "hi"]);
    assert_eq!(env.log().entries(10).unwrap().len(), 2);
}

#[test]
fn flek_log_reads_without_credentials_and_exposes_only_metadata() {
    let env = Env::new();
    let empty = env.ok(&["log"]);
    assert_eq!(empty, json!({"schema_version":1,"entries":[]}));
    assert!(!env.path("log.db").exists());

    let cfg = AgentConfig {
        writes_per_minute: 60,
        ..Default::default()
    };
    for (name, team, target) in [("send", "T1", "C1"), ("pin", "T1", "C2")] {
        env.log()
            .reserve(
                &cfg,
                &ActionDescriptor {
                    action: name.into(),
                    team_id: team.into(),
                    targets: vec![target.into()],
                },
                true,
            )
            .unwrap();
    }
    let v = env.ok(&["log", "--limit", "1"]);
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    let e = entries[0].as_object().unwrap();
    let mut keys: Vec<_> = e.keys().map(String::as_str).collect();
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
    assert_eq!(
        (e["action"].as_str(), e["outcome"].as_str()),
        (Some("pin"), Some("pending"))
    );
    assert_eq!(e["targets"], json!(["C2"]));
    assert_eq!(e["finished_at"], Value::Null);
    assert_eq!(env.ok(&["log"])["entries"].as_array().unwrap().len(), 2);
    env.fail(&["log", "--limit", "0"], 2);
    env.fail(&["log", "--limit", "1001"], 2);
}

#[test]
fn missing_database_or_team_is_reported_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(assert_cmd::cargo::cargo_bin!("flek"))
        .arg("--db")
        .arg(dir.path().join("empty.db"))
        .args(["--json", "--dry-run", "send", "general", "hi"])
        .arg("--agent-log")
        .arg(dir.path().join("log.db"))
        .env("HOME", dir.path())
        .assert()
        .code(3)
        .get_output()
        .clone();
    let v: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(v["error"]["code"], "not_found");
    assert!(!Path::new(&dir.path().join("log.db")).exists());
}

const UPLOAD_BYTES: &str = "UPLOAD-BODY-MUST-NOT-APPEAR";

fn upload_file(env: &Env) -> String {
    let p = env.path("notes.txt");
    std::fs::write(&p, UPLOAD_BYTES).unwrap();
    p.to_str().unwrap().to_owned()
}

#[test]
fn upload_is_denied_by_default_before_any_session_or_log_entry() {
    let env = Env::new();
    let file = upload_file(&env);
    let err = env.fail(&["upload", &file, "--channel", "general"], 8);
    assert_eq!(err["code"], "forbidden");
    assert!(err["message"].as_str().unwrap().contains("agent.toml"));
    assert!(!env.path("log.db").exists());
    // An allow rule for another action or channel does not authorize it.
    env.config(
        r#"
[[allow]]
team = "T1"
channels = ["C1"]
actions = ["send"]
[[allow]]
team = "T1"
channels = ["C2"]
actions = ["upload"]
"#,
    );
    env.fail(&["upload", &file, "--channel", "general"], 8);
    assert!(!env.path("log.db").exists());
}

#[test]
fn upload_dry_run_reports_metadata_only_and_touches_nothing() {
    let env = Env::new();
    let file = upload_file(&env);
    let expected = |authorized: bool| {
        json!({
            "schema_version": 1, "dry_run": true, "authorized": authorized,
            "write": {
                "action": {"action": "upload", "team_id": "T1", "targets": ["C1"]},
                "upload": {"filename": "notes.txt", "size": UPLOAD_BYTES.len(),
                    "channel": "C1", "thread_ts": TS, "comment": "see this"}
            }
        })
    };
    let args = [
        "--dry-run",
        "upload",
        &file,
        "--channel",
        "#general",
        "--thread",
        TS,
        "--comment",
        "see this",
    ];
    assert_eq!(env.ok(&args), expected(false));
    let out = env.cmd().args(args).assert().success().get_output().clone();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains(UPLOAD_BYTES) && !text.contains(env.dir.path().to_str().unwrap()));
    // --yes and a matching allow rule both authorize, still without any I/O.
    let mut yes = vec!["--yes"];
    yes.extend(args);
    assert_eq!(env.ok(&yes), expected(true));
    env.config("[[allow]]\nteam = \"T1\"\nchannels = [\"C1\"]\nactions = [\"upload\"]\n");
    assert_eq!(env.ok(&args), expected(true));
    assert!(!env.path("log.db").exists());
}

#[test]
fn upload_human_dry_run_is_the_same_report_without_secrets() {
    let env = Env::new();
    let file = upload_file(&env);
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("flek"));
    let human = cmd
        .arg("--db")
        .arg(env.path("fixture.db"))
        .args(["--team", "T1", "--dry-run", "--agent-config"])
        .arg(env.path("agent.toml"))
        .arg("--agent-log")
        .arg(env.path("log.db"))
        .env("HOME", env.dir.path())
        .args(["upload", &file, "--channel", "general"])
        .assert()
        .success()
        .get_output()
        .clone();
    let human: Value = serde_json::from_slice(&human.stdout).unwrap();
    assert_eq!(human["write"]["upload"]["filename"], "notes.txt");
    assert!(human.get("schema_version").is_none());
    assert!(!human.to_string().contains(UPLOAD_BYTES));
}

#[test]
fn upload_invalid_input_is_a_typed_error_before_the_policy_or_session() {
    let env = Env::new();
    let file = upload_file(&env);
    let dir = env.dir.path().to_str().unwrap().to_owned();
    let empty = env.path("empty");
    std::fs::write(&empty, b"").unwrap();
    let empty = empty.to_str().unwrap().to_owned();
    let big = env.path("big.bin");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(100 * 1024 * 1024 + 1)
        .unwrap();
    let big = big.to_str().unwrap().to_owned();
    for (args, code) in [
        (vec!["upload", &file, "--channel", "nope"], 3),
        (
            vec!["upload", &file, "--channel", "general", "--thread", "x"],
            2,
        ),
        (vec!["upload", &dir, "--channel", "general"], 2),
        (vec!["upload", &empty, "--channel", "general"], 2),
        (vec!["upload", &big, "--channel", "general"], 2),
    ] {
        // Not even --yes gets past local validation, and no session is needed to say so.
        let mut all = vec!["--yes"];
        all.extend(args.clone());
        env.fail(&all, code);
    }
    let missing = env.path("missing.txt");
    env.cmd()
        .args([
            "--yes",
            "upload",
            missing.to_str().unwrap(),
            "--channel",
            "general",
        ])
        .assert()
        .failure();
    assert!(!env.path("log.db").exists());
}

#[test]
fn upload_rate_limit_is_enforced_before_the_session() {
    let env = Env::new();
    let file = upload_file(&env);
    env.config("writes_per_minute = 1\n");
    let log = env.log();
    let cfg = AgentConfig::load(&env.path("agent.toml")).unwrap();
    log.reserve(
        &cfg,
        &ActionDescriptor {
            action: "send".into(),
            team_id: "T1".into(),
            targets: vec!["C1".into()],
        },
        true,
    )
    .unwrap();
    let err = env.fail(&["--yes", "upload", &file, "--channel", "general"], 6);
    assert_eq!(err["code"], "rate_limited");
    assert_eq!(log.entries(10).unwrap().len(), 1);
}

fn with_messages(env: &Env) {
    use flek_core::store::messages::{upsert_messages, MessageRow};
    let db = Store::open(&env.path("fixture.db")).unwrap();
    db.with_conn(|c| {
        let row = |ts: &str, user: &str| MessageRow {
            ts: ts.into(),
            user_id: Some(user.into()),
            text: "stored".into(),
            raw_json: json!({"ts": ts, "user": user}).to_string(),
            ..Default::default()
        };
        upsert_messages(c, "C1", &[row(TS, "U1"), row("1700000000.222222", "U2")])?;
        Ok(())
    })
    .unwrap();
}

#[test]
fn edit_and_delete_are_guarded_like_every_other_write() {
    let env = Env::new();
    with_messages(&env);
    // Denied by default, before any session or log entry.
    for args in [
        &["edit", "general", TS, "--text", "new"][..],
        &["delete", "general", TS],
    ] {
        let err = env.fail(args, 8);
        assert_eq!(err["code"], "forbidden", "{args:?}");
        assert!(err["message"].as_str().unwrap().contains("agent.toml"));
    }
    assert!(!env.path("log.db").exists());

    // Dry run shows the exact prepared form; --yes and rules decide authorization.
    let edit = env.ok(&["--dry-run", "edit", "#general", TS, "--text", "a & @alice"]);
    assert_eq!(
        edit,
        write(
            "edit",
            json!(["C1"]),
            "chat.update",
            json!({"channel":"C1","ts":TS,"text":"a &amp; <@U2>"}),
            false
        )
    );
    let delete = env.ok(&["--dry-run", "--yes", "delete", "general", TS]);
    assert_eq!(
        delete,
        write(
            "delete",
            json!(["C1"]),
            "chat.delete",
            json!({"channel":"C1","ts":TS}),
            true
        )
    );
    env.config(
        r#"
[[allow]]
team = "T1"
channels = ["C1"]
actions = ["edit"]
"#,
    );
    assert_eq!(
        env.ok(&["--dry-run", "edit", "general", TS, "--text", "x"])["authorized"],
        true
    );
    assert_eq!(
        env.ok(&["--dry-run", "delete", "general", TS])["authorized"],
        false
    );
    assert!(!env.path("log.db").exists());
}

#[test]
fn edit_and_delete_refuse_foreign_missing_and_invalid_targets_before_credentials() {
    let env = Env::new();
    with_messages(&env);
    let foreign = "1700000000.222222";
    // Another author's message is forbidden even with --yes and in a dry run,
    // and the error is not the "pass --yes" hint.
    for args in [
        &["--yes", "delete", "general", foreign][..],
        &[
            "--dry-run",
            "--yes",
            "edit",
            "general",
            foreign,
            "--text",
            "x",
        ],
    ] {
        let err = env.fail(args, 8);
        assert_eq!(err["code"], "forbidden");
        assert!(!err["message"].as_str().unwrap().contains("agent.toml"));
    }
    env.fail(&["--yes", "delete", "general", "1700000000.999999"], 3);
    env.fail(&["--yes", "delete", "nope", TS], 3);
    env.fail(&["--yes", "delete", "general", "bad"], 2);
    env.fail(&["--yes", "edit", "general", TS, "--text", "   "], 2);
    // A permalink works for delete.
    let link = format!(
        "https://fake.slack.com/archives/C1/p{}",
        TS.replace('.', "")
    );
    assert_eq!(
        env.ok(&["--dry-run", "--yes", "delete", &link])["write"]["request"]["params"]["ts"],
        TS
    );
    assert!(!env.path("log.db").exists());
}
