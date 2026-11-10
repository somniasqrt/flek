//! Only offline commands, with a temporary DB and invented identities.
use assert_cmd::Command;
use flek_core::api::Conversation;
use flek_core::store::{
    directory,
    messages::{self, MessageRow},
    Store,
};
use serde_json::{json, Value};

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.db");
    let db = Store::open(&path).unwrap();
    db.with_conn(|c| {
        directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U1")?;
        let convs: Vec<Conversation> = serde_json::from_value(json!([
            {"id":"C1","name":"general","is_channel":true},
            {"id":"C2","name":"general-help","is_channel":true},
            {"id":"C3","name":"general-news","is_channel":true}
        ]))?;
        directory::upsert_conversations(c, "T1", "U1", &convs)?;
        messages::upsert_messages(
            c,
            "C1",
            &[MessageRow {
                ts: "1700000000.123456".into(),
                text: "hello offline".into(),
                raw_json: "{}".into(),
                ..Default::default()
            }],
        )?;
        Ok(())
    })
    .unwrap();
    (dir, path)
}
fn command(path: &std::path::Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("flek"));
    cmd.arg("--db")
        .arg(path)
        .args(["--team", "T1", "--json"])
        .env("RUST_LOG", "trace");
    cmd
}
fn success(path: &std::path::Path, args: &[&str]) -> Value {
    let result = command(path)
        .args(args)
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(
        result.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout.iter().filter(|&&b| b == b'\n').count(), 1);
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert!(value.is_object());
    value
}
fn conversation(id: &str, name: &str, latest: Value) -> Value {
    json!({"id":id,"name":name,"label":name,"kind":"channel","is_archived":false,"is_member":true,"topic":null,"purpose":null,"unread_count":null,"last_read":null,"latest_ts":latest,"synced_at":null,"history_complete":false})
}
#[test]
fn offline_json_contracts() {
    let (_dir, path) = fixture();
    let channels = success(&path, &["channels"]);
    assert_eq!(
        channels,
        json!({"schema_version":1,"conversations":[conversation("C1","general",json!("1700000000.123456")),conversation("C2","general-help",Value::Null),conversation("C3","general-news",Value::Null)]})
    );
    let message = json!({"conversation_id":"C1","conversation":"general","ts":"1700000000.123456","datetime":"2023-11-14T22:13:20.123456Z","thread_ts":null,"reply_count":0,"user_id":null,"user":null,"text":"hello offline","text_resolved":"hello offline","edited":false,"deleted":false,"hidden_by_limit":false,"files":[],"reactions":[],"permalink":"https://fake.slack.com/archives/C1/p1700000000123456"});
    assert_eq!(
        success(&path, &["history", "#general"]),
        json!({"schema_version":1,"conversation":channels["conversations"][0],"messages":[message]})
    );
    let mut hit = message.clone();
    hit["snippet"] = json!("**hello** offline");
    assert_eq!(
        success(&path, &["search", "hello"]),
        json!({"schema_version":1,"results":[hit]})
    );
    assert!(
        success(&path, &["db"])["db_schema_version"]
            .as_u64()
            .unwrap()
            > 1
    );
    assert_eq!(
        success(&path, &["history", "general", "--limit", "0"])["messages"],
        json!([])
    );
    assert_eq!(success(&path, &["search", "absent"])["results"], json!([]));
    let output = Command::new(assert_cmd::cargo::cargo_bin!("flek"))
        .arg("--db")
        .arg(&path)
        .args(["channels", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        channels
    );
}
#[test]
fn errors_are_one_json_object_on_stderr_with_typed_status() {
    let (_dir, path) = fixture();
    for (reference, code, name) in [("missing", 3, "not_found"), ("gen", 4, "ambiguous")] {
        let output = command(&path)
            .args(["history", reference])
            .assert()
            .code(code)
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr.iter().filter(|&&b| b == b'\n').count(), 1);
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["schema_version"], 1);
        assert_eq!(error["ok"], false);
        assert_eq!(error["error"]["code"], name);
        assert!(error["error"]["message"].is_string());
        if code == 4 {
            assert_eq!(
                error["error"]["candidates"],
                json!([
                    {"id":"C1","label":"general","kind":"channel"},{"id":"C2","label":"general-help","kind":"channel"},{"id":"C3","label":"general-news","kind":"channel"}
                ])
            );
        }
    }
    let output = command(&path)
        .args(["history"])
        .assert()
        .code(2)
        .get_output()
        .clone();
    assert!(output.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "usage"
    );
    let output = Command::new(assert_cmd::cargo::cargo_bin!("flek"))
        .arg("--db")
        .arg(path)
        .args(["history", "gen", "--team", "T1"])
        .assert()
        .code(4)
        .get_output()
        .clone();
    let human = String::from_utf8(output.stderr).unwrap();
    for candidate in ["C1", "general", "C2", "general-help", "C3", "general-news"] {
        assert!(human.contains(candidate));
    }
}

#[test]
fn failure_messages_preserve_context_and_clap_diagnostics() {
    let (_dir, path) = fixture();
    let expected = "no conversation matches \"missing\"; try `flek channels`";
    let json_output = command(&path)
        .args(["history", "missing"])
        .assert()
        .code(3)
        .get_output()
        .clone();
    assert_eq!(
        serde_json::from_slice::<Value>(&json_output.stderr).unwrap()["error"]["message"],
        expected
    );
    let human = Command::new(assert_cmd::cargo::cargo_bin!("flek"))
        .arg("--db")
        .arg(&path)
        .args(["--team", "T1", "history", "missing"])
        .assert()
        .code(3)
        .get_output()
        .clone();
    assert_eq!(String::from_utf8(human.stderr).unwrap().trim(), expected);
    for (args, first_line) in [
        (
            vec!["channels", "--foo"],
            "error: unexpected argument '--foo' found",
        ),
        (
            vec!["history"],
            "error: the following required arguments were not provided:",
        ),
    ] {
        let output = command(&path)
            .args(args)
            .env("CLICOLOR_FORCE", "1")
            .assert()
            .code(2)
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "usage");
        assert_eq!(error["error"]["message"], first_line);
        assert!(!error["error"]["message"]
            .as_str()
            .unwrap()
            .contains('\u{1b}'));
    }
    assert_eq!(
        success(&path, &["history", "general-h"])["conversation"]["id"],
        "C2"
    );
}

#[test]
fn empty_database_explains_how_to_populate_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.db");
    for args in [
        vec!["history", "missing"],
        vec!["channels"],
        vec!["search", "hello"],
    ] {
        let output = Command::new(assert_cmd::cargo::cargo_bin!("flek"))
            .arg("--db")
            .arg(&path)
            .arg("--json")
            .args(args)
            .assert()
            .code(3)
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "not_found");
        assert_eq!(
            error["error"]["message"],
            "no stored team; run `flek auth import` and `flek sync`"
        );
    }
}
