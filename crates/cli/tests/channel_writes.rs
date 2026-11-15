//! M8.5 channel/status commands behind the write guard, without credentials: every
//! path here stops before a session is resolved, so the real Keychain and Slack
//! are never touched.
use assert_cmd::Command;
use flek_core::{
    agent::ActionLog,
    store::{directory, Store},
};
use serde_json::{json, Value};
use std::path::PathBuf;

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
                   ('U1','T1','me','Me','me',0), ('U2','T1','alice','Alice','alice',0),
                   ('U3','T1','alina','Alina','alina',0);
                 INSERT INTO conversations (id, team_id, kind, name, label, is_member) VALUES
                   ('C1','T1','public_channel','general','general',1),
                   ('C2','T1','public_channel','random','random',1),
                   ('C4','T1','public_channel','lounge','lounge',0);",
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
    fn cmd(&self) -> Command {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("flek"));
        cmd.arg("--db")
            .arg(self.path("fixture.db"))
            .args(["--team", "T1", "--json", "--agent-config"])
            .arg(self.path("agent.toml"))
            .arg("--agent-log")
            .arg(self.path("log.db"))
            .env("HOME", self.dir.path())
            .env_remove("XDG_CONFIG_HOME");
        cmd
    }
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

const EVERY_COMMAND: [&[&str]; 11] = [
    &["channel", "create", "ops-room"],
    &["channel", "create", "ops-room", "--private"],
    &["channel", "invite", "random", "alice"],
    &["channel", "kick", "random", "alice"],
    &["channel", "join", "lounge"],
    &["channel", "leave", "random"],
    &["channel", "topic", "random", "new topic"],
    &["channel", "purpose", "random", "why"],
    &["channel", "rename", "random", "off-topic"],
    &["channel", "archive", "random"],
    &["status", "clear"],
];

fn write(action: &str, target: &str, method: &str, params: Value, authorized: bool) -> Value {
    json!({
        "schema_version": 1, "dry_run": true, "authorized": authorized,
        "write": {
            "action": {"action": action, "team_id": "T1", "targets": [target]},
            "request": {"method": method, "params": params},
            "no_op": false, "warning": null
        }
    })
}

#[test]
fn every_new_command_is_denied_by_default_before_any_session_or_log_entry() {
    let env = Env::new();
    let mut all: Vec<Vec<&str>> = EVERY_COMMAND.iter().map(|a| a.to_vec()).collect();
    all.push(vec!["status", "set", "Focus", "--emoji", ":books:"]);
    assert_eq!(all.len(), 12);
    for args in all {
        let err = env.fail(&args, 8);
        assert_eq!(err["code"], "forbidden", "{args:?}");
        assert!(err["message"].as_str().unwrap().contains("agent.toml"));
    }
    // Denials are not writes: no log storage was created.
    assert!(!env.path("log.db").exists());
    assert!(ActionLog::new(env.path("log.db"))
        .entries(10)
        .unwrap()
        .is_empty());
}

#[test]
fn a_matching_allow_rule_authorizes_only_its_own_action_and_target() {
    let env = Env::new();
    env.config(
        r#"
[[allow]]
team = "T1"
channels = ["C2", "new:ops-room", "U1"]
actions = ["channel-topic", "channel-create", "status-set"]
"#,
    );
    let authorized = |args: &[&str]| {
        let mut a = vec!["--dry-run"];
        a.extend_from_slice(args);
        env.ok(&a)["authorized"].clone()
    };
    assert_eq!(authorized(&["channel", "topic", "random", "x"]), true);
    assert_eq!(authorized(&["channel", "topic", "lounge", "x"]), false);
    assert_eq!(authorized(&["channel", "purpose", "random", "x"]), false);
    assert_eq!(authorized(&["channel", "create", "Ops Room"]), true);
    assert_eq!(authorized(&["channel", "create", "other"]), false);
    assert_eq!(authorized(&["status", "set", "hi"]), true);
    assert_eq!(authorized(&["status", "clear"]), false);
    // --yes authorizes the invocation; the report says so too.
    assert_eq!(authorized(&["--yes", "channel", "archive", "random"]), true);
    assert!(!env.path("log.db").exists());
}

