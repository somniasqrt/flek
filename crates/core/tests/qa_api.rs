//! QA: rate limiting and secrecy of the session (AGENTS.md §4, §6).

mod qa_common;

use std::time::{Duration, Instant};

use qa_common::*;
use serde_json::json;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use flek_core::api::AuthTest;
use flek_core::auth::keychain::Credentials;
use flek_core::auth::Session;
use flek_core::store::Store;
use flek_core::Secret;

fn auth_ok() -> ResponseTemplate {
    ok(
        json!({"url": "https://fake.slack.com/", "team": "Fake", "user": "alice",
              "team_id": "T0FAKE", "user_id": "U0FAKE"}),
    )
}

fn assert_no_secret(what: &str, s: &str) {
    assert!(
        !s.contains(TOKEN_TAIL) && !s.contains(COOKIE_TAIL),
        "{what} contains a secret"
    );
}

fn request_leaks(req: &Request) -> bool {
    let mut all = format!("{} {:?}", req.url, req.headers);
    all.push_str(&String::from_utf8_lossy(&req.body));
    all.contains(TOKEN_TAIL) || all.contains(COOKIE_TAIL)
}

// ---- rate limiting ----

#[tokio::test]
async fn retry_after_is_waited_for() {
    let server = MockServer::start().await;
    Mock::given(path("/auth.test"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/auth.test"))
        .respond_with(auth_ok())
        .with_priority(2)
        .mount(&server)
        .await;
    let start = Instant::now();
    client(&server).auth_test().await.unwrap();
    assert!(
        start.elapsed() >= Duration::from_millis(900),
        "retried after {:?}, Retry-After was 1 s",
        start.elapsed()
    );
}

#[tokio::test]
async fn ratelimited_body_forever_gives_up() {
    let server = MockServer::start().await;
    Mock::given(path("/auth.test"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Retry-After", "0")
                .set_body_json(json!({"ok": false, "error": "ratelimited"})),
        )
        .mount(&server)
        .await;
    let r = tokio::time::timeout(Duration::from_secs(30), client(&server).auth_test()).await;
    let err = r.expect("client looped on `ratelimited`").unwrap_err();
    assert!(err.to_string().contains("ratelimited"));
    let n = server.received_requests().await.unwrap().len();
    assert!((2..=10).contains(&n), "{n} requests");
}

#[tokio::test]
async fn garbage_retry_after_does_not_hang_or_panic() {
    let server = MockServer::start().await;
    Mock::given(path("/auth.test"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "soon-ish"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/auth.test"))
        .respond_with(auth_ok())
        .with_priority(2)
        .mount(&server)
        .await;
    let r = tokio::time::timeout(Duration::from_secs(70), client(&server).auth_test()).await;
    assert!(r.expect("hung on a garbage Retry-After").is_ok());
}

#[tokio::test]
async fn history_429_mid_backfill_loses_nothing() {
    let server = MockServer::start().await;
    let store = store();
    Mock::given(path("/conversations.history"))
        .respond_with(ok(json!({"messages": [
            {"ts": "4.000000", "user": "U1", "text": "four"},
            {"ts": "3.000000", "user": "U1", "text": "three"}
        ], "has_more": true, "response_metadata": {"next_cursor": "p2"}})))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.history"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .up_to_n_times(2)
        .with_priority(2)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "2.000000", "user": "U1", "text": "two"},
            {"ts": "1.000000", "user": "U1", "text": "one"}
        ])))
        .with_priority(3)
        .mount(&server)
        .await;
    let stats = backfill(&client(&server), &store, &Default::default()).await;
    assert!(stats.complete);
    assert_eq!(count(&store, "SELECT COUNT(*) FROM messages"), 4);
}

// ---- secrets ----

#[test]
fn debug_of_session_types_is_redacted() {
    let creds = Credentials {
        team_id: "T0FAKE".into(),
        user_id: "U0FAKE".into(),
        token: Secret::new(TOKEN),
        cookie: Secret::new(COOKIE),
        origin: None,
    };
    assert_no_secret("Credentials", &format!("{creds:?}"));
    let session = Session {
        credentials: creds,
        identity: AuthTest {
            url: "https://fake.slack.com/".into(),
            team: "Fake".into(),
            user: "alice".into(),
            team_id: "T0FAKE".into(),
            user_id: "U0FAKE".into(),
            enterprise_id: None,
        },
    };
    assert_no_secret("Session", &format!("{session:?} {session:#?}"));
    let s = Secret::new(TOKEN);
    assert_no_secret("Secret", &format!("{s:?} {s} {s:#?}"));
    let server_less = flek_core::api::Client::with_base_url(
        Secret::new(TOKEN),
        Secret::new(COOKIE),
        "http://127.0.0.1:9",
    )
    .unwrap();
    assert_no_secret("Client", &format!("{server_less:?}"));
}

