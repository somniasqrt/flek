//! Download retry memory, manual downloads and the media scan queue
//! (M9.12, M9.15.1, M9.15.5). Temp stores and wiremock only.

use flek_core::api::Client;
use flek_core::store::{unfurls, Store};
use flek_core::sync::files::download_files;
use flek_core::{files, Error, Secret};
use wiremock::matchers::path;
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

fn client(base: &str) -> Client {
    Client::with_base_url(Secret::new("xoxc-fake"), Secret::new("xoxd-fake"), base).unwrap()
}

fn add_file(store: &Store, base: &str, id: &str, size: Option<i64>) {
    store
        .with_conn(|c| {
            c.execute(
                "INSERT INTO files (id, name, mimetype, size, url_private_download)
                 VALUES (?1, ?1 || '.pdf', 'application/pdf', ?2, ?3)",
                rusqlite::params![id, size, format!("{base}/download/{id}")],
            )?;
            Ok(())
        })
        .unwrap();
}

async fn serve(server: &MockServer, id: &str, status: u16, body: &[u8]) {
    Mock::given(path(format!("/download/{id}")))
        .respond_with(ResponseTemplate::new(status).set_body_raw(body.to_vec(), "application/pdf"))
        .mount(server)
        .await;
}

async fn requests(server: &MockServer, id: &str) -> usize {
    let wanted = format!("/download/{id}");
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == wanted)
        .count()
}

