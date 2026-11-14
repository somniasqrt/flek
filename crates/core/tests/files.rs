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
                file("F1", "report.pdf", json!({
                    "thumb_pdf": format!("{base}/thumb/F1.png"), "thumb_pdf_w": 910, "thumb_pdf_h": 1286
                })),
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
    Mock::given(path("/thumb/F1.png"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"\x89PNG".to_vec(), "image/png"))
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
    // Then F1's PDF preview.
    assert_eq!(
        (stats.downloaded, stats.cached, stats.failed, stats.previews),
        (1, 0, 2, 1)
    );
    assert_eq!(calls, 4);
    let preview = files::preview_path(cache.path(), "F1");
    assert_eq!(std::fs::read(&preview).unwrap(), b"\x89PNG");
    let page = store
        .with_read(|c| flek_core::store::messages::page(c, "C1", None, 10))
        .unwrap();
    let f1 = page
        .iter()
        .flat_map(|m| &m.files)
        .find(|f| f.id == "F1")
        .unwrap();
    assert_eq!(
        f1.preview_path.as_deref(),
        Some(&*preview.to_string_lossy())
    );
    assert_eq!(
        (f1.preview_width, f1.preview_height),
        (Some(910), Some(1286))
    );

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

    // Second run: F1 and its preview already cached, no new request (expect(1) above).
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

#[tokio::test]
async fn download_retries_after_429() {
    let server = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    Mock::given(path("/f/a.txt"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/f/a.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("ok", "text/plain"))
        .with_priority(2)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.txt");
    client
        .download(&format!("{}/f/a.txt", server.uri()), &dest, false)
        .await
        .unwrap();
    #[cfg(target_os = "macos")]
    {
        let quarantine = xattr::get(&dest, "com.apple.quarantine")
            .unwrap()
            .expect("download must be quarantined");
        let value = String::from_utf8(quarantine).unwrap();
        let parts: Vec<_> = value.split(';').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "0081");
        assert!(u64::from_str_radix(parts[1], 16).unwrap() > 0);
        assert_eq!(parts[2], "Flek");
        uuid::Uuid::parse_str(parts[3]).unwrap();
    }
    assert_eq!(std::fs::read_to_string(dest).unwrap(), "ok");
}

#[test]
fn files_of_messages_stored_before_m1_6_are_recovered() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flek.db");
    {
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        flek_core::store::migrations::migrate_to(&mut conn, 9).unwrap();
        let raw = json!({"ts": "1.000000", "files": [
            {"id": "F1", "name": "shot.png", "mimetype": "image/png", "filetype": "png",
             "size": 10, "url_private": "https://files.example/F1", "mode": "hosted",
             "original_w": 800, "original_h": 600},
            {"id": "F2", "mode": "hidden_by_limit"},
            {"mode": "tombstone"}
        ]})
        .to_string();
        conn.execute_batch(
            "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
             INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (conversation_id, ts, text, raw_json) VALUES ('C1', '1.000000', '', ?1)",
            [raw],
        )
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let page = store
        .with_read(|c| flek_core::store::messages::page(c, "C1", None, 10))
        .unwrap();
    let files = &page[0].files;
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].id, "F1");
    assert_eq!(files[0].mimetype.as_deref(), Some("image/png"));
    assert_eq!((files[0].width, files[0].height), (Some(800), Some(600)));
    assert!(files[1].hidden_by_limit);
}

#[test]
fn bidi_controls_cannot_disguise_cached_extension() {
    let root = std::path::Path::new("/cache");
    assert_eq!(
        files::cache_path(root, "F1", Some("Розклад\u{202e}fdp.command")),
        root.join("F1/Розкладfdp.command")
    );
}

