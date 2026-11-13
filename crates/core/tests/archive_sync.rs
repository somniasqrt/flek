//! Sync must retain archived evidence when Slack serves partial history.
mod support;

use std::time::Duration;

use flek_core::{
    api::Client,
    store::{
        files::FileRow,
        messages::{self, MessageRow, SyncState},
        Store,
    },
    sync::{
        backfill::{backfill_conversation, RateLimiter},
        channel_details,
        threads::{sync_thread, sync_threads},
        ts_days_ago, SyncOptions,
    },
    Secret,
};
use serde_json::{json, Value};
use support::field;
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

async fn setup() -> (tempfile::TempDir, MockServer, Client, Store, RateLimiter) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("archive.db")).unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Fake');
            INSERT INTO conversations (id, team_id, kind) VALUES ('C1', 'T1', 'public_channel');",
            )?;
            Ok(())
        })
        .unwrap();
    let server = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    (dir, server, client, store, RateLimiter::new(Duration::ZERO))
}

fn row(ts: &str, root: Option<&str>) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        thread_ts: root.map(str::to_owned),
        text: "archived content".into(),
        raw_json: "{}".into(),
        ..Default::default()
    }
}

fn seed(store: &Store, rows: &[MessageRow]) {
    store
        .with_conn(|c| messages::upsert_messages(c, "C1", rows))
        .unwrap();
}

