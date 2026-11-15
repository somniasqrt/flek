//! External upload flow against a mock Slack: the three steps, credential
//! isolation of the upload host, redirects, at-most-once completion,
//! cancellation and rate limits. Never touches real Slack or the Keychain.
use flek_core::{
    api::Client,
    store::{directory, Store},
    sync::upload::{Destination, Progress, Upload, UploadProgress, Uploaded},
    Error, Secret,
};
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::watch;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, Request, ResponseTemplate,
};

const BYTES: &[u8] = b"hello upload bytes";
const TOKEN: &str = "xoxc-fake-token";
const COOKIE: &str = "xoxd-fake-cookie";
const FILE_ID: &str = "F0UPLOAD1";
const TS: &str = "1700000100.000100";

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.with_conn(|c| {
        directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U1")?;
        c.execute_batch(
            "INSERT INTO users (id, team_id, name, real_name, display_name, deleted)
               VALUES ('U1','T1','me','Me','me',0);
             INSERT INTO conversations (id, team_id, kind, name, label)
               VALUES ('C1','T1','public_channel','general','general');",
        )?;
        Ok(())
    })
    .unwrap();
    s
}

fn client(server: &MockServer) -> Client {
    Client::with_base_url(Secret::new(TOKEN), Secret::new(COOKIE), &server.uri())
        .unwrap()
        .without_pacing()
}

fn file(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let p = dir.path().join("report.txt");
    std::fs::write(&p, BYTES).unwrap();
    p
}

fn destination(comment: &str, thread: Option<&str>) -> Destination {
    Destination {
        conversation_id: "C1".into(),
        thread_ts: thread.map(str::to_owned),
        comment: comment.into(),
        blocks: None,
    }
}

fn quiet() -> Progress {
    Arc::new(|_| {})
}

fn never() -> watch::Receiver<bool> {
    watch::channel(false).1
}

async fn ticket(server: &MockServer, upload_url: &str) {
    Mock::given(method("POST"))
        .and(path("/files.getUploadURLExternal"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "file_id": FILE_ID, "upload_url": upload_url})),
        )
        .mount(server)
        .await;
}

async fn complete_ok(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/files.completeUploadExternal"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "files": [{"id": FILE_ID}]})),
        )
        .mount(server)
        .await;
}

/// History as Slack would return it once the share exists.
async fn history_with_share(server: &MockServer, thread: Option<&str>) {
    let mut message = json!({"type": "message", "user": "U1", "ts": TS, "text": "see file",
        "files": [{"id": FILE_ID, "name": "report.txt", "mimetype": "text/plain", "size": 18}]});
    if let Some(root) = thread {
        message["thread_ts"] = root.into();
    }
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "has_more": false,
            "messages": [message]
        })))
        .mount(server)
        .await;
}

async fn history_empty(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/conversations.history"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": true, "has_more": false, "messages": []})),
        )
        .mount(server)
        .await;
}

async fn hits(server: &MockServer, p: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == p)
        .collect()
}

fn upload_url(server: &MockServer) -> String {
    format!("{}/upload/v1/ticket-secret", server.uri())
}

async fn send(
    upload: &mut Upload,
    client: &Client,
    store: &Store,
    d: Destination,
) -> Result<Uploaded, Error> {
    upload.send(client, store, d, never(), quiet()).await
}

