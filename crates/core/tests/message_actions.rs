//! Message menu actions (M4.7 / M8.4.6): permalink, saved list, pins, forward and
//! mark unread against a temp SQLite and a mock Slack. Never touches real Slack.
mod support;
use flek_core::{
    agent::{ActionLog, AgentConfig},
    api::{AuthTest, Client},
    store::{
        directory, message_actions as cache,
        messages::{self, MessageRow},
        Store,
    },
    sync::{live, message_actions as actions, read},
    writes::{PreparedWrite, WriteSession},
    Error, Secret,
};
use serde_json::json;
use support::field;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

const ROOT: &str = "1700000000.000100";
const REPLY: &str = "1700000000.000200";
const LATER: &str = "1700000000.000300";

fn client(server: &MockServer) -> Client {
    Client::with_base_url(
        Secret::new("xoxc-fake-token"),
        Secret::new("xoxd-fake-cookie"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing()
}

fn row(ts: &str, thread: Option<&str>) -> MessageRow {
    MessageRow {
        ts: ts.into(),
        thread_ts: thread.map(Into::into),
        text: format!("text {ts}"),
        raw_json: "{}".into(),
        ..Default::default()
    }
}

fn store_with_url(url: &str) -> Store {
    let s = Store::open_in_memory().unwrap();
    s.with_conn(|c| {
        directory::upsert_team(c, "T1", "Fake", url, "U1")?;
        c.execute_batch(
            "INSERT INTO users (id, team_id, name, real_name, display_name, deleted) VALUES
               ('U1','T1','me','Me','me',0),
               ('U2','T1','alice','Alice','alice',0),
               ('U3','T1','gone','Gone','gone',1);
             INSERT INTO conversations (id, team_id, kind, name, label, last_read) VALUES
               ('C1','T1','public_channel','general','general','1700000000.000300'),
               ('C2','T1','public_channel','random','random',NULL);
             INSERT INTO conversations (id, team_id, kind, label, dm_user_id)
               VALUES ('D1','T1','im','alice','U2');
             INSERT INTO teams (id, name) VALUES ('T2','Other');
             INSERT INTO conversations (id, team_id, kind, name, label)
               VALUES ('CX','T2','public_channel','x','x');",
        )?;
        messages::upsert_messages(
            c,
            "C1",
            &[
                row(ROOT, None),
                row("1700000000.000150", Some(ROOT)),
                row(REPLY, Some(ROOT)),
                row(LATER, None),
            ],
        )?;
        Ok(())
    })
    .unwrap();
    s
}

fn store() -> Store {
    store_with_url("https://fake.slack.com/")
}

fn me() -> AuthTest {
    AuthTest {
        url: "https://fake.slack.com/".into(),
        team: "Fake".into(),
        user: "me".into(),
        team_id: "T1".into(),
        user_id: "U1".into(),
        enterprise_id: None,
    }
}

fn ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"ok":true}))
}

fn failure(code: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":code}))
}

fn last_read(s: &Store, id: &str) -> Option<String> {
    s.with_read(|c| {
        Ok(c.query_row(
            "SELECT last_read FROM conversations WHERE id=?1",
            [id],
            |r| r.get(0),
        )?)
    })
    .unwrap()
}