fn visible(store: &Store, ts: &str) -> bool {
    store
        .with_read(|c| {
            Ok(c.query_row(
                "SELECT deleted = 0 FROM messages WHERE conversation_id = 'C1' AND ts = ?1",
                [ts],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

fn page(messages: Value, more: bool) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"ok":true,"messages":messages,"has_more":more}))
}

#[tokio::test]
async fn thread_missing_old_replies_keeps_the_archive_even_with_root() {
    let (_dir, server, client, store, mut limiter) = setup().await;
    let old = ts_days_ago(100);
    let recent = ts_days_ago(2);
    seed(
        &store,
        &[
            row("100.000000", None),
            row(&old, Some("100.000000")),
            row(&recent, Some("100.000000")),
        ],
    );
    Mock::given(path("/conversations.replies"))
        .respond_with(page(json!([{"ts":"100.000000","text":"root"}]), false))
        .mount(&server)
        .await;
    let stats = sync_thread(&client, &store, &mut limiter, "C1", "100.000000")
        .await
        .unwrap();
    assert!(visible(&store, &old));
    assert!(!visible(&store, &recent));
    assert_eq!(stats.deleted, 1);
}

#[tokio::test]
async fn thread_without_root_keeps_old_and_recent_missing_replies() {
    let (_dir, server, client, store, mut limiter) = setup().await;
    let old = ts_days_ago(100);
    let recent = ts_days_ago(2);
    let fresh = ts_days_ago(1);
    seed(
        &store,
        &[
            row("100.000000", None),
            row(&old, Some("100.000000")),
            row(&recent, Some("100.000000")),
        ],
    );
    Mock::given(path("/conversations.replies"))
        .respond_with(page(
            json!([{"ts":fresh,"thread_ts":"100.000000","text":"fresh reply"}]),
            false,
        ))
        .mount(&server)
        .await;
    let stats = sync_thread(&client, &store, &mut limiter, "C1", "100.000000")
        .await
        .unwrap();
    assert_eq!(stats.deleted, 0);
    assert!(visible(&store, &old));
    assert!(visible(&store, &recent));
    assert!(visible(&store, &fresh));
}

#[tokio::test]
async fn failed_thread_is_remembered_other_threads_continue_and_retry_recovers() {
    let (dir, server, client, store, mut limiter) = setup().await;
    let roots = ["200.000000", "100.000000"];
    seed(
        &store,
        &roots
            .iter()
            .map(|ts| MessageRow {
                reply_count: 1,
                latest_reply: None,
                ..row(ts, None)
            })
            .collect::<Vec<_>>(),
    );
    Mock::given(path("/conversations.replies"))
        .and(field("ts", roots[0]))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":false,"error":"thread_not_found"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.replies")).and(field("ts", roots[1]))
        .respond_with(page(json!([{"ts":roots[1],"text":"root","reply_count":1,"latest_reply":"101.000000"},{"ts":"101.000000","thread_ts":roots[1],"text":"reply"}]), false))
        .expect(2).mount(&server).await;
    let stats = sync_threads(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(stats.threads, 1);
    assert!(visible(&store, "101.000000"));
    drop(store);
    let store = Store::open(&dir.path().join("archive.db")).unwrap();
    assert!(store
        .with_read(|c| messages::stale_threads(c, "C1"))
        .unwrap()
        .is_empty());
    for (opts, expected) in [
        (SyncOptions::default(), 0),
        (
            SyncOptions {
                full: true,
                ..Default::default()
            },
            1,
        ),
        (SyncOptions::refresh_days(7), 0),
    ] {
        assert_eq!(
            sync_threads(&client, &store, &mut limiter, "C1", &opts)
                .await
                .unwrap()
                .threads,
            expected
        );
    }
    server.verify().await;
    server.reset().await;
    store
        .with_conn(|c| {
            c.execute(
                "UPDATE thread_sync SET retry_after = 0 WHERE thread_ts = ?1",
                [roots[0]],
            )?;
            Ok(())
        })
        .unwrap();
    Mock::given(path("/conversations.replies")).and(field("ts", roots[0]))
        .respond_with(page(json!([{"ts":roots[0],"text":"recovered","reply_count":1,"latest_reply":"201.000000"},{"ts":"201.000000","thread_ts":roots[0],"text":"reply"}]), false))
        .expect(1).mount(&server).await;
    assert_eq!(
        sync_threads(&client, &store, &mut limiter, "C1", &SyncOptions::default())
            .await
            .unwrap()
            .threads,
        1
    );
    assert!(store
        .with_read(|c| messages::stale_threads(c, "C1"))
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn auth_errors_propagate_without_suppressing_the_thread() {
    for code in [
        "invalid_auth",
        "token_revoked",
        "not_authed",
        "account_inactive",
    ] {
        let (_dir, server, client, store, mut limiter) = setup().await;
        seed(
            &store,
            &[MessageRow {
                reply_count: 1,
                ..row("100.000000", None)
            }],
        );
        Mock::given(path("/conversations.replies"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":code})),
            )
            .mount(&server)
            .await;
        assert!(
            sync_threads(&client, &store, &mut limiter, "C1", &SyncOptions::default())
                .await
                .unwrap_err()
                .is_auth_error()
        );
        assert_eq!(
            store
                .with_read(|c| messages::stale_threads(c, "C1"))
                .unwrap(),
            ["100.000000"]
        );
    }
}

#[tokio::test]
async fn oversized_refresh_and_full_resync_cannot_delete_old_messages() {
    for full in [false, true] {
        let (_dir, server, client, store, mut limiter) = setup().await;
        let old = ts_days_ago(100);
        let recent = ts_days_ago(2);
        seed(&store, &[row(&old, None), row(&recent, None)]);
        store
            .with_conn(|c| {
                messages::set_sync_state(
                    c,
                    "C1",
                    &SyncState {
                        oldest_ts: Some(old.clone()),
                        newest_ts: Some(recent.clone()),
                        complete: true,
                    },
                )
            })
            .unwrap();
        Mock::given(path("/conversations.history"))
            .respond_with(page(json!([]), false))
            .mount(&server)
            .await;
        let stats = backfill_conversation(
            &client,
            &store,
            &mut limiter,
            "C1",
            &SyncOptions {
                refresh_since: Some(ts_days_ago(120)),
                full,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(visible(&store, &old));
        assert!(!visible(&store, &recent));
        assert_eq!(stats.deleted, 1);
    }
}

#[tokio::test]
async fn incomplete_history_keeps_messages_and_catch_up_cursor() {
    let (_dir, server, client, store, mut limiter) = setup().await;
    let missing = ts_days_ago(2);
    let top = ts_days_ago(1);
    seed(&store, &[row(&missing, None)]);
    let before = SyncState {
        oldest_ts: Some(missing.clone()),
        newest_ts: Some(missing.clone()),
        complete: true,
    };
    store
        .with_conn(|c| messages::set_sync_state(c, "C1", &before))
        .unwrap();
    Mock::given(path("/conversations.history"))
        .respond_with(page(json!([{"ts":top,"text":"new page"}]), true))
        .expect(1)
        .mount(&server)
        .await;
    let error = backfill_conversation(
        &client,
        &store,
        &mut limiter,
        "C1",
        &SyncOptions::refresh_days(7),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        flek_core::Error::IncompleteSync("conversations.history")
    ));
    assert!(visible(&store, &missing));
    assert!(visible(&store, &top));
    assert_eq!(
        store.with_read(|c| messages::sync_state(c, "C1")).unwrap(),
        before
    );
}

#[tokio::test]
async fn incomplete_thread_keeps_messages_and_does_not_record_completion() {
    let (_dir, server, client, store, mut limiter) = setup().await;
    let missing = ts_days_ago(2);
    seed(
        &store,
        &[
            MessageRow {
                reply_count: 2,
                latest_reply: Some(missing.clone()),
                ..row("100.000000", None)
            },
            row(&missing, Some("100.000000")),
        ],
    );
    Mock::given(path("/conversations.replies"))
        .respond_with(page(
            json!([{"ts":"100.000000","text":"root","reply_count":2,"latest_reply":missing}]),
            true,
        ))
        .expect(1)
        .mount(&server)
        .await;
    let error = sync_thread(&client, &store, &mut limiter, "C1", "100.000000")
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        flek_core::Error::IncompleteSync("conversations.replies")
    ));
    assert!(visible(&store, &missing));
    assert_eq!(
        store
            .with_read(|c| messages::stale_threads(c, "C1"))
            .unwrap(),
        ["100.000000"]
    );
}

#[tokio::test]
async fn empty_page_ends_deep_backfill_whatever_has_more_says() {
    let (_dir, server, client, store, mut limiter) = setup().await;
    Mock::given(path("/conversations.history"))
        .respond_with(page(json!([]), true))
        .expect(1)
        .mount(&server)
        .await;
    backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    assert!(
        store
            .with_read(|c| messages::sync_state(c, "C1"))
            .unwrap()
            .complete
    );
}

#[tokio::test]
async fn incomplete_pin_payload_only_fills_gaps() {
    let (_dir, server, client, store, _) = setup().await;
    seed(
        &store,
        &[MessageRow {
            reply_count: 3,
            latest_reply: Some("103.000000".into()),
            reactions: vec![("eyes".into(), "U1".into())],
            files: vec![FileRow::from_slack(&json!({"id":"F1","name":"archive.pdf"})).unwrap()],
            ..row("100.000000", Some("100.000000"))
        }],
    );
    Mock::given(path("/conversations.members"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"members":["U1"]})))
        .mount(&server)
        .await;
    Mock::given(path("/pins.list"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":true,"items":[
                {"type":"message","message":{"ts":"100.000000","text":"partial pin"}},
                {"type":"message","message":{"ts":"200.000000","text":"new pin"}}
            ]})),
        )
        .mount(&server)
        .await;
    channel_details::fresh(&client, &store, "C1").await.unwrap();
    store.with_read(|c| {
        let metadata: (String, String, i64, String, String) = c.query_row(
            "SELECT text, thread_ts, reply_count, latest_reply, raw_json FROM messages WHERE ts = '100.000000'",
            [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
        )?;
        assert_eq!(metadata, ("archived content".into(),"100.000000".into(),3,"103.000000".into(),"{}".into()));
        assert_eq!(messages::reactions(c,"C1","100.000000")?, [("eyes".into(),1)]);
        assert_eq!(c.query_row("SELECT COUNT(*) FROM message_files WHERE ts = '100.000000'", [], |r| r.get::<_,i64>(0))?, 1);
        assert_eq!(c.query_row("SELECT COUNT(*) FROM pins", [], |r| r.get::<_,i64>(0))?, 2);
        Ok(())
    }).unwrap();
    assert!(visible(&store, "200.000000"));
}

