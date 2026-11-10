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
    let mut history = Vec::new();
    while let Ok(change) = rx.try_recv() {
        match change {
            Change::Progress(Progress::Conversations { current, deep, .. }) => {
                seen.push((current, deep))
            }
            Change::History { conversation_id } => history.push(conversation_id),
            _ => {}
        }
    }
    // Every pass that wrote messages tells listeners (fast and deep, both channels).
    history.sort();
    assert_eq!(history, ["C1", "C1", "C2", "C2"]);
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

/// Slack ts `secs` seconds ago.
fn ago(secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("{}.000100", now - secs)
}

/// One channel with two recent messages; `counts` is the `client.counts` reply.
async fn quiet_slack(counts: ResponseTemplate) -> MockServer {
    let s = MockServer::start().await;
    Mock::given(path("/users.list"))
        .respond_with(ok(json!({"members": []})))
        .mount(&s)
        .await;
    Mock::given(path("/client.counts"))
        .respond_with(counts)
        .mount(&s)
        .await;
    Mock::given(path("/users.conversations"))
        .respond_with(ok(
            json!({"channels": [{"id": "C1", "name": "alpha", "is_channel": true}]}),
        ))
        .mount(&s)
        .await;
    Mock::given(path("/conversations.history"))
        .respond_with(ok(json!({"has_more": false, "messages": [
            {"ts": ago(7200), "user": "U1", "text": "older"},
            {"ts": ago(3600), "user": "U1", "text": "newest"}
        ]})))
        .mount(&s)
        .await;
    s
}

fn engine(
    server: &MockServer,
    store: &Store,
    bus: &flek_core::sync::live::Bus,
    refresh_every: u64,
) -> Engine {
    Engine {
        client: Client::with_base_url(
            Secret::new("xoxc-fake"),
            Secret::new("xoxd-fake"),
            &server.uri(),
        )
        .unwrap()
        .without_pacing(),
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
        priority: Priority::default(),
        options: EngineOptions {
            refresh_days: 7,
            refresh_every: std::time::Duration::from_secs(refresh_every),
            history_interval: std::time::Duration::ZERO,
            ..Default::default()
        },
    }
}

async fn history_requests(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/conversations.history")
        .count()
}

async fn start(e: &Engine) {
    e.run(tokio::sync::watch::channel(false).1).await.unwrap();
}

#[tokio::test]
async fn unchanged_conversations_cost_no_request_and_refresh_is_rare_and_quiet() {
    let latest = ago(3600);
    let server = quiet_slack(ok(json!({"channels": [{"id": "C1", "latest": latest}]}))).await;
    let store = Store::open_in_memory().unwrap();
    let bus = bus();

    // First start: history, then the refresh window (never read before).
    start(&engine(&server, &store, &bus, 3600)).await;
    let first = history_requests(&server).await;
    assert_eq!(first, 2, "one page of history, one refresh");

    // Next start: Slack's newest message is stored, refresh is not due yet.
    let mut rx = bus.subscribe();
    start(&engine(&server, &store, &bus, 3600)).await;
    assert_eq!(history_requests(&server).await, first);
    while let Ok(change) = rx.try_recv() {
        if let Change::Progress(Progress::Conversations { deep, .. }) = change {
            assert!(deep, "an unchanged conversation showed up in the fast pass");
        }
    }

    // Refresh due again: it runs, but after Idle and without any progress.
    let mut rx = bus.subscribe();
    start(&engine(&server, &store, &bus, 0)).await;
    assert_eq!(history_requests(&server).await, first + 1);
    let mut idle = false;
    while let Ok(change) = rx.try_recv() {
        match change {
            Change::Progress(Progress::Idle) => idle = true,
            Change::Progress(p) => assert!(!idle, "progress during the refresh: {p:?}"),
            _ => {}
        }
    }
    assert!(idle);
}

#[tokio::test]
async fn new_messages_or_unknown_latest_are_fetched() {
    // Newer than what gets stored: fetched every start.
    let server = quiet_slack(ok(json!({"channels": [{"id": "C1", "latest": ago(60)}]}))).await;
    let store = Store::open_in_memory().unwrap();
    let bus = bus();
    start(&engine(&server, &store, &bus, 3600)).await;
    let before = history_requests(&server).await;
    start(&engine(&server, &store, &bus, 3600)).await;
    assert_eq!(history_requests(&server).await, before + 1);

    // An unexpected `latest` shape: read markers survive, nothing is skipped.
    let server = quiet_slack(ok(json!({"channels": [
        {"id": "C1", "last_read": "5.000000", "latest": {"ts": ago(3600)}}
    ]})))
    .await;
    let store = Store::open_in_memory().unwrap();
    start(&engine(&server, &store, &bus, 3600)).await;
    let before = history_requests(&server).await;
    start(&engine(&server, &store, &bus, 3600)).await;
    assert_eq!(history_requests(&server).await, before + 1);
    let last_read: Option<String> = store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT last_read FROM conversations WHERE id = 'C1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(last_read.as_deref(), Some("5.000000"));

    // client.counts failing: back to fetching every conversation.
    let server = quiet_slack(ResponseTemplate::new(500)).await;
    let store = Store::open_in_memory().unwrap();
    start(&engine(&server, &store, &bus, 3600)).await;
    let before = history_requests(&server).await;
    start(&engine(&server, &store, &bus, 3600)).await;
    assert_eq!(history_requests(&server).await, before + 1);
}
