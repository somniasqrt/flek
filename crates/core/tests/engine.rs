//! Sync engine: fast pass first, priority, deep backfill after.

mod common;
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
    store
        .with_conn(|c| {
            flek_core::store::directory::upsert_team(
                c,
                "T1",
                "Fake",
                "https://fake.slack.com/",
                "U0ME",
            )
        })
        .unwrap();
    Mock::given(path("/search.messages"))
        .respond_with(ok(json!({"messages":{"total":1,"matches":[
            {"channel":{"id":"CSEARCH","name":"discovered"},"ts":"1700000000.000001","text":"remote evidence"}
        ]}}))).mount(&server).await;
    let options = flek_core::search::Options {
        limit: 1,
        ..Default::default()
    };
    let prepared = store
        .with_read(|c| flek_core::search::prepare(c, "T1", "remote", &options))
        .unwrap();
    flek_core::search::run(&store, "T1", prepared, &options, Some(&client), ("", ""))
        .await
        .unwrap();
    Mock::given(path("/conversations.history"))
        .and(field("channel", "CSEARCH"))
        .respond_with(ok(json!({"messages":[]})))
        .with_priority(1)
        .expect(0)
        .mount(&server)
        .await;
    store
        .with_read(|c| {
            assert!(!flek_core::store::directory::list_conversations(c, "T1")?
                .iter()
                .any(|v| v.id == "CSEARCH"));
            assert!(!flek_core::views::conversations(c, "T1", true)?
                .iter()
                .any(|v| v.id == "CSEARCH"));
            Ok(())
        })
        .unwrap();
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

#[tokio::test]
async fn reconnect_pass_refreshes_only_when_due() {
    let latest = ago(3600);
    let server = quiet_slack(ok(json!({"channels":[{"id":"C1","latest":latest}]}))).await;
    let store = Store::open_in_memory().unwrap();
    let e = engine(&server, &store, &bus(), 3600);
    start(&e).await;
    let initial = history_requests(&server).await;
    let mut limiter = flek_core::sync::backfill::RateLimiter::new(std::time::Duration::ZERO);
    e.pass(&mut limiter, false).await.unwrap();
    assert_eq!(
        history_requests(&server).await,
        initial,
        "quick reconnect must be free"
    );
    store
        .with_conn(|c| {
            c.execute("UPDATE refresh_runs SET refreshed_at=0", [])?;
            Ok(())
        })
        .unwrap();
    e.pass(&mut limiter, false).await.unwrap();
    assert_eq!(
        history_requests(&server).await,
        initial + 1,
        "reconnect after refresh interval must revisit edits"
    );
}

#[tokio::test]
async fn broken_settings_keep_sync_running_and_last_good_options() {
    let server = quiet_slack(ok(json!({"channels":[]}))).await;
    let store = Store::open_in_memory().unwrap();
    let events = bus();
    let mut changes = events.subscribe();
    let mut e = engine(&server, &store, &events, 3600);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let good = flek_core::settings::Settings {
        refresh_days: 0,
        max_file_bytes: Some(0),
        ..Default::default()
    };
    good.save(&path).unwrap();
    e.options.settings_path = Some(path.clone());
    e.options.files = Some((dir.path().join("cache"), None));
    let resolved = e.options.reload_settings().unwrap();
    assert_eq!(resolved.refresh_days, 0);
    std::fs::write(&path, b"{broken").unwrap();
    let resolved = e.options.reload_settings().unwrap();
    assert_eq!(resolved.refresh_days, 0);
    assert_eq!(resolved.files.unwrap().1, Some(0));
    start(&e).await;
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    assert!(std::iter::from_fn(|| changes.try_recv().ok())
        .any(|c| matches!(c, Change::SettingsError { error: Some(_) })));
    good.save(&path)
        .expect_err("broken file must be preserved until repaired");
    std::fs::remove_file(&path).unwrap();
    good.save(&path).unwrap();
    start(&e).await;
    assert!(std::iter::from_fn(|| changes.try_recv().ok())
        .any(|c| matches!(c, Change::SettingsError { error: None })));
}

#[tokio::test]
async fn live_pass_with_zero_refresh_interval_does_not_refresh_on_quick_retry() {
    let server = quiet_slack(ok(json!({"channels":[{"id":"C1","latest":ago(3600)}]}))).await;
    let store = Store::open_in_memory().unwrap();
    let e = engine(&server, &store, &bus(), 0);
    start(&e).await;
    let initial = history_requests(&server).await;
    let mut limiter = flek_core::sync::backfill::RateLimiter::new(std::time::Duration::ZERO);
    e.pass(&mut limiter, false).await.unwrap();
    assert_eq!(history_requests(&server).await, initial);
}

#[tokio::test]
async fn engine_retries_partial_pass_without_reconnecting_socket() {
    use flek_core::sync::live::ConnectionState;
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", listener.local_addr().unwrap());
    let connections = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = connections.clone();
    let socket = tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(async move {
                let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                ws.send(tokio_tungstenite::tungstenite::Message::text(
                    json!({"type":"hello"}).to_string(),
                ))
                .await
                .unwrap();
                while ws.next().await.is_some() {}
            });
        }
    });
    let server = MockServer::start().await;
    for (method, payload) in [
        ("users.list", json!({"members":[]})),
        (
            "users.conversations",
            json!({"channels":[{"id":"C1","name":"one"}]}),
        ),
        ("client.counts", json!({"channels":[]})),
    ] {
        Mock::given(path(format!("/{method}")))
            .respond_with(ok(payload))
            .mount(&server)
            .await;
    }
    Mock::given(path("/conversations.history"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":"fatal_error"})),
        )
        .with_priority(1)
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.history"))
        .respond_with(ok(json!({"messages":[],"has_more":false})))
        .with_priority(2)
        .mount(&server)
        .await;
    let store = Store::open_in_memory().unwrap();
    let events = bus();
    let mut changes = events.subscribe();
    let mut e = engine(&server, &store, &events, 3600);
    e.client = common::websocket_client(&server, &url)
        .await
        .without_pacing();
    e.options.follow = true;
    e.options.refresh_days = 0;
    e.options.follow_config.backoff_min = Duration::from_millis(10);
    e.options.follow_config.backoff_max = Duration::from_millis(40);
    let (stop, rx) = tokio::sync::watch::channel(false);
    let watch = async {
        let mut failed = false;
        loop {
            match changes.recv().await.unwrap() {
                Change::Connection(ConnectionState::SyncIncomplete { .. }) => failed = true,
                Change::Connection(ConnectionState::Connected) if failed => {
                    stop.send(true).unwrap();
                    break;
                }
                _ => {}
            }
        }
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(e.run(rx), watch)
    })
    .await
    .unwrap();
    result.unwrap();
    assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 1);
    socket.abort();
}