#[tokio::test]
async fn file_tombstone_preserves_metadata_raw_json_and_cache_paths() {
    let (_dir, server, client, store, mut limiter) = setup().await;
    let file = json!({"id":"F1","name":"archive.pdf","title":"Archive","mimetype":"application/pdf",
        "filetype":"pdf","size":42,"url_private":"https://files.slack.com/F1",
        "url_private_download":"https://files.slack.com/F1/download","permalink":"https://fake.slack.com/files/F1"});
    seed(
        &store,
        &[MessageRow {
            files: vec![FileRow::from_slack(&file).unwrap()],
            ..row("100.000000", None)
        }],
    );
    store.with_conn(|c| {
        c.execute("UPDATE files SET local_path = 'cached.pdf', preview_path = 'preview.png' WHERE id = 'F1'", [])?;
        Ok(())
    }).unwrap();
    let snapshot = || {
        store.with_read(|c| Ok(c.query_row(
        "SELECT json_array(name,title,mimetype,filetype,size,url_private,url_private_download,permalink,is_external,hidden_by_limit,raw_json,local_path,preview_path) FROM files WHERE id = 'F1'",
        [], |r| r.get::<_,String>(0),
    )?)).unwrap()
    };
    let before = snapshot();
    Mock::given(path("/conversations.history"))
        .respond_with(page(json!([{"ts":"100.000000","text":"kept text","files":[{"id":"F1","mode":"tombstone"}]}]), false))
        .mount(&server).await;
    backfill_conversation(&client, &store, &mut limiter, "C1", &SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(snapshot(), before);
}

#[test]
fn thread_retry_migration_preserves_existing_progress() {
    use flek_core::store::migrations;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    let mut conn = rusqlite::Connection::open(&path).unwrap();
    migrations::migrate_to(&mut conn, 18).unwrap();
    conn.execute_batch(
        "INSERT INTO teams (id, name) VALUES ('T1', 'Fake');
        INSERT INTO conversations (id, team_id, kind) VALUES ('C1', 'T1', 'public_channel');
        INSERT INTO thread_sync VALUES ('C1', '100.000000', '101.000000', 123);",
    )
    .unwrap();
    drop(conn);
    let store = Store::open(&path).unwrap();
    store.with_read(|c| {
        let progress: (String, i64, Option<i64>) = c.query_row(
            "SELECT latest_reply, synced_at, retry_after FROM thread_sync WHERE conversation_id = 'C1' AND thread_ts = '100.000000'",
            [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!(progress, ("101.000000".into(), 123, None));
        Ok(())
    }).unwrap();
}
