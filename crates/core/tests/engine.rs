//! Sync engine: fast pass first, priority, deep backfill after.

mod support;
use support::{field, has_field};

use flek_core::api::{AuthTest, Client};
use flek_core::store::{messages, Store};
use flek_core::sync::engine::{Engine, EngineOptions, Priority};
use flek_core::sync::live::{bus, Change, Progress};
use flek_core::Secret;
use serde_json::json;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ok(v: serde_json::Value) -> ResponseTemplate {
    let mut v = v;
    v["ok"] = json!(true);
    ResponseTemplate::new(200).set_body_json(v)
}

async fn slack() -> MockServer {
    let s = MockServer::start().await;
    Mock::given(path("/users.list"))
        .respond_with(ok(json!({"members": []})))
        .mount(&s)
        .await;
    Mock::given(path("/client.counts"))
        .respond_with(ok(json!({"channels": []})))
        .mount(&s)
        .await;
    Mock::given(path("/users.conversations"))
        .respond_with(ok(json!({"channels": [
            {"id": "C1", "name": "alpha", "is_channel": true},
            {"id": "C2", "name": "beta", "is_channel": true}
        ]})))
        .mount(&s)
        .await;
    // Catch-up above the newest stored message: nothing new.
    Mock::given(path("/conversations.history"))
        .and(has_field("oldest"))
        .respond_with(ok(json!({"has_more": false, "messages": []})))
        .with_priority(1)
        .mount(&s)
        .await;
    for ch in ["C1", "C2"] {
        // Older page (deep pass)…
        Mock::given(path("/conversations.history"))
            .and(field("channel", ch.to_string()))
            .and(has_field("latest"))
            .respond_with(ok(json!({"has_more": false, "messages": [{"ts": "1.000000", "user": "U1", "text": format!("{ch} old")}]})))
            .with_priority(1)
            .mount(&s)
            .await;
        // …and the newest page (fast pass).
        Mock::given(path("/conversations.history"))
            .and(field("channel", ch.to_string()))
            .respond_with(ok(json!({"has_more": true, "messages": [{"ts": "2.000000", "user": "U1", "text": format!("{ch} new")}]})))
            .with_priority(2)
            .mount(&s)
            .await;
    }
    s
}

#[tokio::test]
async fn fast_pass_then_deep_with_priority() {
    let server = slack().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    let store = Store::open_in_memory().unwrap();
    let bus = bus();
    let mut rx = bus.subscribe();
    let priority = Priority::default();
    priority.push("C2");

    let engine = Engine {
        client,
        store: store.clone(),
        me: AuthTest {
            url: "https://fake.slack.com/".into(),
            team: "Fake".into(),
            user: "me".into(),
            team_id: "T1".into(),
            user_id: "U0ME".into(),
            enterprise_id: None,
        },
        bus: bus.clone(),
        priority,
        options: EngineOptions {
            refresh_days: 0,
            history_interval: std::time::Duration::ZERO,
            ..Default::default()
        },
    };
    let stats = engine
        .run(tokio::sync::watch::channel(false).1)
        .await
        .unwrap();
    assert_eq!(
        (stats.conversations, stats.messages, stats.failed),
        (2, 4, 0)
    );

    let mut seen = Vec::new();
    while let Ok(change) = rx.try_recv() {
        if let Change::Progress(Progress::Conversations { current, deep, .. }) = change {
            seen.push((current, deep));
        }
    }
    assert_eq!(
        seen,
        [
            ("beta".into(), false),
            ("alpha".into(), false),
            ("beta".into(), true),
            ("alpha".into(), true)
        ]
    );
    for ch in ["C1", "C2"] {
        let state = store.with_conn(|c| messages::sync_state(c, ch)).unwrap();
        assert!(state.complete, "{ch} not fully backfilled");
        assert_eq!(
            store
                .with_conn(|c| messages::recent(c, ch, 10))
                .unwrap()
                .len(),
            2
        );
    }
}