fn count(s: &Store, sql: &'static str) -> i64 {
    s.with_read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

// ------------------------------------------------------------- permalink

#[test]
fn permalink_is_built_offline_from_the_workspace_url() {
    let s = store();
    let link = |ts: &str| s.with_read(|c| cache::permalink(c, "C1", ts)).unwrap();
    assert_eq!(
        link(ROOT).as_deref(),
        Some("https://fake.slack.com/archives/C1/p1700000000000100")
    );
    // A reply carries its thread root so Slack opens the thread.
    assert_eq!(
        link(REPLY).as_deref(),
        Some("https://fake.slack.com/archives/C1/p1700000000000200?thread_ts=1700000000.000100&cid=C1")
    );
    // The API-result cache wins over the built URL.
    s.with_conn(|c| {
        c.execute(
            "INSERT INTO message_permalinks VALUES ('C1', ?1, 'https://fake.slack.com/archives/C1/pcached')",
            [ROOT],
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        link(ROOT).as_deref(),
        Some("https://fake.slack.com/archives/C1/pcached")
    );
    let err = s
        .with_read(|c| cache::permalink(c, "C1", "not-a-ts"))
        .unwrap_err();
    assert!(matches!(err, Error::Usage(_)));
}

#[test]
fn permalink_refuses_foreign_or_unsafe_hosts() {
    for url in [
        "https://evil.example.com/",
        "https://user:pw@fake.slack.com/",
        "http://fake.slack.com/",
        "https://fake.slack.com:8443/",
    ] {
        let s = Store::open_in_memory().unwrap();
        s.with_conn(|c| {
            c.execute_batch("INSERT INTO teams (id,name,url) VALUES ('T1','Fake','');
                INSERT INTO conversations (id,team_id,kind,name,label) VALUES ('C1','T1','public_channel','g','g');")?;
            c.execute("UPDATE teams SET url=?1", [url])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            s.with_read(|c| cache::permalink(c, "C1", ROOT)).unwrap(),
            None,
            "{url}"
        );
    }
}

#[tokio::test]
async fn permalink_falls_back_to_slack_then_to_a_deep_link() {
    let server = MockServer::start().await;
    // No stored workspace URL: only the API can produce an HTTPS link.
    let s = Store::open_in_memory().unwrap();
    s.with_conn(|c| {
        c.execute_batch(
            "INSERT INTO teams (id,name) VALUES ('T1','Fake');
             INSERT INTO conversations (id,team_id,kind,name,label) VALUES ('C1','T1','public_channel','g','g');",
        )?;
        Ok(())
    })
    .unwrap();
    let good = "https://fake.slack.com/archives/C1/p1700000000000100";
    Mock::given(method("POST"))
        .and(path("/chat.getPermalink"))
        .and(field("channel", "C1"))
        .and(field("message_ts", ROOT))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"permalink":good})))
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server);
    assert_eq!(
        actions::permalink(Some(&c), &s, "C1", ROOT).await.unwrap(),
        good
    );
    // Cached now: no second request (the mock expects exactly one).
    assert_eq!(
        actions::permalink(Some(&c), &s, "C1", ROOT).await.unwrap(),
        good
    );
    assert_eq!(count(&s, "SELECT COUNT(*) FROM message_permalinks"), 1);

    // A link for a different message is rejected and never cached.
    Mock::given(method("POST"))
        .and(path("/chat.getPermalink"))
        .and(field("message_ts", REPLY))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"ok":true,"permalink":"https://fake.slack.com/archives/C9/p1700000000000200"}),
        ))
        .mount(&server)
        .await;
    assert!(actions::permalink(Some(&c), &s, "C1", REPLY).await.is_err());
    assert_eq!(count(&s, "SELECT COUNT(*) FROM message_permalinks"), 1);

    // Slack unavailable and no client: the deep link is the last resort.
    let offline = actions::permalink(None, &s, "C1", LATER).await.unwrap();
    assert_eq!(
        offline,
        "slack://channel?team=T1&id=C1&message=1700000000.000300"
    );
}

// ----------------------------------------------------------- saved / pins

#[test]
fn saved_list_is_per_user_and_independent_of_history_merges() {
    let s = store();
    s.with_conn(|c| {
        cache::save(c, "C1", ROOT, "U1", true, true)?;
        cache::save(c, "C1", LATER, "U1", true, true)?;
        cache::save(c, "C1", ROOT, "U2", true, true)?;
        Ok(())
    })
    .unwrap();
    let state = |user: &str, ts: &str| s.with_read(|c| cache::state(c, "C1", ts, user)).unwrap();
    assert!(state("U1", ROOT).saved && state("U1", ROOT).saved_local_only);
    assert!(!state("U1", REPLY).saved);
    s.with_conn(|c| cache::save(c, "C1", ROOT, "U1", false, true))
        .unwrap();
    assert!(!state("U1", ROOT).saved);
    assert!(
        state("U2", ROOT).saved,
        "another user's bookmark is untouched"
    );

    let saved = |user: &str| {
        s.with_read(|c| cache::saved(c, "T1", user, 50, 0))
            .unwrap()
            .into_iter()
            .map(|m| m.ts)
            .collect::<Vec<_>>()
    };
    assert_eq!(saved("U1"), [LATER]);
    assert_eq!(saved("U2"), [ROOT]);
    // Re-importing the message (a history merge) keeps the bookmark.
    s.with_conn(|c| {
        messages::upsert_messages(c, "C1", &[row(LATER, None)])?;
        Ok(())
    })
    .unwrap();
    assert_eq!(saved("U1"), [LATER]);
    // Deleted messages drop out of the list but not out of the table.
    s.with_conn(|c| {
        c.execute("UPDATE messages SET deleted=1 WHERE ts=?1", [LATER])?;
        Ok(())
    })
    .unwrap();
    assert!(saved("U1").is_empty());
}