#[test]
fn desktop_session_types_are_redacted() {
    let teams = flek_core::auth::desktop_import::parse_teams(
        &json!({"teams": {"T0FAKE": {"token": TOKEN, "name": "Fake", "url": "https://fake.slack.com/"}}})
            .to_string(),
    )
    .unwrap();
    assert_no_secret("TeamSession", &format!("{teams:?}"));
}

// FINDING: a malformed localConfig_v2 makes parse_teams fail with a serde
// message that quotes the offending value, which can be the xoxc token.
#[test]
fn malformed_local_config_error_does_not_quote_token() {
    let err = flek_core::auth::desktop_import::parse_teams(
        &json!({"teams": {"T0FAKE": TOKEN}}).to_string(),
    )
    .unwrap_err();
    assert_no_secret("parse_teams error", &format!("{err} {err:?}"));
}

#[tokio::test]
async fn errors_from_echoing_servers_do_not_carry_secrets() {
    let server = MockServer::start().await;
    let echo = format!("<html>bad request token={TOKEN} cookie=d={COOKIE}</html>");
    Mock::given(path("/auth.test"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(echo.clone(), "text/html"))
        .mount(&server)
        .await;
    Mock::given(path("/users.list"))
        .respond_with(ResponseTemplate::new(400).set_body_raw(echo.clone(), "text/plain"))
        .mount(&server)
        .await;
    Mock::given(path("/rtm.connect"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"ok": false, "error": "invalid_auth", "warning": TOKEN, "detail": COOKIE}),
        ))
        .mount(&server)
        .await;
    let c = client(&server);
    let e1 = c.auth_test().await.unwrap_err();
    assert_no_secret("non-JSON error", &format!("{e1} {e1:?}"));
    let e2 = c.users_list().await.unwrap_err();
    assert_no_secret("HTTP 400 error", &format!("{e2} {e2:?}"));
    let e3 = c.rtm_connect().await.unwrap_err();
    assert_no_secret("Slack error", &format!("{e3} {e3:?}"));
}

#[tokio::test]
async fn transport_error_does_not_carry_secrets() {
    // Nothing listens on port 9: the reqwest error includes the URL.
    let c = flek_core::api::Client::with_base_url(
        Secret::new(TOKEN),
        Secret::new(COOKIE),
        "http://127.0.0.1:9",
    )
    .unwrap();
    let e = c.auth_test().await.unwrap_err();
    assert_no_secret("transport error", &format!("{e} {e:?}"));
}

#[tokio::test]
async fn api_redirect_to_another_host_does_not_forward_session() {
    let slack = MockServer::start().await;
    let other = MockServer::start().await;
    // `localhost` vs `127.0.0.1`: a different host for the redirect.
    let target = other.uri().replace("127.0.0.1", "localhost");
    for (p, status) in [("/auth.test", 302), ("/users.list", 307)] {
        Mock::given(path(p))
            .respond_with(
                ResponseTemplate::new(status).insert_header("Location", format!("{target}/steal")),
            )
            .mount(&slack)
            .await;
    }
    Mock::given(path("/steal"))
        .respond_with(auth_ok())
        .mount(&other)
        .await;
    let c = client(&slack);
    let _ = c.auth_test().await;
    let _ = c.users_list().await;
    for req in other.received_requests().await.unwrap() {
        assert!(!request_leaks(&req), "session forwarded to {}", req.url);
    }
}

#[tokio::test]
async fn file_download_redirect_to_another_host_does_not_forward_session() {
    let slack = MockServer::start().await;
    let other = MockServer::start().await;
    let target = other.uri().replace("127.0.0.1", "localhost");
    Mock::given(path("/files-pri/F1/a.txt"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", format!("{target}/a.txt")),
        )
        .mount(&slack)
        .await;
    Mock::given(path("/a.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("data", "text/plain"))
        .mount(&other)
        .await;
    let c = client(&slack);
    let dir = tempfile::tempdir().unwrap();
    let _ = c
        .download(
            &format!("{}/files-pri/F1/a.txt", slack.uri()),
            &dir.path().join("a.txt"),
            false,
        )
        .await;
    for req in other.received_requests().await.unwrap() {
        assert!(!request_leaks(&req), "session forwarded to {}", req.url);
    }
}

#[tokio::test]
async fn database_file_never_contains_session() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("flek.db");
    let store = Store::open(&db).unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
                 INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');",
            )?;
            Ok(())
        })
        .unwrap();
    Mock::given(path("/conversations.history"))
        .respond_with(history(json!([
            {"ts": "2.000000", "user": "U1", "text": "file",
             "files": [{"id": "F1", "name": "a.txt", "url_private_download": format!("{}/files-pri/F1/a.txt", server.uri())}]},
            {"ts": "1.000000", "user": "U1", "text": "hello"}
        ])))
        .mount(&server)
        .await;
    backfill(&client(&server), &store, &Default::default()).await;
    drop(store);
    let mut bytes = Vec::new();
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        bytes.extend(std::fs::read(entry.unwrap().path()).unwrap());
    }
    let text = String::from_utf8_lossy(&bytes);
    assert_no_secret("database file", &text);
}
