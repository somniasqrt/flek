//! Partial history must never turn a decode failure into a deletion.
use flek_core::{
    api::Client,
    store::{
        directory,
        messages::{self, MessageRow, SyncState},
        Store,
    },
    sync::{
        backfill::{backfill_conversation, RateLimiter},
        threads::sync_thread,
        SyncOptions,
    },
    Secret,
};
use serde_json::json;
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

fn ago(seconds: u64) -> String {
    format!(
        "{}.000001",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - seconds
    )
}

#[tokio::test]
async fn malformed_history_does_not_delete_saved_content() {
    let server = MockServer::start().await;
    let root = ago(7200);
    let reply = ago(3600);
    Mock::given(path("/conversations.history"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"ok":true,"messages":[{"ts":root,"text":42}],"has_more":false}),
            ),
        )
        .mount(&server)
        .await;
    Mock::given(path("/conversations.replies")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"messages":[{"ts":root,"text":"root"},{"ts":reply,"thread_ts":root,"text":42}],"has_more":false}))).mount(&server).await;
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
            directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U1")?;
            c.execute(
                "INSERT INTO conversations(id,team_id,kind) VALUES('C1','T1','public_channel')",
                [],
            )?;
            messages::upsert_messages(
                c,
                "C1",
                &[
                    MessageRow {
                        ts: root.clone(),
                        text: "saved root".into(),
                        raw_json: "{}".into(),
                        ..Default::default()
                    },
                    MessageRow {
                        ts: reply.clone(),
                        thread_ts: Some(root.clone()),
                        text: "saved reply".into(),
                        raw_json: "{}".into(),
                        ..Default::default()
                    },
                ],
            )?;
            messages::set_sync_state(
                c,
                "C1",
                &SyncState {
                    oldest_ts: Some(root.clone()),
                    newest_ts: Some(reply.clone()),
                    complete: true,
                },
            )?;
            Ok(())
        })
        .unwrap();
    let mut limiter = RateLimiter::new(std::time::Duration::ZERO);
    let _ = backfill_conversation(
        &client,
        &store,
        &mut limiter,
        "C1",
        &SyncOptions::refresh_days(7),
    )
    .await;
    let _ = sync_thread(&client, &store, &mut limiter, "C1", &root).await;
    store
        .with_conn(|c| {
            let deleted: i64 =
                c.query_row("SELECT COUNT(*) FROM messages WHERE deleted=1", [], |r| {
                    r.get(0)
                })?;
            assert_eq!(
                deleted, 0,
                "unparseable content is not evidence of deletion"
            );
            Ok(())
        })
        .unwrap();
}