#[test]
fn confirmed_cache_updates_and_requires_a_stored_message() {
    let s = store();
    s.with_conn(|c| {
        cache::confirmed(c, "pin", "C1", ROOT, "U1")?;
        cache::confirmed(c, "pin", "C1", ROOT, "U2")?; // idempotent
        Ok(())
    })
    .unwrap();
    assert_eq!(count(&s, "SELECT COUNT(*) FROM pins"), 1);
    assert!(
        s.with_read(|c| cache::state(c, "C1", ROOT, "U1"))
            .unwrap()
            .pinned
    );
    s.with_conn(|c| cache::confirmed(c, "unpin", "C1", ROOT, "U1"))
        .unwrap();
    assert_eq!(count(&s, "SELECT COUNT(*) FROM pins"), 0);
    // A CLI star also shows up in the local list the app renders.
    s.with_conn(|c| cache::confirmed(c, "save", "C1", ROOT, "U1"))
        .unwrap();
    assert!(
        s.with_read(|c| cache::state(c, "C1", ROOT, "U1"))
            .unwrap()
            .saved
    );
    assert!(s
        .with_conn(|c| cache::confirmed(c, "save", "C1", "1.000001", "U1"))
        .is_err());
    assert!(matches!(
        s.with_conn(|c| cache::confirmed(c, "delete", "C1", ROOT, "U1")),
        Err(Error::Forbidden)
    ));
    assert!(s
        .with_read(|c| cache::require_message(c, "C1", "1.000001"))
        .is_err());
    assert!(matches!(
        s.with_read(|c| cache::require_message(c, "C1", "bad")),
        Err(Error::Usage(_))
    ));
}

#[tokio::test]
async fn pin_calls_documented_methods_and_updates_the_cache() {
    let server = MockServer::start().await;
    for endpoint in ["pins.add", "pins.remove"] {
        Mock::given(method("POST"))
            .and(path(format!("/{endpoint}")))
            .and(field("channel", "C1"))
            .and(field("timestamp", ROOT))
            .respond_with(ok())
            .expect(1)
            .mount(&server)
            .await;
    }
    let (s, bus, c) = (store(), live::bus(), client(&server));
    let mut rx = bus.subscribe();
    actions::pin(&c, &s, &bus, "C1", ROOT, "U1", true)
        .await
        .unwrap();
    assert!(
        s.with_read(|c| cache::state(c, "C1", ROOT, "U1"))
            .unwrap()
            .pinned
    );
    assert_eq!(rx.try_recv().unwrap(), live::Change::Directory);
    actions::pin(&c, &s, &bus, "C1", ROOT, "U1", false)
        .await
        .unwrap();
    assert!(
        !s.with_read(|c| cache::state(c, "C1", ROOT, "U1"))
            .unwrap()
            .pinned
    );
    // Unknown messages are refused before any request (mocks expect one call each).
    assert!(actions::pin(&c, &s, &bus, "C1", "1.000001", "U1", true)
        .await
        .is_err());
}

#[tokio::test]
async fn pin_treats_already_pinned_as_success_and_keeps_cache_on_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/pins.add"))
        .and(field("timestamp", ROOT))
        .respond_with(failure("already_pinned"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/pins.add"))
        .and(field("timestamp", LATER))
        .respond_with(failure("permission_denied"))
        .mount(&server)
        .await;
    let (s, bus, c) = (store(), live::bus(), client(&server));
    actions::pin(&c, &s, &bus, "C1", ROOT, "U1", true)
        .await
        .unwrap();
    assert!(
        s.with_read(|c| cache::state(c, "C1", ROOT, "U1"))
            .unwrap()
            .pinned
    );
    let err = actions::pin(&c, &s, &bus, "C1", LATER, "U1", true)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Slack { .. }));
    assert!(
        !s.with_read(|c| cache::state(c, "C1", LATER, "U1"))
            .unwrap()
            .pinned
    );
}

