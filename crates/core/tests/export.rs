//! Offline Obsidian export on a temporary SQLite archive with invented data.
use flek_core::api::{Conversation, User};
use flek_core::export::{self, Options, Summary};
use flek_core::store::{
    directory, files,
    messages::{self, MessageRow},
    Store,
};
use serde_json::json;
use std::{fs, path::Path};

struct Fixture {
    _dir: tempfile::TempDir,
    db: std::path::PathBuf,
    vault: std::path::PathBuf,
    cache: std::path::PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("archive.db");
    let cache = dir.path().join("cache");
    fs::create_dir(&cache).unwrap();
    fs::write(cache.join("F1.txt"), b"hello").unwrap();
    let store = Store::open(&db).unwrap();
    store
        .with_conn(|c| {
            directory::upsert_team(c, "T1", "Fake Team", "https://fake.slack.com/", "U1")?;
            let raw = json!({"id":"U1","name":"alice","real_name":"Alice Example","profile":{"display_name":"Ali"}});
            let mut user: User = serde_json::from_value(raw.clone())?;
            user.raw = raw;
            directory::upsert_users(c, "T1", &[user])?;
            let convs: Vec<Conversation> = serde_json::from_value(json!([
                {"id":"C1","name":"general","is_channel":true},
                {"id":"C2","name":"random","is_channel":true},
                {"id":"C3","name":"empty","is_channel":true}
            ]))?;
            directory::upsert_conversations(c, "T1", "U0", &convs)?;
            messages::upsert_messages(
                c,
                "C1",
                &[
                    MessageRow {
                        ts: "1700000000.000100".into(),
                        thread_ts: Some("1700000000.000100".into()),
                        user_id: Some("U1".into()),
                        text: "Hi <@U1> see <#C2|old> and <https://example.test/a|the site> :tada: a|b [x]".into(),
                        reply_count: 1,
                        edited_ts: Some("1700000001.000000".into()),
                        raw_json: "{}".into(),
                        reactions: vec![("wave".into(), "U1".into()), ("wave".into(), "U9".into())],
                        files: vec![files::FileRow {
                            id: "F1".into(),
                            name: Some("../evil#name.txt".into()),
                            mimetype: Some("text/plain".into()),
                            permalink: Some("https://fake.slack.com/files/U1/F1/x".into()),
                            raw_json: "{}".into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    MessageRow {
                        ts: "1700000100.000200".into(),
                        thread_ts: Some("1700000000.000100".into()),
                        user_id: Some("U1".into()),
                        text: "Reply with xoxc-123-secret token".into(),
                        raw_json: "{}".into(),
                        ..Default::default()
                    },
                    MessageRow {
                        ts: "1700000200.000000".into(),
                        text: "Deleted one".into(),
                        deleted: true,
                        raw_json: "{}".into(),
                        ..Default::default()
                    },
                    MessageRow {
                        ts: "1700086400.000300".into(),
                        hidden_by_limit: true,
                        raw_json: "{}".into(),
                        ..Default::default()
                    },
                ],
            )?;
            messages::upsert_messages(
                c,
                "C2",
                &[MessageRow {
                    ts: "1700000300.000000".into(),
                    text: "Other channel".into(),
                    raw_json: "{}".into(),
                    ..Default::default()
                }],
            )?;
            files::set_local_path(c, "F1", cache.join("F1.txt").to_str().unwrap())?;
            Ok(())
        })
        .unwrap();
    drop(store);
    Fixture {
        vault: dir.path().join("vault"),
        _dir: dir,
        db,
        cache,
    }
}

fn run(f: &Fixture, options: &Options) -> flek_core::Result<Summary> {
    let store = Store::open_read_only(&f.db)?;
    store.with_read(|c| export::obsidian(c, "T1", &f.vault, &f.cache, options))
}

fn all_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

const TEAM_DIR: &str = "Flek/team-e2398779d5a199b7adb4917fab0cbe5660e3c52f";
const GENERAL: &str = "conversations/conversation-28c27031fe7162d732a1c2e209a40bbfcb5fef90";
const RANDOM: &str = "conversations/conversation-cb7d29f43f4a81ff1f3a1202c80293337faa7a0a";
const EMPTY: &str = "conversations/conversation-0371438f82b01efb31e37cd12a4a6b6c819b0e71";
const ATTACHMENT: &str = "attachments/88bfad9cfffeafd299a44d4daf979d57419a2621-aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d/_evil_name.txt";

fn read(f: &Fixture, relative: &str) -> String {
    fs::read_to_string(f.vault.join(TEAM_DIR).join(relative)).unwrap()
}

fn mtimes(f: &Fixture) -> Vec<(std::path::PathBuf, std::time::SystemTime)> {
    all_files(&f.vault)
        .into_iter()
        .map(|p| {
            let t = fs::metadata(&p).unwrap().modified().unwrap();
            (p, t)
        })
        .collect()
}

#[test]
fn general_note_matches_golden_markdown() {
    let f = fixture();
    let s = run(&f, &Options::default()).unwrap();
    assert_eq!(
        (s.conversations, s.messages, s.links, s.files),
        (3, 4, 1, 1)
    );
    assert_eq!((s.notes_written, s.notes_unchanged), (5, 0));
    assert_eq!(
        (
            s.attachments_copied,
            s.attachments_unchanged,
            s.attachments_unavailable
        ),
        (1, 0, 0)
    );
    let golden = r#"# general

Conversation: C1 (public_channel)

[[{T}/index|Index]] · [[{T}/links-and-files|Links and files]]

## 2023-11-14

**22:13:20 — Ali** (edited) ^m-136098d933cff0dc8df28922d49b2c24a3d3b93b

Hi @Ali see [[{T}/{RANDOM}|#random]] and [the site](<https://example.test/a>) :tada: a&#124;b &#91;x&#93;

Thread: 1 replies reported in the archive.

Reactions: :wave: ×2 (Ali, U9)

File at 1700000000.000100: [[{T}/{ATT}|../evil#name.txt]]

**22:15:00 — Ali** ^m-0091b4f85c70a0d9262c7b3ad33297cc2e3d0e1c

Reply to [[{T}/{GENERAL}#^m-136098d933cff0dc8df28922d49b2c24a3d3b93b|general · 1700000000.000100]]

> Reply with [redacted] token

## 2023-11-15

**22:13:20 — Unknown author** (hidden by limit) ^m-24a3c547b7fcf6214dc42a19b368c360edd091a7

_Text hidden by the workspace history limit and not stored locally._

"#
    .replace("{T}", TEAM_DIR)
    .replace("{RANDOM}", RANDOM)
    .replace("{GENERAL}", GENERAL)
    .replace("{ATT}", ATTACHMENT);
    assert_eq!(read(&f, &format!("{GENERAL}.md")), golden);
    // Deleted messages are omitted; other conversations keep their own notes.
    assert!(!read(&f, &format!("{GENERAL}.md")).contains("Deleted one"));
    assert!(read(&f, &format!("{RANDOM}.md")).contains("Other channel"));
    assert!(read(&f, &format!("{EMPTY}.md")).starts_with("# empty\n"));
    assert_eq!(read(&f, ATTACHMENT), "hello");

    let catalog = read(&f, "links-and-files.md");
    assert!(catalog.contains("[https://example.test/a](<https://example.test/a>)"));
    assert!(catalog.contains("| cached copy |"));
    assert!(catalog.contains("<https://fake.slack.com/files/U1/F1/x>"));
    let index = read(&f, "index.md");
    assert!(index.starts_with("# Fake Team\n"));
    assert!(index.contains("general]] — 3 messages"));
    assert!(index.contains("empty]] — 0 messages"));

    // Nothing credential-shaped or outside the Flek directory is written.
    for path in all_files(&f.vault) {
        assert!(path.starts_with(f.vault.join(TEAM_DIR)), "{path:?}");
        if path.extension().is_some_and(|e| e == "md") {
            assert!(!fs::read_to_string(&path).unwrap().contains("xoxc-"));
        }
    }
}

#[test]
fn reexport_rewrites_nothing_and_detects_changes() {
    let f = fixture();
    run(&f, &Options::default()).unwrap();
    let before = mtimes(&f);
    std::thread::sleep(std::time::Duration::from_millis(30));
    let s = run(&f, &Options::default()).unwrap();
    assert_eq!((s.notes_written, s.notes_unchanged), (0, 5));
    assert_eq!((s.attachments_copied, s.attachments_unchanged), (0, 1));
    assert_eq!(before, mtimes(&f));

    // Editing one stored message rewrites only that conversation's note.
    let store = Store::open(&f.db).unwrap();
    store
        .with_conn(|c| {
            messages::upsert_messages(
                c,
                "C2",
                &[MessageRow {
                    ts: "1700000300.000000".into(),
                    text: "Edited elsewhere".into(),
                    raw_json: "{}".into(),
                    ..Default::default()
                }],
            )
        })
        .unwrap();
    drop(store);
    let s = run(&f, &Options::default()).unwrap();
    assert_eq!((s.notes_written, s.notes_unchanged), (1, 4));
    assert!(read(&f, &format!("{RANDOM}.md")).contains("Edited elsewhere"));
    let after = mtimes(&f);
    for (path, time) in before {
        if !path.ends_with(format!("{RANDOM}.md")) {
            assert!(after.contains(&(path, time)));
        }
    }
}

#[test]
fn channel_filter_exports_one_conversation_beside_the_full_archive() {
    let f = fixture();
    let s = run(
        &f,
        &Options {
            channel: Some("#random".into()),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!((s.conversations, s.messages), (1, 1));
    assert!(f.vault.join(TEAM_DIR).join(format!("{RANDOM}.md")).exists());
    assert!(!f
        .vault
        .join(TEAM_DIR)
        .join(format!("{GENERAL}.md"))
        .exists());
    assert!(!f.vault.join(TEAM_DIR).join("index.md").exists());
    let names: Vec<_> = fs::read_dir(f.vault.join(TEAM_DIR))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().any(|n| n.starts_with("index-")));
    assert!(names.iter().any(|n| n.starts_with("links-and-files-")));
    // An unselected channel mention stays readable but is not linked.
    let _ = run(
        &f,
        &Options {
            channel: Some("#general".into()),
            ..Options::default()
        },
    )
    .unwrap();
    let general = read(&f, &format!("{GENERAL}.md"));
    assert!(general.contains("#random"));
    assert!(!general.contains(RANDOM));

    let missing = run(
        &f,
        &Options {
            channel: Some("#nope".into()),
            ..Options::default()
        },
    );
    assert!(missing.is_err());
}

#[test]
fn time_bounds_go_to_a_separate_projection() {
    let f = fixture();
    let s = run(
        &f,
        &Options {
            since: Some("1700000050.000000".into()),
            until: Some("1700086400.000000".into()),
            ..Options::default()
        },
    )
    .unwrap();
    // The reply and the other channel's message are in range; the root, the
    // deleted message and the hidden one are not.
    assert_eq!((s.conversations, s.messages), (3, 2));
    let ranges = f
        .vault
        .canonicalize()
        .unwrap()
        .join(TEAM_DIR)
        .join("ranges");
    assert!(s.directory.starts_with(ranges));
    assert!(!f.vault.join(TEAM_DIR).join("index.md").exists());
    let note = fs::read_to_string(s.directory.join(format!("{GENERAL}.md"))).unwrap();
    assert!(note.contains("(root not in this export)"));
    assert!(note.contains("Reply with"));
    assert!(!note.contains("hidden by limit"));

    for (since, until) in [("1700000100", "1700000100"), ("1700000200", "1700000100")] {
        let e = run(
            &f,
            &Options {
                since: Some(since.into()),
                until: Some(until.into()),
                ..Options::default()
            },
        );
        assert!(e.is_err());
    }
}

#[test]
fn files_outside_the_cache_are_never_copied() {
    let f = fixture();
    let outside = f._dir.path().join("secret.txt");
    fs::write(&outside, b"private").unwrap();
    let store = Store::open(&f.db).unwrap();
    store
        .with_conn(|c| files::set_local_path(c, "F1", outside.to_str().unwrap()))
        .unwrap();
    drop(store);
    let s = run(&f, &Options::default()).unwrap();
    assert_eq!((s.attachments_copied, s.attachments_unavailable), (0, 1));
    assert!(!f
        .vault
        .join(TEAM_DIR)
        .join("attachments")
        .read_dir()
        .unwrap()
        .any(|_| true));
    assert!(read(&f, "links-and-files.md").contains("not cached / unavailable"));
}

#[test]
fn read_only_open_never_creates_or_changes_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("none.db");
    assert!(Store::open_read_only(&missing).is_err());
    assert!(!missing.exists());

    let f = fixture();
    let store = Store::open_read_only(&f.db).unwrap();
    let write = store.with_read(|c| {
        c.execute("DELETE FROM messages", [])?;
        Ok(())
    });
    assert!(write.is_err());
}
