//! Helpers shared by the QA tests (`qa_*.rs`). Fake ids and tokens only.
#![allow(dead_code)]

use std::time::Duration;

use flek_core::api::Client;
use flek_core::rtm::Event;
use flek_core::store::messages::{self, MessageRow, SyncState};
use flek_core::store::Store;
use flek_core::sync::backfill::{backfill_conversation, BackfillStats, RateLimiter};
use flek_core::sync::live::{apply, Applied};
use flek_core::sync::SyncOptions;
use flek_core::Secret;
use serde_json::{json, Value};
use wiremock::{MockServer, ResponseTemplate};

/// Long fake secrets so a leaked tail is easy to spot (`SECRETTAIL`).
pub const TOKEN: &str = "xoxc-0000000000-1111111111-TOKENSECRETTAIL";
pub const COOKIE: &str = "xoxd-COOKIESECRETTAIL%2Bfake%3D%3D";
pub const TOKEN_TAIL: &str = "TOKENSECRETTAIL";
pub const COOKIE_TAIL: &str = "COOKIESECRETTAIL";

/// Team T1, user U1 (alice), channel C1 (#general), DM D1. No sync state.
pub fn store() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name, self_user_id) VALUES ('T1', 'Team', 'U0ME');
                 INSERT INTO users (id, team_id, name, display_name, real_name) VALUES ('U1', 'T1', 'alice', 'Alice', 'Alice Doe');
                 INSERT INTO users (id, team_id, name, display_name, real_name) VALUES ('U2', 'T1', 'bob', 'Bob', 'Bob Roe');
                 INSERT INTO conversations (id, team_id, kind, name, label) VALUES ('C1', 'T1', 'public_channel', 'general', 'general');",
            )?;
            Ok(())
        })
        .unwrap();
    store
}

/// Like [`store`], plus a finished sync state so live events are accepted.
pub fn live_store() -> Store {
    let store = store();
    store
        .with_conn(|c| {
            messages::set_sync_state(
                c,
                "C1",
                &SyncState {
                    oldest_ts: Some("1.000000".into()),
                    newest_ts: Some("10.000000".into()),
                    complete: true,
                },
            )
        })
        .unwrap();
    store
}

pub fn client(server: &MockServer) -> Client {
    Client::with_base_url(Secret::new(TOKEN), Secret::new(COOKIE), &server.uri())
        .unwrap()
        .without_pacing()
}

pub fn row(ts: &str, text: &str) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        user_id: Some("U1".into()),
        text: text.into(),
        raw_json: json!({"ts": ts, "user": "U1", "text": text}).to_string(),
        ..Default::default()
    }
}

pub fn upsert(store: &Store, rows: &[MessageRow]) {
    store
        .with_conn(|c| messages::upsert_messages(c, "C1", rows))
        .unwrap();
}

pub async fn ev(store: &Store, v: Value) -> Applied {
    apply(store, Event::parse(v)).await.unwrap()
}

/// Raw stored state of a message.
#[derive(Debug, PartialEq, Eq)]
pub struct Stored {
    pub text: String,
    pub hidden: bool,
    pub deleted: bool,
    pub edited_ts: Option<String>,
    pub reply_count: i64,
    pub raw_json: String,
}

pub fn stored(store: &Store, ts: &str) -> Option<Stored> {
    store
        .with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT COALESCE(text, ''), hidden_by_limit, deleted, edited_ts, reply_count, COALESCE(raw_json, '')
                 FROM messages WHERE conversation_id = 'C1' AND ts = ?1",
            )?;
            let mut rows = stmt.query([ts])?;
            Ok(match rows.next()? {
                Some(r) => Some(Stored {
                    text: r.get(0)?,
                    hidden: r.get(1)?,
                    deleted: r.get(2)?,
                    edited_ts: r.get(3)?,
                    reply_count: r.get(4)?,
                    raw_json: r.get(5)?,
                }),
                None => None,
            })
        })
        .unwrap()
}

pub fn count(store: &Store, sql: &str) -> i64 {
    store
        .with_conn(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

pub fn ok(mut body: Value) -> ResponseTemplate {
    body["ok"] = json!(true);
    ResponseTemplate::new(200).set_body_json(body)
}

pub fn history(messages: Value) -> ResponseTemplate {
    ok(json!({"messages": messages, "has_more": false}))
}

pub async fn backfill(client: &Client, store: &Store, opts: &SyncOptions) -> BackfillStats {
    backfill_conversation(
        client,
        store,
        &mut RateLimiter::new(Duration::ZERO),
        "C1",
        opts,
    )
    .await
    .unwrap()
}

pub fn full() -> SyncOptions {
    SyncOptions {
        full: true,
        ..SyncOptions::default()
    }
}