#[tokio::test]
async fn save_falls_back_to_a_local_bookmark_unless_the_failure_is_real() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/stars.add"))
        .and(field("timestamp", ROOT))
        .respond_with(failure("missing_scope"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/stars.add"))
        .and(field("timestamp", LATER))
        .respond_with(failure("ratelimited_for_real"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/stars.remove"))
        .respond_with(failure("not_starred"))
        .mount(&server)
        .await;
    let (s, bus, c) = (store(), live::bus(), client(&server));
    let state = |ts: &str| s.with_read(|c| cache::state(c, "C1", ts, "U1")).unwrap();
    // Unsupported method: still bookmarked locally, flagged local-only.
    actions::save(Some(&c), &s, &bus, "C1", ROOT, "U1", true)
        .await
        .unwrap();
    assert!(state(ROOT).saved && state(ROOT).saved_local_only);
    // No client at all (offline): same.
    actions::save(None, &s, &bus, "C1", REPLY, "U1", true)
        .await
        .unwrap();
    assert!(state(REPLY).saved);
    // Unstar of a never-starred item is idempotent.
    actions::save(Some(&c), &s, &bus, "C1", ROOT, "U1", false)
        .await
        .unwrap();
    assert!(!state(ROOT).saved);
    // Other Slack errors surface and leave the bookmark alone.
    assert!(actions::save(Some(&c), &s, &bus, "C1", LATER, "U1", true)
        .await
        .is_err());
    assert!(!state(LATER).saved);
}

// ----------------------------------------------------------------- forward

fn posted(channel: &str, ts: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "ok": true, "channel": channel, "ts": ts,
        "message": {"type":"message","user":"U1","text":"x","ts":ts}
    }))
}

#[tokio::test]
async fn forward_posts_comment_and_permalink_to_a_channel() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .and(field("channel", "C2"))
        .and(field(
            "text",
            "look at this\nhttps://fake.slack.com/archives/C1/p1700000000000100",
        ))
        .respond_with(posted("C2", "1700000009.000100"))
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    let sent = actions::forward(
        &client(&server),
        &s,
        &me(),
        "C1",
        ROOT,
        "C2",
        "  look at this ",
    )
    .await
    .unwrap();
    assert_eq!(sent.conversation_id, "C2");
    // The message is stored locally in the destination, the source untouched.
    assert_eq!(
        count(
            &s,
            "SELECT COUNT(*) FROM messages WHERE conversation_id='C2'"
        ),
        1
    );
}

#[tokio::test]
async fn forward_without_comment_posts_only_the_link_and_reuses_an_existing_dm() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .and(field("channel", "D1"))
        .and(field(
            "text",
            "https://fake.slack.com/archives/C1/p1700000000000200?thread_ts=1700000000.000100&amp;cid=C1",
        ))
        .respond_with(posted("D1", "1700000009.000200"))
        .expect(1)
        .mount(&server)
        .await;
    // No conversations.open request may happen: an im with that person is cached.
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .respond_with(ok())
        .expect(0)
        .mount(&server)
        .await;
    let s = store();
    let sent = actions::forward(&client(&server), &s, &me(), "C1", REPLY, "U2", "")
        .await
        .unwrap();
    assert_eq!(sent.conversation_id, "D1");
}

