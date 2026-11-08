//! API client against a local mock server. Never hits real Slack.

mod support;
use support::field;

use flek_core::api::Client;
use flek_core::{Error, Secret};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "xoxc-fake-token";
const COOKIE: &str = "xoxd-fake%2Fcookie";

fn client(server: &MockServer) -> Client {
    Client::with_base_url(Secret::new(TOKEN), Secret::new(COOKIE), &server.uri()).unwrap()
}

fn auth_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "ok": true,
        "url": "https://fake.slack.com/",
        "team": "Fake Team",
        "user": "alice",
        "team_id": "T0FAKE",
        "user_id": "U0FAKE"
    }))
}

#[tokio::test]
async fn auth_test_sends_session_and_parses() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth.test"))
        // Like Slack desktop: token only in the body, no Authorization header.
        .and(|req: &wiremock::Request| !req.headers.contains_key("authorization"))
        .and(header("accept", "*/*"))
        .and(header("cookie", "d=xoxd-fake%2Fcookie"))
        .and(|req: &wiremock::Request| {
            req.headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with("multipart/form-data; boundary="))
        })
        .and(field("token", "xoxc-fake-token"))
        .respond_with(auth_ok())
        .expect(1)
        .mount(&server)
        .await;

    let me = client(&server).auth_test().await.unwrap();
    assert_eq!(me.team_id, "T0FAKE");
    assert_eq!(me.user, "alice");
    assert_eq!(me.url, "https://fake.slack.com/");
    assert_eq!(me.enterprise_id, None);
}

#[tokio::test]
async fn slack_error_is_mapped() {
    let server = MockServer::start().await;
    Mock::given(path("/auth.test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": false, "error": "missing_scope", "needed": "users:read"
        })))
        .mount(&server)
        .await;

    let err = client(&server).auth_test().await.unwrap_err();
    match &err {
        Error::Slack { code, needed } => {
            assert_eq!(code, "missing_scope");
            assert_eq!(needed.as_deref(), Some("users:read"));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(!err.is_auth_error());
    assert!(!err.to_string().contains(TOKEN));
}

#[tokio::test]
async fn invalid_auth_is_an_auth_error() {
    let server = MockServer::start().await;
    Mock::given(path("/auth.test"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"ok": false, "error": "invalid_auth"})),
        )
        .mount(&server)
        .await;
    assert!(client(&server)
        .auth_test()
        .await
        .unwrap_err()
        .is_auth_error());
}

#[tokio::test]
async fn retries_after_http_429() {
    let server = MockServer::start().await;
    Mock::given(path("/auth.test"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/auth.test"))
        .respond_with(auth_ok())
        .with_priority(2)
        .mount(&server)
        .await;

    assert_eq!(client(&server).auth_test().await.unwrap().user_id, "U0FAKE");
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn retries_on_ratelimited_body() {
    let server = MockServer::start().await;
    Mock::given(path("/auth.test"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Retry-After", "0")
                .set_body_json(serde_json::json!({"ok": false, "error": "ratelimited"})),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/auth.test"))
        .respond_with(auth_ok())
        .with_priority(2)
        .mount(&server)
        .await;

    assert!(client(&server).auth_test().await.is_ok());
}

#[tokio::test]
async fn gives_up_after_repeated_429() {
    let server = MockServer::start().await;
    Mock::given(path("/auth.test"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .mount(&server)
        .await;

    let err = client(&server).auth_test().await.unwrap_err();
    assert!(matches!(err, Error::Slack { ref code, .. } if code == "ratelimited"));
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test]
async fn rtm_connect_returns_url() {
    let server = MockServer::start().await;
    Mock::given(path("/rtm.connect"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true, "url": "wss://wss-primary.slack.com/websocket/fake",
            "team": {"id": "T0FAKE"}, "self": {"id": "U0FAKE"}
        })))
        .mount(&server)
        .await;
    assert_eq!(
        client(&server).rtm_connect().await.unwrap().url,
        "wss://wss-primary.slack.com/websocket/fake"
    );
}

#[tokio::test]
async fn requests_present_the_session_issuer() {
    use flek_core::api::ClientProfile;
    use wiremock::Request;

    const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 27_0_1) AppleWebKit/537.36 (KHTML, like Gecko) Slack/4.51.180 Chrome/150.0.7871.114 Electron/43.1.1 Safari/537.36 AppleSilicon Sonic Slack_SSB/4.51.180";
    let server = MockServer::start().await;
    Mock::given(path("/auth.test"))
        .and(|req: &Request| {
            req.headers.get("user-agent").and_then(|v| v.to_str().ok()) == Some(UA)
        })
        .and(header("origin", "https://app.slack.com"))
        .and(header("sec-fetch-site", "same-site"))
        .and(header("sec-fetch-mode", "cors"))
        .and(header("sec-ch-ua-platform", "\"macOS\""))
        .and(|req: &Request| {
            req.headers.get("sec-ch-ua").and_then(|v| v.to_str().ok())
                == Some(r#""Not;A=Brand";v="8", "Chromium";v="150""#)
        })
        .respond_with(auth_ok())
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::with_profile(
        Secret::new(TOKEN),
        Secret::new(COOKIE),
        &server.uri(),
        ClientProfile::from_user_agent(UA),
    )
    .unwrap();
    client.auth_test().await.unwrap();
}