fn failure(store: &Store, id: &str) -> Option<(i64, bool)> {
    store
        .with_conn(|c| {
            use rusqlite::OptionalExtension;
            Ok(c.query_row(
                "SELECT failure_count, permanent FROM file_download_failures
                 WHERE file_id = ?1 AND preview = 0",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
        })
        .unwrap()
}

fn make_due(store: &Store) {
    store
        .with_conn(|c| {
            c.execute("UPDATE file_download_failures SET retry_after = 0", [])?;
            c.execute("UPDATE link_images SET retry_after = 0", [])?;
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn failed_downloads_are_remembered_between_passes() {
    let server = MockServer::start().await;
    let base = server.uri();
    let (client, store, cache) = (client(&base), store(), tempfile::tempdir().unwrap());
    add_file(&store, &base, "FGONE", Some(5));
    add_file(&store, &base, "FFLAKY", Some(5));
    serve(&server, "FGONE", 404, b"").await;
    serve(&server, "FFLAKY", 500, b"").await;

    let pass = || download_files(&client, &store, cache.path(), None, |_, _| {});
    assert_eq!(pass().await.unwrap().failed, 2);
    assert_eq!(failure(&store, "FGONE"), Some((1, true)));
    assert_eq!(failure(&store, "FFLAKY"), Some((1, false)));

    // A reconnect right away asks for neither.
    let again = pass().await.unwrap();
    assert_eq!((again.failed, again.downloaded), (0, 0));
    assert_eq!(requests(&server, "FGONE").await, 1);
    assert_eq!(requests(&server, "FFLAKY").await, 1);

    // Once the pause is over only the transient failure is retried.
    make_due(&store);
    assert_eq!(pass().await.unwrap().failed, 1);
    assert_eq!(requests(&server, "FGONE").await, 1);
    assert_eq!(requests(&server, "FFLAKY").await, 2);
    assert_eq!(failure(&store, "FFLAKY"), Some((2, false)));
}

#[tokio::test]
async fn the_pause_grows_and_is_capped_at_a_day() {
    let store = store();
    add_file(&store, "https://files.slack.com", "F1", Some(5));
    let mut previous = 0;
    for attempt in 1..=14 {
        let wait: i64 = store
            .with_conn(|c| {
                flek_core::store::files::record_failure(c, "F1", false, false)?;
                Ok(c.query_row(
                    "SELECT retry_after - unixepoch() FROM file_download_failures",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert!(wait >= previous, "attempt {attempt}: {wait} < {previous}");
        assert!((59..=86_400).contains(&wait), "attempt {attempt}: {wait}");
        previous = wait;
    }
    assert_eq!(previous, 86_400);
}

#[tokio::test]
async fn a_manual_download_retries_a_permanent_failure_and_clears_it() {
    let server = MockServer::start().await;
    let base = server.uri();
    let (client, store, cache) = (client(&base), store(), tempfile::tempdir().unwrap());
    add_file(&store, &base, "F1", Some(5));
    store
        .with_conn(|c| flek_core::store::files::record_failure(c, "F1", false, true))
        .unwrap();
    serve(&server, "F1", 200, b"%PDF-").await;

    let auto = download_files(&client, &store, cache.path(), None, |_, _| {})
        .await
        .unwrap();
    assert_eq!((auto.downloaded, auto.failed), (0, 0));
    assert_eq!(requests(&server, "F1").await, 0);

    let saved = files::fetch_stored(&client, &store, cache.path(), "F1")
        .await
        .unwrap();
    assert_eq!(std::fs::read(saved).unwrap(), b"%PDF-");
    assert_eq!(failure(&store, "F1"), None);
}

#[tokio::test]
async fn a_failed_manual_download_is_recorded_too() {
    let server = MockServer::start().await;
    let base = server.uri();
    let (client, store, cache) = (client(&base), store(), tempfile::tempdir().unwrap());
    add_file(&store, &base, "F1", Some(5));
    serve(&server, "F1", 410, b"").await;
    let error = files::fetch_stored(&client, &store, cache.path(), "F1")
        .await
        .unwrap_err();
    assert!(matches!(error, Error::HttpStatus(410)));
    assert_eq!(failure(&store, "F1"), Some((1, true)));
}

#[tokio::test]
async fn the_auto_download_limit_does_not_apply_to_the_button() {
    // M9.12.2: with the limit at 0 (or below the file size) the button still works.
    let server = MockServer::start().await;
    let base = server.uri();
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("flek.db")).unwrap();
    store
        .with_conn(|c| {
            c.execute_batch("INSERT INTO teams (id, name) VALUES ('T1', 'Team');")?;
            Ok(())
        })
        .unwrap();
    flek_core::settings::Settings {
        max_file_bytes: Some(0),
        ..Default::default()
    }
    .save(&dir.path().join("settings.json"))
    .unwrap();
    let (client, cache) = (client(&base), tempfile::tempdir().unwrap());
    let body = vec![b'x'; 4096];
    add_file(&store, &base, "FBIG", Some(body.len() as i64));
    serve(&server, "FBIG", 200, &body).await;

    let auto = download_files(&client, &store, cache.path(), Some(0), |_, _| {})
        .await
        .unwrap();
    assert_eq!(auto.downloaded, 0);
    assert_eq!(requests(&server, "FBIG").await, 0);

    let saved = files::fetch_stored(&client, &store, cache.path(), "FBIG")
        .await
        .unwrap();
    assert_eq!(std::fs::metadata(saved).unwrap().len(), 4096);
}

#[tokio::test]
async fn a_manual_download_is_still_bounded_by_the_declared_size() {
    let server = MockServer::start().await;
    let base = server.uri();
    let (client, store, cache) = (client(&base), store(), tempfile::tempdir().unwrap());
    add_file(&store, &base, "FLIAR", Some(10));
    serve(&server, "FLIAR", 200, &vec![b'x'; 10 + 64 * 1024 + 1]).await;
    assert!(files::fetch_stored(&client, &store, cache.path(), "FLIAR")
        .await
        .is_err());
    assert!(!files::cache_path(cache.path(), "FLIAR", Some("FLIAR.pdf")).exists());
}

#[tokio::test]
async fn clearing_the_cache_does_not_start_a_download_storm() {
    let server = MockServer::start().await;
    let base = server.uri();
    let (client, store, cache) = (client(&base), store(), tempfile::tempdir().unwrap());
    add_file(&store, &base, "F1", Some(5));
    serve(&server, "F1", 200, b"%PDF-").await;
    let pass = || download_files(&client, &store, cache.path(), None, |_, _| {});
    assert_eq!(pass().await.unwrap().downloaded, 1);

    files::storage::clear(&store, cache.path()).await.unwrap();
    assert!(!files::cache_path(cache.path(), "F1", Some("F1.pdf")).exists());
    assert_eq!(pass().await.unwrap().downloaded, 0);
    assert_eq!(requests(&server, "F1").await, 1);

    // The user can still fetch it, and that file is automatic again afterwards.
    files::fetch_stored(&client, &store, cache.path(), "F1")
        .await
        .unwrap();
    assert_eq!(requests(&server, "F1").await, 2);
    let paused: bool = store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT auto_download_paused FROM files WHERE id = 'F1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(!paused);
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn downloads_are_quarantined_and_an_approval_is_never_rewritten() {
    const ATTR: &str = "com.apple.quarantine";
    let server = MockServer::start().await;
    let base = server.uri();
    let (client, store, cache) = (client(&base), store(), tempfile::tempdir().unwrap());
    add_file(&store, &base, "F1", Some(5));
    serve(&server, "F1", 200, b"%PDF-").await;
    let saved = files::fetch_stored(&client, &store, cache.path(), "F1")
        .await
        .unwrap();
    assert!(xattr::get(&saved, ATTR).unwrap().is_some());

    // Gatekeeper records the user's approval in the flags; keep it.
    let approved = b"00c1;65000000;Flek;synthetic".to_vec();
    xattr::set(&saved, ATTR, &approved).unwrap();
    files::quarantine(&saved).unwrap();
    files::quarantine(&saved).unwrap();
    assert_eq!(xattr::get(&saved, ATTR).unwrap().unwrap(), approved);
}

#[test]
fn imported_ids_and_names_cannot_take_service_paths() {
    let cache = std::path::Path::new("/synthetic/cache");
    let links = files::link_image_path(cache, "https://files.slack-edge.com/a.png");
    let links_dir = links.parent().unwrap();
    for id in ["_links", "_LINKS", "_Links"] {
        let attachment = files::cache_path(cache, id, Some("a.png"));
        assert!(attachment.starts_with(cache));
        assert!(
            !attachment
                .parent()
                .unwrap()
                .as_os_str()
                .eq_ignore_ascii_case(links_dir.as_os_str()),
            "{id} shares the image cache directory"
        );
        assert!(!files::preview_path(cache, id)
            .parent()
            .unwrap()
            .as_os_str()
            .eq_ignore_ascii_case(links_dir.as_os_str()));
    }
    for name in [files::PREVIEW_NAME, "_FLEK_PREVIEW.PNG"] {
        let attachment = files::cache_path(cache, "F1", Some(name));
        let preview = files::preview_path(cache, "F1");
        assert_eq!(attachment.parent(), preview.parent());
        assert!(!attachment
            .as_os_str()
            .eq_ignore_ascii_case(preview.as_os_str()));
    }
    // Ordinary files keep their layout, so existing caches stay valid.
    assert_eq!(
        files::cache_path(cache, "F1", Some("report.pdf")),
        cache.join("F1").join("report.pdf")
    );
    assert_eq!(
        files::preview_path(cache, "F1"),
        cache.join("F1").join(files::PREVIEW_NAME)
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_cache_is_refused_without_touching_its_target() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("elsewhere");
    std::fs::create_dir(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
    let inside = target.join("file.pdf");
    std::fs::write(&inside, b"x").unwrap();
    let cache = root.path().join("cache");
    std::os::unix::fs::symlink(&target, &cache).unwrap();

    assert!(files::prepare_cache(&cache).is_err());
    assert!(files::inside_cache(&cache, &cache.join("file.pdf")).is_none());
    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
}

#[cfg(unix)]
#[test]
fn the_cache_tree_is_repaired_once_per_process() {
    use std::os::unix::fs::PermissionsExt;
    let cache = tempfile::tempdir().unwrap();
    let file = cache.path().join("F1").join("a.pdf");
    std::fs::create_dir(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"x").unwrap();
    let mode = |m| std::fs::set_permissions(&file, std::fs::Permissions::from_mode(m)).unwrap();
    let current = || std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    mode(0o644);
    files::prepare_cache(cache.path()).unwrap();
    assert_eq!(current(), 0o600);
    // A later call does not walk the tree again.
    mode(0o644);
    files::prepare_cache(cache.path()).unwrap();
    assert_eq!(current(), 0o644);
}

fn image_state(store: &Store, url: &str) -> (bool, bool) {
    store
        .with_conn(|c| {
            let failed = c.query_row(
                "SELECT failed FROM link_images WHERE url = ?1",
                [url],
                |r| r.get(0),
            )?;
            Ok((failed, unfurls::image_pending(c, url)?))
        })
        .unwrap()
}

#[test]
fn only_definitive_image_refusals_are_final() {
    // M9.15.1: a dropped connection must not mark an avatar failed forever.
    let store = store();
    let (gone, flaky) = (
        "https://avatars.slack-edge.com/gone.png",
        "https://avatars.slack-edge.com/flaky.png",
    );
    store
        .with_conn(|c| {
            assert!(unfurls::image_pending(c, gone)?);
            unfurls::record_image_failure(c, gone, true)?;
            unfurls::record_image_failure(c, flaky, false)?;
            Ok(())
        })
        .unwrap();
    // Neither is asked for again right away.
    assert_eq!(image_state(&store, gone), (true, false));
    assert_eq!(image_state(&store, flaky), (false, false));
    make_due(&store);
    assert_eq!(image_state(&store, gone), (true, false));
    assert_eq!(image_state(&store, flaky), (false, true));
}

#[test]
fn image_errors_are_classified() {
    for status in [404, 410, 301] {
        assert!(files::permanent_image_failure(&Error::HttpStatus(status)));
    }
    for status in [408, 429, 500, 502, 503] {
        assert!(!files::permanent_image_failure(&Error::HttpStatus(status)));
    }
    assert!(files::permanent_image_failure(&Error::File(
        "link preview is not an image".into()
    )));
    assert!(!files::permanent_image_failure(&Error::Io(
        std::io::Error::other("disk full")
    )));
    for status in [403, 404, 410] {
        assert!(files::permanent_file_failure(&Error::HttpStatus(status)));
    }
    for status in [401, 429, 500, 503] {
        assert!(!files::permanent_file_failure(&Error::HttpStatus(status)));
    }
    assert!(!files::permanent_file_failure(&Error::Io(
        std::io::Error::other("disk full")
    )));
}

#[test]
fn an_image_whose_file_is_gone_is_not_cached() {
    let store = store();
    let url = "https://avatars.slack-edge.com/a.png";
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.png");
    std::fs::write(&file, b"\x89PNG").unwrap();
    store
        .with_conn(|c| {
            unfurls::set_image(c, url, file.to_str())?;
            assert!(unfurls::is_cached(c, url));
            std::fs::remove_file(&file).unwrap();
            assert!(!unfurls::is_cached(c, url));
            assert_eq!(unfurls::image_path(c, &unfurls::url_hash(url))?, None);
            // The next pass drops the stale path and the image is wanted again.
            let missing = unfurls::missing_image_hints(c);
            assert_eq!(missing.len(), 1);
            unfurls::invalidate_missing(c, &missing)?;
            assert!(unfurls::image_pending(c, url)?);
            assert!(unfurls::missing_image_hints(c).is_empty());
            Ok(())
        })
        .unwrap();
}

fn add_message(store: &Store, ts: &str, text: &str, image: Option<&str>) {
    let raw = match image {
        Some(url) => serde_json::json!({"attachments": [{"image_url": url}]}),
        None => serde_json::json!({}),
    };
    store
        .with_conn(|c| {
            c.execute(
                "INSERT INTO messages (conversation_id, ts, text, raw_json)
                 VALUES ('C1', ?1, ?2, ?3)",
                rusqlite::params![ts, text, raw.to_string()],
            )?;
            Ok(())
        })
        .unwrap();
}

fn queued(store: &Store) -> i64 {
    store
        .with_conn(|c| {
            Ok(
                c.query_row("SELECT COUNT(*) FROM media_message_changes", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
}

fn drain(store: &Store) -> Vec<usize> {
    let mut batches = Vec::new();
    loop {
        let scanned = store.with_conn(|c| unfurls::scan_media(c)).unwrap();
        if scanned.is_empty() {
            return batches;
        }
        batches.push(scanned.len());
        store
            .with_conn(|c| unfurls::save_media_scan(c, scanned))
            .unwrap();
    }
}

#[test]
fn media_scan_only_looks_at_new_and_changed_messages() {
    // M9.15.5: no full scan of `messages` on every pass.
    let store = store();
    for i in 0..2001 {
        add_message(
            &store,
            &format!("{}.000000", 1_700_000_000 + i),
            "plain",
            None,
        );
    }
    assert_eq!(queued(&store), 2001);
    assert_eq!(drain(&store), [1000, 1000, 1]);
    assert_eq!(queued(&store), 0);

    store
        .with_conn(|c| {
            c.execute(
                "UPDATE messages SET text = ':party-parrot:' WHERE ts = '1700000005.000000'",
                [],
            )?;
            // Not a content change: stays out of the queue.
            c.execute(
                "UPDATE messages SET thread_ts = ts WHERE ts = '1700000006.000000'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(queued(&store), 1);
    assert_eq!(drain(&store), [1]);
    let names: Vec<String> = store
        .with_conn(|c| {
            let mut stmt = c.prepare("SELECT name FROM media_emoji")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<Result<_, _>>()?)
        })
        .unwrap();
    assert_eq!(names, ["party-parrot"]);
}

#[test]
fn an_edit_between_scan_and_save_is_scanned_again() {
    let store = store();
    let (old, new) = (
        "https://files.slack-edge.com/old.png",
        "https://files.slack-edge.com/new.png",
    );
    add_message(&store, "1700000000.000000", "x", Some(old));
    let stale = store.with_conn(|c| unfurls::scan_media(c)).unwrap();
    store
        .with_conn(|c| {
            let raw = serde_json::json!({"attachments": [{"image_url": new}]}).to_string();
            c.execute("UPDATE messages SET raw_json = ?1", [raw])?;
            unfurls::save_media_scan(c, stale)
        })
        .unwrap();
    assert_eq!(queued(&store), 1, "the newer version is still queued");
    drain(&store);
    let urls = store
        .with_conn(|c| unfurls::images_to_fetch(c, 10))
        .unwrap();
    assert_eq!(urls, [new]);
}

#[test]
fn images_of_the_newest_messages_come_first_and_deleted_ones_never() {
    let store = store();
    let url = |n: &str| format!("https://files.slack-edge.com/{n}.png");
    add_message(&store, "1700000001.000000", "", Some(&url("a")));
    add_message(&store, "1700000003.000000", "", Some(&url("z")));
    add_message(&store, "1700000005.000000", "", Some(&url("gone")));
    drain(&store);
    // Still queued: read through the overlay.
    add_message(&store, "1700000004.000000", "", Some(&url("m")));
    add_message(&store, "1700000002.000000", "", Some(&url("z")));
    store
        .with_conn(|c| {
            c.execute(
                "UPDATE messages SET deleted = 1 WHERE ts = '1700000005.000000'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    for persisted in [false, true] {
        if persisted {
            drain(&store);
        }
        let all = store
            .with_conn(|c| unfurls::images_to_fetch(c, 10))
            .unwrap();
        assert_eq!(
            all,
            [url("m"), url("z"), url("a")],
            "persisted: {persisted}"
        );
        let capped = store.with_conn(|c| unfurls::images_to_fetch(c, 2)).unwrap();
        assert_eq!(capped, [url("m"), url("z")]);
    }
}

#[test]
fn migration_0020_gives_old_failures_another_attempt_and_queues_stored_messages() {
    // Before 0020 every error, a dropped connection included, was final.
    use flek_core::store::migrations;
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    migrations::migrate_to(&mut conn, 19).unwrap();
    conn.execute_batch(
        "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
         INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');
         INSERT INTO messages (conversation_id, ts, text, raw_json) VALUES ('C1', '1.000000', ':x:', '{}');
         INSERT INTO link_images (hash, url, local_path, failed, fetched_at) VALUES
            ('h1', 'https://a.slack-edge.com/failed.png', NULL, 1, 1),
            ('h2', 'https://a.slack-edge.com/ok.png', '/synthetic/ok.png', 0, 1);
         INSERT INTO files (id, name) VALUES ('_links', 'a.png'), ('F1', '_flek_preview.png'), ('F2', 'b.pdf');
         UPDATE files SET local_path = '/synthetic/' || id;",
    )
    .unwrap();
    migrations::migrate(&mut conn).unwrap();
    let (failed, waits): (bool, bool) = conn
        .query_row(
            "SELECT failed, retry_after > unixepoch() FROM link_images WHERE hash = 'h1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(
        !failed && waits,
        "old failures are retried, but not at once"
    );
    let kept: Option<String> = conn
        .query_row(
            "SELECT local_path FROM link_images WHERE hash = 'h2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept.as_deref(), Some("/synthetic/ok.png"));
    let queued: i64 = conn
        .query_row("SELECT COUNT(*) FROM media_message_changes", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(queued, 1);
    let paths: Vec<(String, Option<String>)> = conn
        .prepare("SELECT id, local_path FROM files ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        paths,
        [
            ("F1".to_owned(), None),
            ("F2".to_owned(), Some("/synthetic/F2".to_owned())),
            ("_links".to_owned(), None),
        ]
    );
}