#[tokio::test]
async fn stored_paths_outside_cache_are_refused() {
    let store = store();
    let cache = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    let path = outside.path().to_string_lossy().into_owned();
    store
        .with_conn(|c| {
            c.execute(
                "INSERT INTO files (id, name, local_path) VALUES ('FOUT', 'x.pdf', ?1)",
                [&path],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(files::cached_open_target(&store, cache.path(), "FOUT")
        .await
        .is_err());
    assert!(files::stored_cached_path(&store, cache.path(), "FOUT")
        .await
        .is_err());
}

#[test]
fn file_views_preserve_real_name_and_strip_bidi_for_display() {
    let store = store();
    store.with_conn(|c| {
        c.execute_batch("INSERT INTO messages (conversation_id, ts, text) VALUES ('C1', '1.000000', 'synthetic');
            INSERT INTO files (id, name, title, mimetype) VALUES ('FMASK', 'x.terminal', 'Розклад.pdf', 'application/pdf');
            INSERT INTO message_files (conversation_id, ts, file_id) VALUES ('C1', '1.000000', 'FMASK');")?;
        c.execute("INSERT INTO files (id, name, title, mimetype) VALUES ('FBIDI', ?1, 'title', 'application/pdf')", ["Розклад\u{202e}fdp.command"])?;
        c.execute_batch("INSERT INTO message_files (conversation_id, ts, file_id) VALUES ('C1', '1.000000', 'FBIDI');")?;
        Ok(())
    }).unwrap();
    let rows = store
        .with_read(|c| flek_core::store::messages::page(c, "C1", None, 10))
        .unwrap();
    let files = serde_json::to_value(&rows[0].files).unwrap();
    let masked = files
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "FMASK")
        .unwrap();
    assert_eq!(masked["name"], "x.terminal");
    assert_eq!(masked["title"], "Розклад.pdf");
    assert_eq!(masked["can_open"], false);
    let bidi = files
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "FBIDI")
        .unwrap();
    assert_eq!(bidi["name"], "Розкладfdp.command");
    assert_eq!(bidi["can_open"], false);
    let browser = store
        .with_read(|c| {
            flek_core::reads::files(
                c,
                "T1",
                &flek_core::reads::FileFilter {
                    limit: 10,
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let json = serde_json::to_value(browser).unwrap();
    assert!(!json.to_string().contains('\u{202e}'));
    assert!(json
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["can_open"] == false));
}

#[cfg(unix)]
#[test]
fn cache_containment_resolves_symlinks_and_rejects_directories() {
    let cache = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    let link = cache.path().join("link.pdf");
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    assert!(files::inside_cache(cache.path(), &link).is_none());
    assert!(files::inside_cache(cache.path(), cache.path()).is_none());
    let file = cache.path().join("real.pdf");
    std::fs::write(&file, "synthetic").unwrap();
    assert_eq!(
        files::inside_cache(cache.path(), &file),
        Some(file.canonicalize().unwrap())
    );
}

#[tokio::test]
async fn open_policy_uses_cached_extension_even_with_safe_metadata_name() {
    let store = store();
    let cache = tempfile::tempdir().unwrap();
    let path = cache.path().join("x.pdf.command");
    std::fs::write(&path, "synthetic").unwrap();
    store.with_conn(|c| {
        c.execute("INSERT INTO files (id, name, title, mimetype, local_path) VALUES ('FMASK', 'report.pdf', 'Розклад.pdf', 'application/pdf', ?1)", [path.to_str().unwrap()])?;
        Ok(())
    }).unwrap();
    let (resolved, allowed) = files::cached_open_target(&store, cache.path(), "FMASK")
        .await
        .unwrap();
    assert_eq!(resolved, path.canonicalize().unwrap());
    assert!(!allowed);
}

#[tokio::test]
async fn svg_thumbnails_join_the_preview_queue_and_only_images_are_kept() {
    let server = MockServer::start().await;
    let base = server.uri();
    let client =
        Client::with_base_url(Secret::new("xoxc-fake"), Secret::new("xoxd-fake"), &base).unwrap();
    let store = store();
    let cache = tempfile::tempdir().unwrap();
    let file = |id: &str, name: &str, mime: &str, extra: serde_json::Value| {
        let mut f = json!({
            "id": id, "name": name, "mimetype": mime, "size": 5,
            "url_private_download": format!("{base}/download/{id}"),
        });
        f.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        f
    };
    let svg = "image/svg+xml";
    Mock::given(path("/conversations.history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "has_more": false, "messages": [
            {"ts": "1.000000", "user": "U1", "text": "", "files": [
                file("S1", "logo.SVG", "image/svg+xml; charset=utf-8", json!({
                    "thumb_360": format!("{base}/thumb/S1-360"), "thumb_480": format!("{base}/thumb/S1-480")
                })),
                file("S2", "jpeg-thumb.svg", svg, json!({"thumb_480": "", "thumb_360": format!("{base}/thumb/S2")})),
                file("S3", "html-thumb.svg", svg, json!({"thumb_160": format!("{base}/thumb/S3")})),
                file("S4", "no-thumb.svg", svg, json!({})),
                // Size-specific thumbnails of other types are never queued.
                file("J1", "photo.jpg", "image/jpeg", json!({"thumb_480": format!("{base}/thumb/J1")})),
                file("H1", "page.html", svg, json!({"thumb_480": format!("{base}/thumb/H1")})),
            ]}
        ]})))
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::path_regex("^/download/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"<svg/>".to_vec(), svg))
        .mount(&server)
        .await;
    for (thumb, body, times) in [
        ("S1-480", &b"\x89PNG\r\n"[..], 1),
        ("S1-360", &b"\x89PNG\r\n"[..], 0),
        ("S2", &b"\xff\xd8\xff\xe0"[..], 1),
        // A sign-in page behind an image content type is refused once, for good.
        ("S3", &b"<html>"[..], 1),
        ("J1", &b"\x89PNG\r\n"[..], 0),
        ("H1", &b"\x89PNG\r\n"[..], 0),
    ] {
        Mock::given(path(format!("/thumb/{thumb}")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body.to_vec(), "image/png"))
            .expect(times)
            .mount(&server)
            .await;
    }
    backfill_conversation(
        &client,
        &store,
        &mut RateLimiter::new(Duration::ZERO),
        "C1",
        &SyncOptions::default(),
    )
    .await
    .unwrap();
    for _ in 0..2 {
        download_files(&client, &store, cache.path(), Some(1_000_000), |_, _| {})
            .await
            .unwrap();
    }
    let (previews, refused): (Vec<String>, Vec<String>) = store
        .with_conn(|c| {
            let ids = |sql: &str| -> flek_core::Result<Vec<String>> {
                let mut stmt = c.prepare(sql)?;
                let rows = stmt.query_map([], |r| r.get(0))?;
                Ok(rows.collect::<Result<_, _>>()?)
            };
            Ok((
                ids("SELECT id FROM files WHERE preview_path IS NOT NULL ORDER BY id")?,
                ids("SELECT file_id FROM file_download_failures WHERE preview = 1 AND permanent = 1")?,
            ))
        })
        .unwrap();
    assert_eq!(previews, ["S1", "S2"]);
    assert_eq!(refused, ["S3"]);
    assert!(!files::preview_path(cache.path(), "S3").exists());
    assert_eq!(files::preview_type(b"\x89PNG\r\n"), Some("image/png"));
    assert_eq!(files::preview_type(b"\xff\xd8\xff\xe0"), Some("image/jpeg"));
    assert_eq!(files::preview_type(b"<svg/>"), None);
    assert_eq!(files::preview_type(b""), None);
}

#[tokio::test]
async fn inline_svg_read_is_svg_only_and_capped_at_one_mebibyte() {
    let dir = tempfile::tempdir().unwrap();
    let svg = dir.path().join("drawing.svg");
    let cap = 1024 * 1024;
    std::fs::write(&svg, vec![b' '; cap]).unwrap();
    assert_eq!(
        files::read_inline_svg(&svg, Some("image/svg+xml"))
            .await
            .unwrap()
            .len(),
        cap
    );
    std::fs::write(&svg, vec![b' '; cap + 1]).unwrap();
    let error = files::read_inline_svg(&svg, Some("image/svg+xml"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("inline_preview_too_large"));
    std::fs::write(&svg, b"<svg/>").unwrap();
    assert!(files::read_inline_svg(&svg, Some("text/html"))
        .await
        .is_err());
    let html = dir.path().join("page.html");
    std::fs::write(&html, b"<svg/>").unwrap();
    assert!(files::read_inline_svg(&html, Some("image/svg+xml"))
        .await
        .is_err());
}
