//! API client against a local mock server. Never hits real Slack.

use flek_core::api::Client;
use flek_core::{Error, Secret};
use wiremock::matchers::{body_string_contains, header, method, path};
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
        .and(header("authorization", "Bearer xoxc-fake-token"))
        .and(header("cookie", "d=xoxd-fake%2Fcookie"))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .and(body_string_contains("token=xoxc-fake-token"))
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
