//! Link previews (images cached through the proxy) and avatars.

use flek_core::api::Client;
use flek_core::store::messages::{self, MessageRow};
use flek_core::store::{directory, unfurls, Store};
use flek_core::sync::files::{download_avatars, download_link_images};
use flek_core::Secret;
use serde_json::json;
use wiremock::matchers::{path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

#[tokio::test]
async fn previews_show_with_cached_images_only() {
    let server = MockServer::start().await;
    let proxy = format!("{}/proxy", server.uri());
    // The proxy is public: no session may reach it.
    Mock::given(path("/proxy"))
        .and(query_param("url", "https://site.example/cover.png"))
        .and(|r: &Request| {
            !r.headers.contains_key("cookie") && !r.headers.contains_key("authorization")
        })
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"\x89PNG".to_vec(), "image/png"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/proxy"))
        .and(query_param("url", "https://site.example/icon.ico"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<html>", "text/html"))
        .expect(1)
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
    assert_eq!(n, 1, "the HTML 'icon' is refused");

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

    // Nothing left to try: failures are remembered (expect(1) above).
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
        .expect(1)
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
        1
    );
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
