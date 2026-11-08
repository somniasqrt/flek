//! File metadata and cache downloads.

use std::time::Duration;

use flek_core::api::Client;
use flek_core::store::Store;
use flek_core::sync::backfill::{backfill_conversation, RateLimiter};
use flek_core::sync::files::download_files;
use flek_core::sync::SyncOptions;
use flek_core::{files, Secret};
use serde_json::json;
use wiremock::matchers::{header, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
                 INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');",
            )?;
            Ok(())
        })
        .unwrap();
    store
}

#[tokio::test]
async fn stores_metadata_and_downloads_files() {
    let server = MockServer::start().await;
    let base = server.uri();
    let client =
        Client::with_base_url(Secret::new("xoxc-fake"), Secret::new("xoxd-fake"), &base).unwrap();
    let store = store();
    let cache = tempfile::tempdir().unwrap();

    let file = |id: &str, name: &str, extra: serde_json::Value| {
        let mut f = json!({
            "id": id, "name": name, "mimetype": "application/pdf", "size": 5,
            "url_private_download": format!("{base}/download/{id}/{name}"),
            "permalink": format!("https://fake.slack.com/files/{id}")
        });
        f.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        f
    };
    Mock::given(path("/conversations.history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "has_more": false, "messages": [
            {"ts": "3.000000", "user": "U1", "text": "", "files": [{"id": "F3", "mode": "hidden_by_limit"}]},
            {"ts": "2.000000", "user": "U1", "text": "two files", "files": [
                file("F1", "report.pdf", json!({})),
                file("F2", "login.pdf", json!({})),
                file("F4", "big.pdf", json!({"size": 10_000_000})),
                {"id": "F5", "name": "doc", "is_external": true, "url_private_download": "https://drive.example/doc"}
            ]},
            {"ts": "1.000000", "user": "U1", "text": "evil", "files": [
                {"id": "F6", "name": "x", "url_private_download": "https://evil.example/x"}
            ]}
        ]})))
        .mount(&server)
        .await;
    Mock::given(path("/download/F1/report.pdf"))
        .and(header("authorization", "Bearer xoxc-fake"))
        .and(header("cookie", "d=xoxd-fake"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"%PDF-".to_vec(), "application/pdf"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/download/F2/login.pdf"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("<html>sign in</html>", "text/html; charset=utf-8"),
        )
        .mount(&server)
        .await;

    backfill_conversation(
        &client,
        &store,
        &mut RateLimiter::new(Duration::ZERO),
        "C1",
        &SyncOptions::default(),
    )
    .await
    .unwrap();

    let (files_n, links, hidden): (i64, i64, bool) = store
        .with_conn(|c| {
            Ok((
                c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?,
                c.query_row(
                    "SELECT COUNT(*) FROM message_files WHERE ts = '2.000000'",
                    [],
                    |r| r.get(0),
                )?,
                c.query_row(
                    "SELECT hidden_by_limit FROM files WHERE id = 'F3'",
                    [],
                    |r| r.get(0),
                )?,
            ))
        })
        .unwrap();
    assert_eq!((files_n, links, hidden), (6, 4, true));

    let mut calls = 0;
    let stats = download_files(&client, &store, cache.path(), Some(1_000_000), |_, _| {
        calls += 1
    })
    .await
    .unwrap();
    // F1 ok; F2 is HTML; F6 is on a foreign host; F3 hidden, F4 too big, F5 external are skipped.
    assert_eq!((stats.downloaded, stats.cached, stats.failed), (1, 0, 2));
    assert_eq!(calls, 3);

    let dest = files::cache_path(cache.path(), "F1", Some("report.pdf"));
    assert_eq!(std::fs::read(&dest).unwrap(), b"%PDF-");
    let local: String = store
        .with_conn(|c| {
            Ok(
                c.query_row("SELECT local_path FROM files WHERE id = 'F1'", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(local, dest.to_string_lossy());

    // Second run: F1 already cached, no new request (expect(1) above).
    let stats = download_files(&client, &store, cache.path(), Some(1_000_000), |_, _| {})
        .await
        .unwrap();
    assert_eq!((stats.downloaded, stats.cached), (0, 1));
}

#[test]
fn cache_paths_are_sanitized() {
    let root = std::path::Path::new("/c");
    assert_eq!(
        files::cache_path(root, "F1", Some("../../etc/passwd")),
        root.join("F1/_.._etc_passwd")
    );
    assert_eq!(files::cache_path(root, "F1", Some("")), root.join("F1/F1"));
    assert_eq!(files::cache_path(root, "F1", None), root.join("F1/F1"));
}