#[tokio::test]
async fn forward_opens_a_dm_for_a_person_without_one() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .and(field("users", "U1x"))
        .respond_with(ok())
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/conversations.open"))
        .and(field("users", "U4"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"channel":{"id":"D4","is_im":true,"user":"U4"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .and(field("channel", "D4"))
        .respond_with(posted("D4", "1700000009.000300"))
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    s.with_conn(|c| {
        c.execute(
            "INSERT INTO users (id,team_id,name,real_name,display_name,deleted) VALUES ('U4','T1','dan','Dan','dan',0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let sent = actions::forward(&client(&server), &s, &me(), "C1", ROOT, "U4", "hi")
        .await
        .unwrap();
    assert_eq!(sent.conversation_id, "D4");
    assert_eq!(
        count(
            &s,
            "SELECT COUNT(*) FROM conversations WHERE id='D4' AND dm_user_id='U4'"
        ),
        1
    );
}

#[tokio::test]
async fn forward_rejects_unavailable_destinations_and_foreign_sources() {
    let server = MockServer::start().await; // any request would 404 and fail loudly
    let s = store();
    s.with_conn(|c| {
        c.execute("UPDATE conversations SET is_archived=1 WHERE id='C2'", [])?;
        Ok(())
    })
    .unwrap();
    let c = client(&server);
    let go = |src: &str, dest: &str| {
        let (c, s) = (&c, &s);
        let (src, dest) = (src.to_owned(), dest.to_owned());
        async move { actions::forward(c, s, &me(), &src, ROOT, &dest, "").await }
    };
    // Archived channel, deleted user, a bot-less unknown id, a channel of another team.
    for dest in ["C2", "U3", "UNOPE", "CX"] {
        assert!(
            matches!(go("C1", dest).await, Err(Error::NotFound(_))),
            "{dest}"
        );
    }
    // Source in another team's conversation.
    assert!(matches!(
        go("CX", "C1").await,
        Err(Error::NotFound(_) | Error::Forbidden)
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}

// ------------------------------------------------------------ mark unread

#[test]
fn previous_timestamp_is_exact_integer_arithmetic() {
    assert_eq!(
        read::previous_timestamp("1700000000.000100").unwrap(),
        "1700000000.000099"
    );
    assert_eq!(
        read::previous_timestamp("1700000000.000000").unwrap(),
        "1699999999.999999"
    );
    assert_eq!(
        read::previous_timestamp("1700000000.000001").unwrap(),
        "1700000000.000000"
    );
    for bad in ["", "abc", "1700000000", "1.2.3", "-1.000000"] {
        assert!(read::previous_timestamp(bad).is_err(), "{bad:?}");
    }
}

#[tokio::test]
async fn unread_rewinds_the_cursor_to_the_previous_timeline_message() {
    let server = MockServer::start().await;
    // Target LATER: the thread replies at .000150/.000200 are not timeline rows, so the
    // cursor lands on ROOT, not on a reply.
    Mock::given(method("POST"))
        .and(path("/conversations.mark"))
        .and(field("channel", "C1"))
        .and(field("ts", ROOT))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;
    let (s, bus, c) = (store(), live::bus(), client(&server));
    let mut rx = bus.subscribe();
    let cursor = read::unread(&c, &s, &bus, "C1", LATER).await.unwrap();
    assert_eq!(cursor, ROOT);
    assert_eq!(
        last_read(&s, "C1").as_deref(),
        Some(ROOT),
        "cursor moved backwards"
    );
    assert!(rx.try_recv().is_ok(), "a read-marker change is announced");
    // Normal automatic marking stays forward-only: an older mark is a no-op (no request).
    read::mark(&c, &s, &bus, "C1", "1700000000.000050")
        .await
        .unwrap();
    assert_eq!(last_read(&s, "C1").as_deref(), Some(ROOT));
}

#[tokio::test]
async fn unread_on_the_first_message_uses_the_preceding_microsecond() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.mark"))
        .and(field("ts", "1700000000.000099"))
        .respond_with(ok())
        .expect(1)
        .mount(&server)
        .await;
    let (s, bus, c) = (store(), live::bus(), client(&server));
    let cursor = read::unread(&c, &s, &bus, "C1", ROOT).await.unwrap();
    assert_eq!(cursor, "1700000000.000099");
    assert_eq!(last_read(&s, "C1").as_deref(), Some("1700000000.000099"));
}

#[tokio::test]
async fn unread_keeps_the_local_cursor_when_slack_refuses() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.mark"))
        .respond_with(failure("channel_not_found"))
        .mount(&server)
        .await;
    let (s, bus, c) = (store(), live::bus(), client(&server));
    assert!(read::unread(&c, &s, &bus, "C1", LATER).await.is_err());
    assert_eq!(last_read(&s, "C1").as_deref(), Some(LATER));
    // Unknown message: refused before any request.
    assert!(read::unread(&c, &s, &bus, "C1", "1.000001").await.is_err());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

// ------------------------------------------------------ guarded `forward`

fn prepare_forward(
    s: &Store,
    reference: &str,
    ts: Option<&str>,
    to: &str,
    comment: &str,
    cfg: &AgentConfig,
) -> flek_core::Result<PreparedWrite> {
    s.with_read(|c| PreparedWrite::forward(c, "T1", reference, ts, to, comment, cfg))
}

#[test]
fn guarded_forward_prepares_offline_and_targets_the_destination() {
    let s = store();
    let cfg = AgentConfig::default();
    let p = prepare_forward(
        &s,
        "general",
        Some(ROOT),
        "random",
        "FYI @alice <!channel>",
        &cfg,
    )
    .unwrap();
    assert_eq!(p.action.action, "forward");
    assert_eq!(p.action.targets, ["C2"]);
    assert_eq!(p.request.method, "chat.postMessage");
    assert_eq!(p.request.params["channel"], "C2");
    let text = &p.request.params["text"];
    // Explicit @name resolves, raw markup is escaped, the permalink follows.
    assert_eq!(
        text,
        "FYI <@U2> &lt;!channel&gt;\nhttps://fake.slack.com/archives/C1/p1700000000000100"
    );
    assert!(!p.request.params.contains_key("token"));
    // A permalink reference works like (channel, ts).
    let by_link = prepare_forward(
        &s,
        "https://fake.slack.com/archives/C1/p1700000000000100",
        None,
        "C2",
        "",
        &cfg,
    )
    .unwrap();
    assert_eq!(
        by_link.request.params["text"],
        "https://fake.slack.com/archives/C1/p1700000000000100"
    );
    // The configured signature is appended like it is for `send`.
    let signed = AgentConfig {
        signature: Some("via agent".into()),
        ..Default::default()
    };
    let p = prepare_forward(&s, "general", Some(ROOT), "random", "note", &signed).unwrap();
    assert!(p.request.params["text"].starts_with("note\n\nvia agent\nhttps://"));
}

#[test]
fn guarded_forward_validates_before_any_network_work() {
    let s = store();
    let cfg = AgentConfig::default();
    let kind = |r: flek_core::Result<PreparedWrite>| r.err().expect("must fail").kind().exit_code();
    // Unknown destination / source / message, ambiguous prefix, bad timestamp.
    assert_eq!(
        kind(prepare_forward(&s, "general", Some(ROOT), "nope", "", &cfg)),
        3
    );
    assert_eq!(
        kind(prepare_forward(&s, "nope", Some(ROOT), "random", "", &cfg)),
        3
    );
    assert_eq!(
        kind(prepare_forward(
            &s,
            "general",
            Some("1.000001"),
            "random",
            "",
            &cfg
        )),
        3
    );
    assert_eq!(
        kind(prepare_forward(
            &s,
            "general",
            Some("bad"),
            "random",
            "",
            &cfg
        )),
        2
    );
    // Without a workspace URL no permalink is known offline: usage error, never a guess.
    let bare = Store::open_in_memory().unwrap();
    bare.with_conn(|c| {
        c.execute_batch(
            "INSERT INTO teams (id,name) VALUES ('T1','Fake');
             INSERT INTO conversations (id,team_id,kind,name,label) VALUES
               ('C1','T1','public_channel','general','general'),
               ('C2','T1','public_channel','random','random');",
        )?;
        messages::upsert_messages(c, "C1", &[row(ROOT, None)])?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        kind(prepare_forward(
            &bare,
            "general",
            Some(ROOT),
            "random",
            "",
            &cfg
        )),
        2
    );
}

#[tokio::test]
async fn guarded_forward_is_denied_by_default_and_runs_with_yes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .and(field("channel", "C2"))
        .and(field(
            "text",
            "https://fake.slack.com/archives/C1/p1700000000000100",
        ))
        .respond_with(posted("C2", "1700000009.000400"))
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    let cfg = AgentConfig::default();
    let p = prepare_forward(&s, "general", Some(ROOT), "random", "", &cfg).unwrap();
    // The dry run reports policy without doing anything.
    let report = p.dry_run_report(&cfg, false);
    assert_eq!(report["authorized"], false);
    assert_eq!(p.dry_run_report(&cfg, true)["authorized"], true);

    let dir = tempfile::tempdir().unwrap();
    let log = ActionLog::new(dir.path().join("log.db"));
    let session = || async {
        Ok(WriteSession {
            client: client(&server),
            team_id: "T1".into(),
            user_id: "U1".into(),
        })
    };
    let denied = p.run_guarded(&log, &cfg, false, &s, session).await;
    assert!(matches!(denied, Err(Error::Forbidden)));
    assert!(server.received_requests().await.unwrap().is_empty());

    let receipt = p.run_guarded(&log, &cfg, true, &s, session).await.unwrap();
    assert_eq!(receipt.action, "forward");
    assert_eq!(receipt.channel.as_deref(), Some("C2"));
    assert_eq!(receipt.ts.as_deref(), Some("1700000009.000400"));
    assert!(!receipt.not_stored_locally);
    let entries = log.entries(10).unwrap();
    assert_eq!(entries.last().unwrap().action, "forward");
    assert_eq!(entries.last().unwrap().targets, ["C2"]);
}