#[tokio::test]
async fn three_step_flow_sends_no_session_credentials_to_the_upload_host() {
    let server = MockServer::start().await;
    ticket(&server, &upload_url(&server)).await;
    Mock::given(method("POST"))
        .and(path("/upload/v1/ticket-secret"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    complete_ok(&server).await;
    history_with_share(&server, Some("1700000000.000001")).await;
    let dir = tempfile::tempdir().unwrap();
    let store = store();
    let seen: Arc<Mutex<Vec<UploadProgress>>> = Arc::default();
    let sink = seen.clone();
    let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
    let done = upload
        .send(
            &client(&server),
            &store,
            destination("a <!channel> note & more", Some("1700000000.000001")),
            never(),
            Arc::new(move |p| sink.lock().unwrap().push(p)),
        )
        .await
        .unwrap();
    assert_eq!(done.file_id, FILE_ID);
    assert_eq!(done.ts.as_deref(), Some(TS));
    assert!(!done.completion_unknown && !done.not_stored_locally);

    // Step 1: the file's name and exact length are announced.
    let t = hits(&server, "/files.getUploadURLExternal").await;
    assert_eq!(t.len(), 1);
    let body = String::from_utf8_lossy(&t[0].body).into_owned();
    assert!(body.contains("report.txt") && body.contains(&BYTES.len().to_string()));

    // Step 2: exact bytes, and nothing that identifies the session.
    let up = hits(&server, "/upload/v1/ticket-secret").await;
    assert_eq!(up.len(), 1);
    assert_eq!(up[0].body, BYTES);
    for (name, value) in up[0].headers.iter() {
        let n = name.as_str().to_ascii_lowercase();
        let v = value.to_str().unwrap_or_default();
        assert!(
            !matches!(
                n.as_str(),
                "cookie" | "authorization" | "proxy-authorization"
            ),
            "credential header {n} leaked to the upload host"
        );
        assert!(!v.contains("xox"), "credential value in header {n}");
    }
    assert!(!String::from_utf8_lossy(&up[0].body).contains("xox"));
    assert!(up[0].url.query().is_none());

    // Step 3: once, with destination, thread and an escaped comment.
    let c = hits(&server, "/files.completeUploadExternal").await;
    assert_eq!(c.len(), 1);
    let body = String::from_utf8_lossy(&c[0].body).into_owned();
    for needle in [
        "channel_id",
        "C1",
        "thread_ts",
        "1700000000.000001",
        FILE_ID,
        "initial_comment",
        "&lt;!channel&gt; note &amp; more",
    ] {
        assert!(body.contains(needle), "completion lacks {needle}");
    }
    assert!(!body.contains("<!channel>"));

    // Stored through the normal history path, and progress ended complete.
    store
        .with_read(|c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM message_files WHERE file_id = ?1 AND ts = ?2",
                [FILE_ID, TS],
                |r| r.get(0),
            )?;
            assert_eq!(n, 1);
            Ok(())
        })
        .unwrap();
    let seen = seen.lock().unwrap();
    let stages: Vec<_> = seen.iter().map(|p| p.stage).collect();
    assert_eq!(stages.first(), Some(&"preparing"));
    assert!(stages.contains(&"completing") && stages.last() == Some(&"syncing"));
    let last = seen.iter().rfind(|p| p.stage == "uploading").unwrap();
    assert_eq!(
        (last.sent, last.total),
        (BYTES.len() as u64, BYTES.len() as u64)
    );
    // Progress events never carry the ticket URL.
    assert!(!serde_json::to_string(&*seen)
        .unwrap()
        .contains("ticket-secret"));
}

