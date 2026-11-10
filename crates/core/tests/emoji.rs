//! Custom emoji: `emoji.list` stored, aliases resolved, only used images cached.

use std::time::Duration;

use flek_core::api::Client;
use flek_core::store::emoji;
use flek_core::store::messages::{self, MessageRow};
use flek_core::store::{unfurls, Store};
use flek_core::sync::directory::{sync_custom_emoji, EMOJI_MAX_AGE};
use flek_core::sync::files::download_custom_emoji;
use flek_core::Secret;
use serde_json::json;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
}

fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
                 INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');",
            )?;
            messages::upsert_messages(
                c,
                "C1",
                &[
                    MessageRow {
                        ts: "1.000000".into(),
                        text: "standup at 10:30 :party_parrot: and :shipit:".into(),
                        raw_json: "{}".into(),
                        ..Default::default()
                    },
                    MessageRow {
                        ts: "2.000000".into(),
                        text: "reacted".into(),
                        raw_json: "{}".into(),
                        ..Default::default()
                    },
                ],
            )?;
            c.execute(
                "INSERT INTO reactions (conversation_id, ts, name, user_id)
                 VALUES ('C1', '2.000000', 'gdocs', 'U1')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    store
}

#[tokio::test]
async fn custom_emoji_are_listed_resolved_and_fetched_when_used() {
    let server = MockServer::start().await;
    let img = |n: &str| format!("{}/emoji/{n}.png", server.uri());
    Mock::given(path("/emoji.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "emoji": {
                "party_parrot": img("party_parrot"),
                "gdocs": img("gdocs"),
                "unused": img("unused"),
                "shipit": "alias:squirrel",
                "squirrel": img("squirrel"),
                "yes": "alias:thumbsup",
                "loop_a": "alias:loop_b",
                "loop_b": "alias:loop_a"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    // Imported HTTP/private image URLs must never be contacted.
    for name in ["party_parrot", "gdocs", "squirrel"] {
        Mock::given(path(format!("/emoji/{name}.png")))
            .and(|r: &Request| {
                !r.headers.contains_key("cookie") && !r.headers.contains_key("authorization")
            })
            .respond_with(ResponseTemplate::new(200).set_body_raw(b"\x89PNG".to_vec(), "image/png"))
            .expect(0)
            .mount(&server)
            .await;
    }
    Mock::given(path("/emoji/unused.png"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"\x89PNG".to_vec(), "image/png"))
        .expect(0)
        .mount(&server)
        .await;

    let client = client(&server);
    let store = store();
    assert!(sync_custom_emoji(&client, &store, "T1", Duration::ZERO)
        .await
        .unwrap());
    // Fresh list: the daily refresh skips the call (expect(1) above).
    assert!(!sync_custom_emoji(&client, &store, "T1", EMOJI_MAX_AGE)
        .await
        .unwrap());

    // Nothing cached yet: no images, but standard aliases are known.
    let before = store.with_read(|c| emoji::custom_emoji(c, "T1")).unwrap();
    assert!(before.images.is_empty());
    assert_eq!(before.aliases.len(), 1);
    assert_eq!(
        before.aliases.get("yes").map(String::as_str),
        Some("thumbsup")
    );

    let cache = tempfile::tempdir().unwrap();
    let n = download_custom_emoji(&client, &store, cache.path(), "T1")
        .await
        .unwrap();
    assert_eq!(
        n, 0,
        "private HTTP images must be refused before any request"
    );
    assert!(store
        .with_read(|c| emoji::custom_emoji(c, "T1"))
        .unwrap()
        .images
        .is_empty());
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "only emoji.list is allowed"
    );
    // Cache hydration/alias resolution remains independent of network URL policy.
    let image = cache.path().join("synthetic.png");
    std::fs::write(&image, b"\x89PNG").unwrap();
    store
        .with_conn(|c| {
            for name in ["party_parrot", "gdocs", "squirrel"] {
                unfurls::set_image(c, &img(name), image.to_str())?;
            }
            Ok(())
        })
        .unwrap();

    let after = store.with_read(|c| emoji::custom_emoji(c, "T1")).unwrap();
    let hash = |n: &str| unfurls::url_hash(&img(n));
    assert_eq!(
        after.images.get("party_parrot"),
        Some(&hash("party_parrot"))
    );
    assert_eq!(after.images.get("gdocs"), Some(&hash("gdocs")));
    assert_eq!(after.images.get("shipit"), Some(&hash("squirrel")));
    assert_eq!(after.images.get("squirrel"), Some(&hash("squirrel")));
    assert!(!after.images.contains_key("unused"));
    assert!(!after.images.contains_key("loop_a") && !after.aliases.contains_key("loop_a"));

    // Everything used is cached: nothing left to fetch.
    assert_eq!(
        download_custom_emoji(&client, &store, cache.path(), "T1")
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn emoji_list_failure_is_not_fatal() {
    let server = MockServer::start().await;
    Mock::given(path("/emoji.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": false, "error": "missing_scope"
        })))
        .mount(&server)
        .await;
    let store = store();
    assert!(
        !sync_custom_emoji(&client(&server), &store, "T1", Duration::ZERO)
            .await
            .unwrap()
    );
    assert!(store
        .with_read(|c| emoji::synced_at(c, "T1"))
        .unwrap()
        .is_none());
}
