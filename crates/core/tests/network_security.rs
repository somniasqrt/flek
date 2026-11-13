use flek_core::{api::Client, Secret};
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn cross_origin_redirect_never_carries_session() {
    let source = MockServer::start().await;
    let target = MockServer::start().await;
    // Different host spelling as well as port, even though both are local mocks.
    let location = target.uri().replace("127.0.0.1", "localhost");
    Mock::given(path("/file"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", location))
        .mount(&source)
        .await;
    Mock::given(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&target)
        .await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &source.uri(),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let result = client
        .download(
            &format!("{}/file", source.uri()),
            &dir.path().join("file"),
            false,
        )
        .await;
    assert!(result.is_err());
    assert!(target.received_requests().await.unwrap().is_empty());
    assert_eq!(source.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn refused_file_urls_make_no_request() {
    let server = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    for url in [
        "http://files.slack.com/x",
        "https://evil.test/x",
        "https://files.slack.com.evil.test/x",
        "https://files.slack.com:8443/x",
        "https://a.slack-edge.com:8443/x",
        "https://a.slack-files.com:8443/x",
    ] {
        let err = client
            .download(url, &dir.path().join("file"), false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("untrusted file URL"));
    }
    assert!(server.received_requests().await.unwrap().is_empty());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn invalid_rtm_connect_destinations_are_rejected_before_handshake() {
    for url in ["ws://wss-primary.slack.com/x", "wss://evil.test/x"] {
        let server = MockServer::start().await;
        Mock::given(path("/rtm.connect"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ok": true, "url": url})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client = Client::with_base_url(
            Secret::new("xoxc-fake"),
            Secret::new("xoxd-fake"),
            &server.uri(),
        )
        .unwrap();
        let err = flek_core::rtm::Connection::open(&client)
            .await
            .err()
            .unwrap();
        assert!(err.to_string().contains("untrusted RTM URL"));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn file_bounds_use_settings_and_declared_size_and_remove_partial_files() {
    use flek_core::{files, store::files::PendingDownload};
    let server = MockServer::start().await;
    Mock::given(path("/file"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0; 70_000]))
        .mount(&server)
        .await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut file = PendingDownload {
        id: "F1".into(),
        name: Some("x.bin".into()),
        mimetype: None,
        size: None,
        url: format!("{}/file", server.uri()),
        local_path: None,
    };
    for (size, limit) in [(None, Some(10)), (Some(1), None), (None, Some(0))] {
        file.size = size;
        let err = files::fetch_bounded(&client, dir.path(), &file, limit)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("byte limit"));
        assert!(!dir.path().join("F1/x.bin").exists());
        if dir.path().join("F1").exists() {
            assert_eq!(std::fs::read_dir(dir.path().join("F1")).unwrap().count(), 0);
        }
    }
    // The small tolerance (64 KiB) accommodates metadata rounding.
    file.size = Some(70_000);
    assert!(
        files::fetch_bounded(&client, dir.path(), &file, Some(70_000))
            .await
            .unwrap()
            .1
    );
}

#[cfg(unix)]
#[test]
fn own_storage_permissions_are_created_and_repaired() {
    use flek_core::{files, settings::Settings, store::Store};
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let original_parent_mode = std::fs::metadata(root.path()).unwrap().permissions().mode() & 0o777;
    let data = root.path().join("data");
    let path = data.join("flek.db");
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    {
        let db = Store::open(&path).unwrap();
        db.with_conn(|c| { c.execute_batch("CREATE TABLE permission_test (id INTEGER); INSERT INTO permission_test VALUES (1);")?; Ok(()) }).unwrap();
        for file in ["flek.db", "flek.db-wal", "flek.db-shm"] {
            assert_eq!(mode(&data.join(file)), 0o600, "{file}");
            std::fs::set_permissions(data.join(file), std::fs::Permissions::from_mode(0o666))
                .unwrap();
        }
        db.with_conn(|c| {
            c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            Ok(())
        })
        .unwrap();
        for file in ["flek.db", "flek.db-wal", "flek.db-shm"] {
            assert_eq!(mode(&data.join(file)), 0o600);
        }
    }
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    Store::open(&path).unwrap();
    assert_eq!(mode(&data), 0o700);
    assert_eq!(mode(&path), 0o600);
    let settings = data.join("settings.json");
    Settings::default().save(&settings).unwrap();
    assert_eq!(mode(&settings), 0o600);
    std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o644)).unwrap();
    Settings::load(&settings).unwrap();
    assert_eq!(mode(&settings), 0o600);
    let cache = root.path().join("flek/files");
    std::fs::create_dir_all(cache.join("F1")).unwrap();
    let cached = cache.join("F1/x");
    std::fs::write(&cached, "synthetic").unwrap();
    files::prepare_cache(&cache).unwrap();
    for directory in [cache.parent().unwrap(), &cache, &cache.join("F1")] {
        assert_eq!(mode(directory), 0o700);
    }
    assert_eq!(mode(&cached), 0o600);
    // A dangling private-storage symlink must fail before creating its target.
    let outside = root.path().join("outside.db");
    let link = data.join("linked.db");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    assert!(Store::open(&link).is_err());
    assert!(!outside.exists());
    // Shared parent stays untouched.
    assert_eq!(mode(root.path()), original_parent_mode);
}

#[tokio::test]
async fn same_origin_redirects_keep_session_but_stop_after_five_hops() {
    use wiremock::matchers::header;
    let server = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    Mock::given(path("/start"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", format!("{}/end", server.uri())),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/end"))
        .and(header("cookie", "d=xoxd-fake"))
        .and(header("authorization", "Bearer xoxc-fake"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/loop"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", format!("{}/loop", server.uri())),
        )
        .expect(6)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("file");
    client
        .download(&format!("{}/start", server.uri()), &dest, false)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"ok");
    assert!(client
        .download(&format!("{}/loop", server.uri()), &dest, false)
        .await
        .is_err());
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        b"ok",
        "failed download preserves previous cache entry"
    );
}

#[tokio::test]
async fn api_redirects_never_reach_another_origin() {
    let source = MockServer::start().await;
    let target = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &source.uri(),
    )
    .unwrap();
    for status in [301, 302, 303, 307, 308] {
        let method = format!("redirect{status}");
        Mock::given(path(format!("/{method}")))
            .respond_with(ResponseTemplate::new(status).insert_header("Location", target.uri()))
            .expect(1)
            .mount(&source)
            .await;
        assert!(client
            .call::<serde_json::Value>(&method, &[])
            .await
            .is_err());
    }
    assert!(target.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn api_same_origin_redirect_retains_cookie() {
    use wiremock::matchers::{header, method};
    let server = MockServer::start().await;
    Mock::given(path("/auth.test"))
        .and(method("POST"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/redirected"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/redirected"))
        .and(header("cookie", "d=xoxd-fake"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    client
        .call::<serde_json::Value>("auth.test", &[])
        .await
        .unwrap();
}