#[tokio::test]
async fn a_redirecting_upload_host_never_receives_the_file_and_completion_is_not_called() {
    let server = MockServer::start().await;
    ticket(&server, &upload_url(&server)).await;
    Mock::given(path("/upload/v1/ticket-secret"))
        .respond_with(ResponseTemplate::new(307).insert_header("location", "/elsewhere"))
        .mount(&server)
        .await;
    Mock::given(path("/elsewhere"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    complete_ok(&server).await;
    history_empty(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
    let err = send(
        &mut upload,
        &client(&server),
        &store(),
        destination("", None),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::HttpStatus(307)), "{err:?}");
    assert!(hits(&server, "/elsewhere").await.is_empty());
    assert!(hits(&server, "/files.completeUploadExternal")
        .await
        .is_empty());
    // The error text carries neither the ticket nor any credential.
    let text = format!("{err} {err:?}");
    assert!(!text.contains("ticket-secret") && !text.contains("xox"));
}

#[tokio::test]
async fn upload_urls_outside_slack_are_refused_before_any_bytes_move() {
    for bad in [
        "https://evil.example/upload/x",
        "http://files.slack.com/upload/x",
        "https://files.slack.com.evil.example/upload/x",
        "https://user:pw@files.slack.com/upload/x",
        "file:///etc/passwd",
        "not a url",
    ] {
        let server = MockServer::start().await;
        ticket(&server, bad).await;
        complete_ok(&server).await;
        history_empty(&server).await;
        let dir = tempfile::tempdir().unwrap();
        let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
        let err = send(
            &mut upload,
            &client(&server),
            &store(),
            destination("", None),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::File(_)), "{bad}: {err:?}");
        assert!(!format!("{err}").contains("evil"), "{bad}");
        assert!(hits(&server, "/files.completeUploadExternal")
            .await
            .is_empty());
    }
}

#[tokio::test]
async fn an_ambiguous_completion_is_never_replayed() {
    let server = MockServer::start().await;
    ticket(&server, &upload_url(&server)).await;
    Mock::given(path("/upload/v1/ticket-secret"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(path("/files.completeUploadExternal"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    history_empty(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let store = store();
    let client = client(&server);
    let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
    let first = send(&mut upload, &client, &store, destination("x", None))
        .await
        .unwrap();
    assert!(first.completion_unknown && first.not_stored_locally && first.ts.is_none());
    // A retry on the same upload re-checks history only: no second share, no new bytes.
    let second = send(&mut upload, &client, &store, destination("x", None))
        .await
        .unwrap();
    assert!(second.completion_unknown);
    assert_eq!(
        hits(&server, "/files.completeUploadExternal").await.len(),
        1
    );
    assert_eq!(hits(&server, "/upload/v1/ticket-secret").await.len(), 1);
    assert_eq!(hits(&server, "/files.getUploadURLExternal").await.len(), 1);
}

#[tokio::test]
async fn a_lost_completion_response_resolves_once_the_message_arrives() {
    let server = MockServer::start().await;
    ticket(&server, &upload_url(&server)).await;
    Mock::given(path("/upload/v1/ticket-secret"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    // A 200 whose body is not JSON: the share may have happened.
    Mock::given(path("/files.completeUploadExternal"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<<garbled"))
        .mount(&server)
        .await;
    history_empty(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let store = store();
    let client = client(&server);
    let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
    let first = send(&mut upload, &client, &store, destination("", None))
        .await
        .unwrap();
    assert!(first.completion_unknown);
    server.reset().await;
    ticket(&server, &upload_url(&server)).await;
    history_with_share(&server, None).await;
    let second = send(&mut upload, &client, &store, destination("", None))
        .await
        .unwrap();
    assert_eq!(second.ts.as_deref(), Some(TS));
    assert!(!second.completion_unknown && !second.not_stored_locally);
    assert!(hits(&server, "/files.completeUploadExternal")
        .await
        .is_empty());
    assert!(hits(&server, "/upload/v1/ticket-secret").await.is_empty());
}

#[tokio::test]
async fn a_definitive_refusal_allows_one_completion_retry_without_reuploading() {
    let server = MockServer::start().await;
    ticket(&server, &upload_url(&server)).await;
    Mock::given(path("/upload/v1/ticket-secret"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(path("/files.completeUploadExternal"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok": false, "error": "channel_not_found"})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    complete_ok(&server).await;
    history_with_share(&server, None).await;
    let dir = tempfile::tempdir().unwrap();
    let store = store();
    let client = client(&server);
    let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
    let err = send(&mut upload, &client, &store, destination("", None))
        .await
        .unwrap_err();
    assert!(matches!(&err, Error::Slack { code, .. } if code == "channel_not_found"));
    let done = send(&mut upload, &client, &store, destination("", None))
        .await
        .unwrap();
    assert_eq!(done.ts.as_deref(), Some(TS));
    assert_eq!(
        hits(&server, "/files.completeUploadExternal").await.len(),
        2
    );
    assert_eq!(hits(&server, "/upload/v1/ticket-secret").await.len(), 1);
    assert_eq!(hits(&server, "/files.getUploadURLExternal").await.len(), 1);
}

#[tokio::test]
async fn the_destination_is_frozen_after_the_first_attempt() {
    let server = MockServer::start().await;
    ticket(&server, &upload_url(&server)).await;
    Mock::given(path("/upload/v1/ticket-secret"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(path("/files.completeUploadExternal"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    history_empty(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let store = store();
    let client = client(&server);
    let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
    send(&mut upload, &client, &store, destination("one", None))
        .await
        .unwrap();
    let err = send(&mut upload, &client, &store, destination("two", None))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Usage(_)));
    assert_eq!(
        hits(&server, "/files.completeUploadExternal").await.len(),
        1
    );
}

#[tokio::test]
async fn cancellation_before_start_makes_no_request() {
    let server = MockServer::start().await;
    ticket(&server, &upload_url(&server)).await;
    complete_ok(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
    let (_tx, rx) = watch::channel(true);
    let err = upload
        .send(
            &client(&server),
            &store(),
            destination("", None),
            rx,
            quiet(),
        )
        .await
        .unwrap_err();
    assert!(matches!(&err, Error::Usage(m) if m.contains("cancelled")));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn cancellation_interrupts_a_slow_transfer_and_never_completes() {
    let server = MockServer::start().await;
    ticket(&server, &upload_url(&server)).await;
    Mock::given(path("/upload/v1/ticket-secret"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
        .mount(&server)
        .await;
    complete_ok(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
    let (tx, rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        tx.send_replace(true);
    });
    let started = Instant::now();
    let err = upload
        .send(
            &client(&server),
            &store(),
            destination("", None),
            rx,
            quiet(),
        )
        .await
        .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(matches!(&err, Error::Usage(m) if m.contains("cancelled")));
    assert!(hits(&server, "/files.completeUploadExternal")
        .await
        .is_empty());
}

#[tokio::test]
async fn a_rate_limited_upload_host_is_retried_from_byte_zero() {
    let server = MockServer::start().await;
    ticket(&server, &upload_url(&server)).await;
    Mock::given(path("/upload/v1/ticket-secret"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(path("/upload/v1/ticket-secret"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    complete_ok(&server).await;
    history_with_share(&server, None).await;
    let dir = tempfile::tempdir().unwrap();
    let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
    let done = send(
        &mut upload,
        &client(&server),
        &store(),
        destination("", None),
    )
    .await
    .unwrap();
    assert_eq!(done.ts.as_deref(), Some(TS));
    let up = hits(&server, "/upload/v1/ticket-secret").await;
    assert_eq!(up.len(), 3);
    assert!(up.iter().all(|r| r.body == BYTES));
    assert_eq!(
        hits(&server, "/files.completeUploadExternal").await.len(),
        1
    );
}

#[tokio::test]
async fn persistent_429_on_the_upload_host_fails_without_completion() {
    let server = MockServer::start().await;
    ticket(&server, &upload_url(&server)).await;
    Mock::given(path("/upload/v1/ticket-secret"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .mount(&server)
        .await;
    complete_ok(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
    let err = send(
        &mut upload,
        &client(&server),
        &store(),
        destination("", None),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::HttpStatus(429)), "{err:?}");
    assert_eq!(hits(&server, "/upload/v1/ticket-secret").await.len(), 4);
    assert!(hits(&server, "/files.completeUploadExternal")
        .await
        .is_empty());
}

#[tokio::test]
async fn a_rate_limited_ticket_or_completion_call_is_retried_by_the_api_client() {
    let server = MockServer::start().await;
    Mock::given(path("/files.getUploadURLExternal"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    ticket(&server, &upload_url(&server)).await;
    Mock::given(path("/upload/v1/ticket-secret"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(path("/files.completeUploadExternal"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    complete_ok(&server).await;
    history_with_share(&server, None).await;
    let dir = tempfile::tempdir().unwrap();
    let mut upload = Upload::from_path(&file(&dir)).await.unwrap();
    let done = send(
        &mut upload,
        &client(&server),
        &store(),
        destination("", None),
    )
    .await
    .unwrap();
    assert!(!done.completion_unknown);
    assert_eq!(hits(&server, "/files.getUploadURLExternal").await.len(), 2);
    assert_eq!(
        hits(&server, "/files.completeUploadExternal").await.len(),
        2
    );
    assert_eq!(hits(&server, "/upload/v1/ticket-secret").await.len(), 1);
}

#[tokio::test]
async fn staging_validates_names_sizes_offsets_and_destinations() {
    for (name, len) in [
        ("", 1),
        ("a/b", 1),
        ("a\\b", 1),
        ("tab\there", 1),
        ("ok", 0),
        ("ok", 100 * 1024 * 1024 + 1),
    ] {
        assert!(matches!(
            Upload::stage(name.into(), len),
            Err(Error::Usage(_))
        ));
    }
    assert!(Upload::stage("ok".into(), 100 * 1024 * 1024).is_ok());

    let mut upload = Upload::stage("ok".into(), 4).unwrap();
    // Not fully staged yet.
    let server = MockServer::start().await;
    let err = send(
        &mut upload,
        &client(&server),
        &store(),
        destination("", None),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::Usage(_)));
    assert!(upload.append(1, b"ab").await.is_err(), "gap");
    assert!(upload.append(0, b"abc").await.is_ok());
    assert!(
        upload.append(0, b"abc").await.is_ok(),
        "exact replay is idempotent"
    );
    assert!(
        upload.append(0, b"xyz").await.is_err(),
        "conflicting replay"
    );
    assert!(upload.append(3, b"de").await.is_err(), "over length");
    assert!(upload.append(3, b"d").await.is_ok());
    let bad_ts = send(
        &mut upload,
        &client(&server),
        &store(),
        destination("", Some("nope")),
    )
    .await
    .unwrap_err();
    assert!(matches!(bad_ts, Error::Usage(_)));
    let no_channel = Destination {
        conversation_id: " ".into(),
        ..destination("", None)
    };
    assert!(send(&mut upload, &client(&server), &store(), no_channel)
        .await
        .is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_changed_source_is_refused_while_staging() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        Upload::from_path(dir.path()).await,
        Err(Error::Usage(_))
    ));
    let empty = dir.path().join("empty");
    std::fs::write(&empty, b"").unwrap();
    assert!(matches!(
        Upload::from_path(&empty).await,
        Err(Error::Usage(_))
    ));
    assert!(Upload::from_path(&dir.path().join("missing"))
        .await
        .is_err());
}

#[tokio::test]
async fn debug_output_of_upload_state_hides_the_ticket_url() {
    let server = MockServer::start().await;
    ticket(&server, &upload_url(&server)).await;
    let t = client(&server)
        .get_upload_url("report.txt", BYTES.len() as u64)
        .await
        .unwrap();
    assert_eq!(t.file_id, FILE_ID);
    assert!(!format!("{t:?}").contains("ticket-secret"));
}