#[test]
fn dry_run_snapshots_for_every_new_command() {
    let env = Env::new();
    let expires = chrono::Utc::now().timestamp() + 7200;
    let expires_s = expires.to_string();
    let profile = format!(
        r#"{{"status_text":"Focus","status_emoji":":books:","status_expiration":{expires}}}"#
    );
    let cleared = r#"{"status_text":"","status_emoji":"","status_expiration":0}"#;
    let cases: Vec<(Vec<&str>, Value)> = vec![
        (
            vec!["channel", "create", "Ops Room"],
            write(
                "channel-create",
                "new:ops-room",
                "conversations.create",
                json!({"name":"ops-room","is_private":"false"}),
                false,
            ),
        ),
        (
            vec!["channel", "create", "ops-room", "--private"],
            write(
                "channel-create",
                "new:ops-room",
                "conversations.create",
                json!({"name":"ops-room","is_private":"true"}),
                false,
            ),
        ),
        (
            vec!["channel", "invite", "#random", "alice"],
            write(
                "channel-invite",
                "C2",
                "conversations.invite",
                json!({"channel":"C2","users":"U2"}),
                false,
            ),
        ),
        (
            vec!["channel", "kick", "random", "U2"],
            write(
                "channel-kick",
                "C2",
                "conversations.kick",
                json!({"channel":"C2","user":"U2"}),
                false,
            ),
        ),
        (
            vec!["channel", "join", "lounge"],
            write(
                "channel-join",
                "C4",
                "conversations.join",
                json!({"channel":"C4"}),
                false,
            ),
        ),
        (
            vec!["channel", "leave", "random"],
            write(
                "channel-leave",
                "C2",
                "conversations.leave",
                json!({"channel":"C2"}),
                false,
            ),
        ),
        (
            vec!["channel", "topic", "random", "new topic"],
            write(
                "channel-topic",
                "C2",
                "conversations.setTopic",
                json!({"channel":"C2","topic":"new topic"}),
                false,
            ),
        ),
        (
            vec!["channel", "purpose", "random", ""],
            write(
                "channel-purpose",
                "C2",
                "conversations.setPurpose",
                json!({"channel":"C2","purpose":""}),
                false,
            ),
        ),
        (
            vec!["channel", "rename", "random", "Off Topic"],
            write(
                "channel-rename",
                "C2",
                "conversations.rename",
                json!({"channel":"C2","name":"off-topic"}),
                false,
            ),
        ),
        (
            vec!["channel", "archive", "random"],
            write(
                "channel-archive",
                "C2",
                "conversations.archive",
                json!({"channel":"C2"}),
                false,
            ),
        ),
        (
            vec![
                "status",
                "set",
                "Focus",
                "--emoji",
                ":books:",
                "--expires",
                &expires_s,
            ],
            write(
                "status-set",
                "U1",
                "users.profile.set",
                json!({ "profile": profile }),
                false,
            ),
        ),
        (
            vec!["status", "clear"],
            write(
                "status-clear",
                "U1",
                "users.profile.set",
                json!({ "profile": cleared }),
                false,
            ),
        ),
    ];
    for (args, expected) in cases {
        let mut a = vec!["--dry-run"];
        a.extend(args.iter().copied());
        assert_eq!(env.ok(&a), expected, "{args:?}");
        // The same invocation with --yes only flips `authorized`.
        let mut yes = vec!["--dry-run", "--yes"];
        yes.extend(args.iter().copied());
        let mut allowed = expected;
        allowed["authorized"] = json!(true);
        assert_eq!(env.ok(&yes), allowed, "{args:?} --yes");
    }
    assert!(!env.path("log.db").exists());
}

#[test]
fn dry_run_human_output_is_the_same_report_without_secrets() {
    let env = Env::new();
    let out = Command::new(assert_cmd::cargo::cargo_bin!("flek"))
        .arg("--db")
        .arg(env.path("fixture.db"))
        .args(["--team", "T1", "--dry-run", "--agent-config"])
        .arg(env.path("agent.toml"))
        .args(["channel", "topic", "random", "hello"])
        .env("HOME", env.dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let text = String::from_utf8(out.stdout).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["write"]["request"]["method"], "conversations.setTopic");
    assert_eq!(v["write"]["action"]["action"], "channel-topic");
    let lower = text.to_lowercase();
    for needle in ["xox", "cookie", "token"] {
        assert!(!lower.contains(needle));
    }
}

#[test]
fn ambiguous_person_exits_4_with_candidates_before_policy_or_session() {
    let env = Env::new();
    // "ali" is only a prefix of alice and alina: never guessed.
    let err = env.fail(&["channel", "invite", "random", "ali"], 4);
    assert_eq!(err["code"], "ambiguous");
    let ids: Vec<&str> = err["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"U2") && ids.contains(&"U3"), "{ids:?}");
    let err = env.fail(&["--dry-run", "channel", "kick", "random", "ali"], 4);
    assert_eq!(err["code"], "ambiguous");
    assert!(!env.path("log.db").exists());
}

#[test]
fn unresolvable_or_unsafe_requests_fail_offline_with_typed_errors() {
    let env = Env::new();
    assert_eq!(
        env.fail(&["channel", "invite", "random", "nobody"], 3)["code"],
        "not_found"
    );
    assert_eq!(
        env.fail(&["channel", "topic", "nowhere", "x"], 3)["code"],
        "not_found"
    );
    assert_eq!(env.fail(&["channel", "create", "bad!"], 2)["code"], "usage");
    assert_eq!(
        env.fail(&["channel", "kick", "random", "me"], 2)["code"],
        "usage"
    );
    assert_eq!(
        env.fail(&["status", "set", "x", "--emoji", "books"], 2)["code"],
        "usage"
    );
    assert_eq!(
        env.fail(&["status", "set", "x", "--expires", "yesterday"], 2)["code"],
        "usage"
    );
    // Invalid policy files are usage errors that never quote their content.
    env.config("writes_per_minute = 0\n");
    let err = env.fail(&["channel", "join", "lounge"], 2);
    assert_eq!(err["code"], "usage");
    assert!(!env.path("log.db").exists());
}
