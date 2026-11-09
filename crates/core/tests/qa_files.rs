//! QA: file cache paths, downloads, hidden and shared files.

mod qa_common;

use std::path::{Component, Path, PathBuf};

use qa_common::*;
use serde_json::json;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

use flek_core::files::cache_path;
use flek_core::store::messages;
use flek_core::sync::files::download_files;

/// The path is a file strictly inside `<cache>/<something>/`, with no `..`
/// or `.` components after the cache dir.
fn contained(cache: &Path, p: &Path) -> bool {
    let Ok(rest) = p.strip_prefix(cache) else {
        return false;
    };
    let comps: Vec<Component> = rest.components().collect();
    comps.len() == 2 && comps.iter().all(|c| matches!(c, Component::Normal(_)))
}

// FINDING: a file id of "." or ".." is not neutralised: ("..", None) maps to
// `<cache>/..` (outside the cache), and ("..", "x.txt") / (".", "x.txt") drop
// the per-id directory, so different files with the same name collide.
#[test]
fn cache_path_never_escapes_or_points_at_a_directory() {
    let cache = PathBuf::from("/tmp/flek-cache");
    let cases: [(&str, Option<&str>); 9] = [
        ("F1", Some("../../etc/passwd")),
        ("F1", Some("..")),
        ("F1", Some(".")),
        ("F1", Some("")),
        ("..", None),
        ("..", Some("x.txt")),
        (".", Some("x.txt")),
        ("../F2", Some("x.txt")),
        ("F1", Some("..\\..\\x")),
    ];
    let bad: Vec<String> = cases
        .iter()
        .map(|(id, name)| ((id, name), cache_path(&cache, id, *name)))
        .filter(|(_, p)| !contained(&cache, p))
        .map(|(c, p)| format!("{c:?} -> {}", p.display()))
        .collect();
    assert!(bad.is_empty(), "escaping paths:\n{}", bad.join("\n"));
}

#[test]
fn cache_path_is_keyed_by_id() {
    let cache = PathBuf::from("/tmp/flek-cache");
    assert_ne!(
        cache_path(&cache, "F1", Some("a.txt")),
        cache_path(&cache, "F2", Some("a.txt"))
    );
    assert_eq!(
        cache_path(&cache, "F1", Some("a.txt")),
        cache_path(&cache, "F1", Some("a.txt"))
    );
}

#[tokio::test]
async fn downloads_stay_in_cache_and_happen_once() {
    let server = MockServer::start().await;
    let base = server.uri();
    let store = store();
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "3.000000", "user": "U1", "text": "same file again",
             "files": [{"id": "F1", "name": "../../escape.txt", "mimetype": "text/plain", "size": 4,
                        "url_private_download": format!("{base}/dl/F1")}]},
            {"ts": "2.000000", "user": "U1", "text": "evil names",
             "files": [{"id": "F1", "name": "../../escape.txt", "mimetype": "text/plain", "size": 4,
                        "url_private_download": format!("{base}/dl/F1")},
                       {"id": "..", "name": "..", "mimetype": "text/plain", "size": 4,
                        "url_private_download": format!("{base}/dl/dotdot")}]}
        ])))
        .mount(&server)
        .await;
    Mock::given(path("/dl/F1"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("data", "text/plain"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/dl/dotdot"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("data", "text/plain"))
        .mount(&server)
        .await;
    let c = client(&server);
    backfill(&c, &store, &Default::default()).await;

    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("cache");
    std::fs::create_dir(&cache).unwrap();
    let _ = download_files(&c, &store, &cache, None, |_, _| {}).await;
    let _ = download_files(&c, &store, &cache, None, |_, _| {}).await;

    // Nothing may land outside the cache dir.
    let outside: Vec<String> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "cache")
        .collect();
    assert!(outside.is_empty(), "written outside cache: {outside:?}");
    let local: Option<String> = store
        .with_conn(|c| {
            Ok(
                c.query_row("SELECT local_path FROM files WHERE id = 'F1'", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    let local = PathBuf::from(local.expect("F1 downloaded"));
    assert!(local.starts_with(&cache) && contained(&cache, &local));
    // Shared by two messages: one files row, two links.
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM message_files WHERE file_id = 'F1'"
        ),
        2
    );
}

#[tokio::test]
async fn deleting_one_message_keeps_shared_file_on_the_other() {
    let store = live_store();
    let server = MockServer::start().await;
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "3.000000", "user": "U1", "text": "again",
             "files": [{"id": "F1", "name": "a.txt", "url_private_download": "https://files.slack.com/F1"}]},
            {"ts": "2.000000", "user": "U1", "text": "first",
             "files": [{"id": "F1", "name": "a.txt", "url_private_download": "https://files.slack.com/F1"}]}
        ])))
        .mount(&server)
        .await;
    backfill(&client(&server), &store, &full()).await;
    ev(
        &store,
        json!({"type": "message", "subtype": "message_deleted", "channel": "C1", "ts": "30.000000",
               "deleted_ts": "2.000000"}),
    )
    .await;
    let other = store
        .with_conn(|c| messages::message(c, "C1", "3.000000"))
        .unwrap()
        .unwrap();
    assert_eq!(other.files.len(), 1);
    assert_eq!(other.files[0].id, "F1");
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM files WHERE id = 'F1'"),
        1
    );
}

#[tokio::test]
async fn hidden_file_recovers_when_a_visible_copy_arrives() {
    let server = MockServer::start().await;
    let store = store();
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "2.000000", "user": "U1", "text": "",
             "files": [{"id": "F1", "mode": "hidden_by_limit"}]}
        ])))
        .mount(&server)
        .await;
    let c = client(&server);
    backfill(&c, &store, &Default::default()).await;
    server.reset().await;
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "2.000000", "user": "U1", "text": "the report",
             "files": [{"id": "F1", "name": "report.pdf", "url_private_download": "https://files.slack.com/F1"}]}
        ])))
        .mount(&server)
        .await;
    backfill(&c, &store, &full()).await;
    let (name, hidden): (Option<String>, bool) = store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT name, hidden_by_limit FROM files WHERE id = 'F1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(name.as_deref(), Some("report.pdf"));
    assert!(!hidden);
    assert!(!stored(&store, "2.000000").unwrap().hidden);
}
