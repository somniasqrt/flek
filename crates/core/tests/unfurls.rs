//! Link previews (images cached through the proxy) and avatars.

use flek_core::api::Client;
use flek_core::store::messages::{self, MessageRow};
use flek_core::store::{directory, unfurls, Store};
use flek_core::sync::files::{download_avatars, download_link_images, fetch_profile_photo};
use flek_core::Secret;
use serde_json::json;
use wiremock::matchers::{path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

#[tokio::test]
async fn previews_show_with_cached_images_only() {
    let server = MockServer::start().await;
    let proxy = format!("{}/proxy", server.uri());
    // HTTP/private proxy fixtures must never be contacted.
    Mock::given(path("/proxy"))
        .and(query_param("url", "https://site.example/cover.png"))
        .and(|r: &Request| {
            !r.headers.contains_key("cookie") && !r.headers.contains_key("authorization")
        })
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"\x89PNG".to_vec(), "image/png"))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(path("/proxy"))
        .and(query_param("url", "https://site.example/icon.ico"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<html>", "text/html"))
        .expect(0)
        .mount(&server)
        .await;

    let store = Store::open_in_memory().unwrap();
    let raw = json!({"ts": "1.000000", "text": "look", "attachments": [{
        "service_name": "Site", "title": "A page", "title_link": "https://site.example/a",
        "text": "About *it*", "color": "36a64f", "original_url": "https://site.example/a",
        "image_url": "https://site.example/cover.png", "image_width": 800, "image_height": 400,
        "service_icon": "https://site.example/icon.ico"
    }, {"fallback": "nothing to show"}]})
    .to_string();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
                 INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');",
            )?;
            messages::upsert_messages(
                c,
                "C1",
                &[MessageRow {
                    ts: "1.000000".into(),
                    text: "look".into(),
                    raw_json: raw.clone(),
                    ..Default::default()
                }],
            )?;
            Ok(())
        })
        .unwrap();

    let shown = |store: &Store| {
        store
            .with_read(|c| messages::page(c, "C1", None, 10))
            .unwrap()
            .remove(0)
            .unfurls
    };
    // Before caching: the preview shows without images.
    let before = shown(&store);
    assert_eq!(before.len(), 1, "empty attachments are dropped");
    assert_eq!(before[0].title.as_deref(), Some("A page"));
    assert_eq!(before[0].color.as_deref(), Some("#36a64f"));
    assert!(before[0].image.is_none());

    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    let cache = tempfile::tempdir().unwrap();
    let n = download_link_images(&client, &store, cache.path(), &proxy)
        .await
        .unwrap();
    assert_eq!(
        n, 0,
        "HTTP/private proxy must be refused before any request"
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(shown(&store)[0].image.is_none());
    let image = cache.path().join("synthetic.png");
    std::fs::write(&image, b"\x89PNG").unwrap();
    store
        .with_conn(|c| unfurls::set_image(c, "https://site.example/cover.png", image.to_str()))
        .unwrap();

    let after = shown(&store);
    let hash = unfurls::url_hash("https://site.example/cover.png");
    assert_eq!(after[0].image.as_deref(), Some(hash.as_str()));
    assert_eq!(
        (after[0].image_width, after[0].image_height),
        (Some(800), Some(400))
    );
    assert!(after[0].icon.is_none());
    let cached = store
        .with_read(|c| unfurls::image_path(c, &hash))
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(cached).unwrap(), b"\x89PNG");

    // Nothing left to try: rejected URLs are remembered.
    assert_eq!(
        download_link_images(&client, &store, cache.path(), &proxy)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn avatars_of_authors_are_cached() {
    let server = MockServer::start().await;
    Mock::given(path("/avatars/u1.png"))
        .and(|r: &Request| {
            !r.headers.contains_key("cookie") && !r.headers.contains_key("authorization")
        })
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"\x89PNG".to_vec(), "image/png"))
        .expect(0)
        .mount(&server)
        .await;

    let avatar = |name: &str| {
        json!({"profile": {"image_72": format!("{}/avatars/{name}.png", server.uri())}}).to_string()
    };
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
                 INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');",
            )?;
            // U1 wrote something; U2 never shows up, so it is not fetched.
            for (id, raw) in [("U1", avatar("u1")), ("U2", avatar("u2"))] {
                c.execute(
                    "INSERT INTO users (id, team_id, name, raw_json) VALUES (?1, 'T1', ?1, ?2)",
                    [id, raw.as_str()],
                )?;
            }
            messages::upsert_messages(
                c,
                "C1",
                &[MessageRow {
                    ts: "1.000000".into(),
                    user_id: Some("U1".into()),
                    text: "hi".into(),
                    raw_json: "{}".into(),
                    ..Default::default()
                }],
            )?;
            Ok(())
        })
        .unwrap();

    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    let cache = tempfile::tempdir().unwrap();
    assert_eq!(
        download_avatars(&client, &store, cache.path(), "T1")
            .await
            .unwrap(),
        0
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    let image = cache.path().join("synthetic.png");
    std::fs::write(&image, b"\x89PNG").unwrap();
    let url = format!("{}/avatars/u1.png", server.uri());
    store
        .with_conn(|c| unfurls::set_image(c, &url, image.to_str()))
        .unwrap();
    let shown = store.with_read(|c| directory::avatars(c, "T1")).unwrap();
    assert_eq!(shown.len(), 1);
    assert_eq!(
        shown.get("U1"),
        Some(&unfurls::url_hash(&format!(
            "{}/avatars/u1.png",
            server.uri()
        )))
    );
    assert_eq!(
        download_avatars(&client, &store, cache.path(), "T1")
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn profile_photo_refuses_private_http_and_uses_existing_cache() {
    let server = MockServer::start().await;
    Mock::given(path("/avatars/u1_512.png"))
        .and(|r: &Request| {
            !r.headers.contains_key("cookie") && !r.headers.contains_key("authorization")
        })
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"\x89PNG".to_vec(), "image/png"))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(path("/avatars/u2_192.png"))
        .respond_with(ResponseTemplate::new(404))
        .expect(0)
        .mount(&server)
        .await;

    let big = format!("{}/avatars/u1_512.png", server.uri());
    let store = Store::open_in_memory().unwrap();
    let raws = [
        (
            "U1",
            json!({"profile": {"image_72": "https://cdn.example/u1_72.png",
                "image_192": "https://cdn.example/u1_192.png", "image_512": big}}),
        ),
        (
            "U2",
            json!({"profile": {"image_192": format!("{}/avatars/u2_192.png", server.uri())}}),
        ),
        ("U3", json!({"profile": {}})),
    ];
    store
        .with_conn(|c| {
            c.execute("INSERT INTO teams (id, name) VALUES ('T1', 'Team')", [])?;
            for (id, raw) in &raws {
                c.execute(
                    "INSERT INTO users (id, team_id, name, raw_json) VALUES (?1, 'T1', ?1, ?2)",
                    [*id, raw.to_string().as_str()],
                )?;
            }
            Ok(())
        })
        .unwrap();
    let cache = tempfile::tempdir().unwrap();
    let ua = "Mozilla/5.0 (test)";

    assert!(fetch_profile_photo(ua, &store, cache.path(), "U1")
        .await
        .is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
    let image = cache.path().join("synthetic.png");
    std::fs::write(&image, b"\x89PNG").unwrap();
    store
        .with_conn(|c| unfurls::set_image(c, &big, image.to_str()))
        .unwrap();
    let hash = Some(unfurls::url_hash(&big));
    // A cached record needs no network request; the profile shows it.
    let again = fetch_profile_photo(ua, &store, cache.path(), "U1")
        .await
        .unwrap();
    assert_eq!(again, hash);
    let profile = store
        .with_read(|c| directory::user_profile(c, "U1"))
        .unwrap()
        .unwrap();
    assert_eq!(profile.photo, hash);
    assert!(profile.has_photo);

    // 192 px is the fallback; a failure is an error, retried next time.
    for _ in 0..2 {
        assert!(fetch_profile_photo(ua, &store, cache.path(), "U2")
            .await
            .is_err());
    }
    // No photo at all, and unknown users: nothing to fetch.
    for id in ["U3", "U404"] {
        assert_eq!(
            fetch_profile_photo(ua, &store, cache.path(), id)
                .await
                .unwrap(),
            None
        );
    }
}
