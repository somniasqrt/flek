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
        json!({"schema_version":1,"conversation":channels["conversations"][0],"messages":[message],"next_cursor":null,"prev_cursor":null})
    );
    let mut hit = message.clone();
    hit["snippet"] = json!("**hello** offline");
    let search = success(&path, &["search", "hello"]);
    hit["before"] = json!([]);
    hit["after"] = json!([]);
    hit["source"] = json!("local");
    assert_eq!(search["results"], json!([hit]));
    assert_eq!(search["query"]["terms"], json!(["hello"]));
    assert_eq!(
        search["remote"],
        json!({"searched":false,"total":0,"pages":0,"error":null})
    );
    assert!(search["next_cursor"].is_null());
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

fn read_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let (dir, path) = fixture();
    let cache = dir.path().join("cached.txt");
    std::fs::write(&cache, "fake cached data").unwrap();
    let db = Store::open(&path).unwrap();
    db.with_conn(|c| {
        c.execute("INSERT INTO users(id,team_id,name,display_name,raw_json) VALUES ('U2','T1','alice','Alice',?1)",[json!({"profile":{"email":"alice@example.test","title":"Engineer","status_text":"Reading"}}).to_string()])?;
        c.execute("INSERT INTO users(id,team_id,name) VALUES ('U3','T1','alicia')",[])?;
        c.execute("UPDATE conversations SET last_read='1699999999.000000' WHERE id='C1'",[])?;
        messages::upsert_messages(c,"C1",&[
            MessageRow {ts:"1700000001.000001".into(),user_id:Some("U2".into()),text:"hello <@U1>".into(),raw_json:"{}".into(),files:vec![flek_core::store::files::FileRow {id:"F1".into(),name:Some("cached.txt".into()),mimetype:Some("text/plain".into()),raw_json:"{}".into(),..Default::default()}],..Default::default()},
            MessageRow {ts:"1700000002.000001".into(),thread_ts:Some("1700000001.000001".into()),user_id:Some("U2".into()),text:"reply".into(),raw_json:"{}".into(),..Default::default()}
        ])?;
        flek_core::store::files::set_local_path(c,"F1",&cache.to_string_lossy())?;Ok(())
    }).unwrap();
    (dir, path)
}
#[test]
fn agent_read_json_and_human_contracts_without_credentials() {
    let (dir, path) = read_fixture();
    let unread = success(&path, &["unread", "--messages", "1", "--include-unknown"]);
    assert_eq!(unread["failed"], 0);
    assert_eq!(unread["conversations"][0]["mentions"], 1);
    assert_eq!(
        unread["conversations"][0]["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(unread["unknown"].as_array().unwrap().len(), 2);
    let users = success(&path, &["users", "--query", "alice"]);
    assert_eq!(users["users"][0]["id"], "U2");
    assert_eq!(
        success(&path, &["user", "@alice"])["user"]["title"],
        "Engineer"
    );
    let info = success(&path, &["channel", "info", "general"]);
    assert!(info["members"].is_null());
    assert!(info["pins"].is_null());
    assert!(info["members_synced_at"].is_null());
    let thread = success(
        &path,
        &[
            "thread",
            "https://fake.slack.com/archives/C1/p1700000002000001",
        ],
    );
    assert_eq!(thread["root_ts"], "1700000001.000001");
    assert_eq!(thread["messages"].as_array().unwrap().len(), 2);
    let m = success(
        &path,
        &["message", "C1", "1700000001.000001", "--context", "1"],
    );
    assert_eq!(m["before"].as_array().unwrap().len(), 1);
    assert!(m["after"].as_array().unwrap().is_empty());
    assert_eq!(
        success(&path, &["mentions", "--since", "2020-01-01"])["messages"][0]["user_id"],
        "U2"
    );
    let files = success(
        &path,
        &[
            "files",
            "--channel",
            "general",
            "--from",
            "alice",
            "--type",
            "doc",
        ],
    );
    assert_eq!(files["files"][0]["id"], "F1");
    assert_eq!(files["files"][0]["conversation_id"], "C1");
    let dest = dir.path().join("copy.txt");
    let f = success(
        &path,
        &["file", "get", "F1", "--to", dest.to_str().unwrap()],
    );
    assert_eq!(f["file"]["id"], "F1");
    assert_eq!(std::fs::read_to_string(dest).unwrap(), "fake cached data");
    let p = success(&path, &["history", "C1", "--limit", "1"]);
    let cursor = p["next_cursor"].as_str().unwrap();
    assert_eq!(
        success(
            &path,
            &["history", "C1", "--limit", "1", "--cursor", cursor]
        )["messages"][0]["text"],
        "hello offline"
    );
    let p = success(
        &path,
        &[
            "history",
            "C1",
            "--around",
            "1700000001.000001",
            "--since",
            "2020-01-01",
            "--until",
            "2030-01-01",
        ],
    );
    assert_eq!(p["messages"][1]["target"], true);
    for args in [
        vec!["unread"],
        vec!["users"],
        vec!["user", "alice"],
        vec!["channel", "info", "C1"],
        vec!["thread", "C1", "1700000001.000001"],
        vec!["message", "C1", "1700000001.000001"],
        vec!["mentions", "--since", "2020-01-01"],
        vec!["files"],
        vec!["file", "get", "F1"],
        vec!["history", "C1"],
    ] {
        let result = Command::new(assert_cmd::cargo::cargo_bin!("flek"))
            .arg("--db")
            .arg(&path)
            .args(&args)
            .assert()
            .success()
            .get_output()
            .clone();
        assert!(!result.stdout.is_empty(), "{args:?}");
        assert!(!result.stdout.starts_with(b"{"), "{args:?}");
    }
}
#[test]
fn agent_read_errors_are_typed() {
    let (_dir, path) = read_fixture();
    for (args, code, kind) in [
        (vec!["user", "ali"], 4, "ambiguous"),
        (vec!["user", "nobody"], 3, "not_found"),
        (vec!["message", "C1", "1700000999.000001"], 3, "not_found"),
        (vec!["thread", "C1", "1700000999.000001"], 3, "not_found"),
        (vec!["channel", "info", "missing"], 3, "not_found"),
        (vec!["file", "get", "MISSING"], 3, "not_found"),
        (vec!["files", "--from", "ali"], 4, "ambiguous"),
        (vec!["files", "--since", "nonsense"], 2, "usage"),
        (vec!["mentions", "--since", "nonsense"], 2, "usage"),
        (vec!["history", "C1", "--cursor", "garbage"], 2, "usage"),
        (
            vec![
                "history",
                "C1",
                "--since",
                "2026-01-01",
                "--until",
                "2020-01-01",
            ],
            2,
            "usage",
        ),
        (vec!["history", "C1", "--around", "nonsense"], 2, "usage"),
        (vec!["message", "C1", "invalid"], 2, "usage"),
    ] {
        let result = command(&path)
            .args(&args)
            .assert()
            .code(code)
            .get_output()
            .clone();
        assert!(result.stdout.is_empty());
        let v: Value = serde_json::from_slice(&result.stderr).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["error"]["code"], kind, "{args:?}");
    }
}

#[test]
fn agent_search_context_cursor_filters_and_non_member_channels() {
    let (_dir, path) = read_fixture();
    let first = success(
        &path,
        &[
            "search",
            "hello in:#general from:alice",
            "--context",
            "1",
            "--limit",
            "1",
        ],
    );
    assert_eq!(first["query"]["from"], json!([{"id":"U2","label":"Alice"}]));
    assert_eq!(
        first["query"]["in_conversations"],
        json!([{"id":"C1","label":"general"}])
    );
    assert_eq!(first["results"][0]["before"][0]["text"], "hello offline");
    let first = success(&path, &["search", "hello", "--recent", "--limit", "1"]);
    let cursor = first["next_cursor"].as_str().unwrap();
    let next = success(
        &path,
        &[
            "search", "hello", "--recent", "--limit", "1", "--cursor", cursor,
        ],
    );
    assert_eq!(next["results"][0]["text"], "hello offline");
    assert!(next["next_cursor"].is_null());
    assert_eq!(
        success(&path, &["search", "hello", "--recent", "--offset", "1"])["results"],
        next["results"]
    );
    assert_eq!(
        success(
            &path,
            &[
                "search",
                "hello",
                "--since",
                "1700000001.000001",
                "--until",
                "1700000002.000001"
            ]
        )["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for (query, code, kind) in [
        ("hello in:missing", 3, "not_found"),
        ("hello in:gen", 4, "ambiguous"),
        ("hello from:nobody", 3, "not_found"),
    ] {
        let output = command(&path)
            .args(["search", query])
            .assert()
            .code(code)
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], kind);
    }
    for args in [
        vec!["search", "hello", "--cursor", "bad"],
        vec![
            "search", "hello", "--cursor", cursor, "--recent", "--offset", "1",
        ],
    ] {
        command(&path).args(args).assert().code(2);
    }
    let db = Store::open(&path).unwrap();
    db.with_conn(|c| {
        c.execute(
            "INSERT INTO conversations(id, team_id, kind, name, label, is_member)
             VALUES ('CSEARCH', 'T1', 'public_channel', 'discovered', 'discovered', 0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let normal = success(&path, &["channels"]);
    assert_eq!(normal["conversations"].as_array().unwrap().len(), 3);
    let all = success(&path, &["channels", "--include-non-member"]);
    assert_eq!(all["conversations"].as_array().unwrap().len(), 4);
    assert!(all["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == "CSEARCH" && c["is_member"] == false));
    let output = Command::new(assert_cmd::cargo::cargo_bin!("flek"))
        .arg("--db")
        .arg(&path)
        .args(["--team", "T1", "channels"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(!String::from_utf8(output.stdout)
        .unwrap()
        .contains("discovered"));
}
