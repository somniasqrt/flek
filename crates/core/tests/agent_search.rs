//! Agent search uses only invented identities, in-memory SQLite and mock HTTP.
mod support;
use flek_core::{
    api::{self, Client},
    search::{self, Options},
    store::{
        directory,
        messages::{self, MessageRow},
        Store,
    },
    views, Error, Secret,
};
use serde_json::{json, Value};
use support::field;
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

fn ts(i: usize) -> String {
    format!("1700000000.{i:06}")
}
fn seed() -> Store {
    let db = Store::open_in_memory().unwrap();
    db.with_conn(|c| {
        directory::upsert_team(c, "TFAKE", "Fake", "https://fake.slack.com/", "USELF")?;
        let users: Vec<api::User> = serde_json::from_value(json!([
            {"id":"UFAKE", "name":"alice", "profile":{"display_name":"Alice"}},
            {"id":"UOTHER", "name":"alicia"}
        ]))?;
        directory::upsert_users(c, "TFAKE", &users)?;
        let convs: Vec<api::Conversation> = serde_json::from_value(json!([
            {"id":"CFAKE", "name":"general"}, {"id":"COTHER", "name":"general-help"}
        ]))?;
        directory::upsert_conversations(c, "TFAKE", "USELF", &convs)?;
        let mut rows: Vec<_> = (1..=5)
            .map(|i| MessageRow {
                ts: ts(i),
                text: "hello complete local content".into(),
                user_id: Some("UFAKE".into()),
                raw_json: "{}".into(),
                ..Default::default()
            })
            .collect();
        rows[0].blocks_json = Some("[{\"type\":\"section\"}]".into());
        rows[0].reactions.push(("wave".into(), "UFAKE".into()));
        rows[0].files.push(flek_core::store::files::FileRow {
            id: "FLOCAL".into(),
            name: Some("complete.txt".into()),
            raw_json: "{}".into(),
            ..Default::default()
        });
        rows.push(MessageRow {
            ts: ts(6),
            thread_ts: Some(ts(3)),
            text: "hello reply".into(),
            raw_json: "{}".into(),
            ..Default::default()
        });
        rows.push(MessageRow {
            ts: ts(7),
            thread_ts: Some(ts(3)),
            text: "hello reply".into(),
            raw_json: "{}".into(),
            ..Default::default()
        });
        messages::upsert_messages(c, "CFAKE", &rows)?;
        Ok(())
    })
    .unwrap();
    db
}
fn client(server: &MockServer) -> Client {
    Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing()
}
async fn run(
    db: &Store,
    input: &str,
    o: &Options,
    client: Option<&Client>,
) -> flek_core::Result<search::Page> {
    let p = db.with_read(|c| search::prepare(c, "TFAKE", input, o))?;
    search::run(db, "TFAKE", p, o, client, ("**", "**")).await
}
#[test]
fn parsed_query_contract_and_slack_translation() {
    let db = seed();
    let o = Options {
        limit: 10,
        ..Default::default()
    };
    let input = r#"from:@alice in:#general has:link has:file before:2024-02-01 after:2023-01-01 hello prefix* "exact phrase" -old"#;
    let p = db
        .with_read(|c| search::prepare(c, "TFAKE", input, &o))
        .unwrap();
    let q = serde_json::to_value(&p.query).unwrap();
    assert_eq!(q["from"], json!([{"id":"UFAKE","label":"Alice"}]));
    assert_eq!(
        q["in_conversations"],
        json!([{"id":"CFAKE","label":"general"}])
    );
    assert_eq!(q["before"], "2024-02-01");
    assert_eq!(q["after"], "2023-01-01");
    assert_eq!(
        q["terms"],
        json!(["hello", "prefix*", "\"exact phrase\"", "-old"])
    );
    assert!(q["has_link"].as_bool().unwrap() && q["has_file"].as_bool().unwrap());
    assert_eq!(
        p.remote_query,
        r#"hello "exact phrase" -old from:<@UFAKE> in:<#CFAKE> has:link has:file before:2024-02-01 after:2023-01-01"#
    );
    assert_eq!(
        p.remote_unsupported_filters,
        ["local_stemming:hello", "prefix*"]
    );
    for (input, kind) in [
        ("in:missing", 3),
        ("in:@general", 3),
        ("in:gen", 4),
        ("from:missing", 3),
        ("from:ali", 4),
    ] {
        assert_eq!(
            db.with_read(|c| search::prepare(c, "TFAKE", input, &o))
                .err()
                .unwrap()
                .kind()
                .exit_code(),
            kind
        );
    }
    assert!(db
        .with_read(|c| search::prepare(c, "TFAKE", "before:2023-02-29", &o))
        .is_err());
    let p = db
        .with_read(|c| {
            search::prepare(
                c,
                "TFAKE",
                "from:alice from:alicia in:general in:general-help",
                &o,
            )
        })
        .unwrap();
    assert!(p.remote_query.is_empty());
    assert_eq!(
        p.remote_unsupported_filters,
        ["from:multiple", "in:multiple"]
    );
}
#[tokio::test]
async fn times_context_and_opaque_paging_are_local() {
    let db = seed();
    let mut o = Options {
        limit: 2,
        recent: true,
        context: 1,
        ..Default::default()
    };
    let page = run(&db, "hello", &o, None).await.unwrap();
    assert!(!page.remote.searched);
    assert!(page.results.iter().all(|h| h.source == "local"));
    assert_eq!(page.results[0].result.message.ts, ts(7));
    assert_eq!(page.results[0].before[0].ts, ts(6));
    assert_eq!(page.results[1].before[0].ts, ts(3)); // reply's thread root
    assert_eq!(page.results[1].after[0].ts, ts(7));
    assert_eq!(page.results[0].result.snippet, "**hello** reply");
    o.cursor = page.next_cursor;
    let next = run(&db, "hello", &o, None).await.unwrap();
    assert_eq!(next.results[0].result.message.ts, ts(5));
    assert_eq!(next.results[0].before[0].ts, ts(4));
    assert!(next.results[0].after.is_empty()); // ordinary replies don't enter channel timeline
    assert!(matches!(
        run(&db, "different", &o, None).await,
        Err(Error::Usage(_))
    ));
    o.cursor = None;
    o.offset = 2;
    assert_eq!(
        run(&db, "hello", &o, None).await.unwrap().results[0]
            .result
            .message
            .ts,
        ts(5)
    );
    o.offset = 0;
    o.since = Some(ts(2));
    o.until = Some(ts(4));
    o.limit = 20;
    let page = run(&db, "hello", &o, None).await.unwrap();
    assert_eq!(page.query.since_ts.as_deref(), Some(ts(2).as_str()));
    assert_eq!(page.query.until_ts.as_deref(), Some(ts(4).as_str()));
    assert_eq!(page.results.len(), 2); // since inclusive, until exclusive
    o.since = Some("7d".into());
    o.until = None;
    let p = db
        .with_read(|c| search::prepare(c, "TFAKE", "hello", &o))
        .unwrap();
    assert!(p.query.since_ts.unwrap() > ts(7));
    assert!(p.remote_unsupported_filters.contains(&"--since".into()));
    o.since = Some(ts(4));
    o.until = Some(ts(2));
    assert!(matches!(
        run(&db, "hello", &o, None).await,
        Err(Error::Usage(_))
    ));
    o = Options {
        limit: 0,
        ..Default::default()
    };
    let page = run(&db, "hello", &o, None).await.unwrap();
    assert!(page.results.is_empty() && page.next_cursor.is_none());
}
#[tokio::test]
async fn remote_ingests_merges_without_degrading_and_never_grants_membership() {
    let db = seed();
    let server = MockServer::start().await;
    let client = client(&server);
    Mock::given(path("/search.messages"))
        .and(field("query","hello"))
        .and(field("count","25"))
        .and(field("sort","score"))
        .and(field("highlight","false"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"messages":{
            "total":4,"paging":{"pages":1},"matches":[
                {"channel":{"id":"CFAKE","name":"general"},"ts":ts(1),"text":"hello","user":"UFAKE"},
                {"channel":{"id":"CFAKE","name":"general"},"ts":ts(1),"text":"","user":"UFAKE"},
                {"channel":{"id":"CNEW","name":"public-discovery"},"ts":ts(20),"text":"hello remote","user":"UNEW","username":"newperson","user_profile":{"display_name":"New Person"}},
                {"channel":{"id":"CNEW","name":"public-discovery"},"ts":ts(21),"text":"hello file","user":"UIDONLY","files":[{"id":"FHIDDEN","mode":"hidden_by_limit"}]}
            ]
        }}))).expect(1).mount(&server).await;
    let o = Options {
        limit: 20,
        recent: false,
        ..Default::default()
    };
    let page = run(&db, "hello", &o, Some(&client)).await.unwrap();
    assert_eq!((page.remote.total, page.remote.pages), (4, 1));
    assert!(page.remote.searched && page.remote.error.is_none());
    assert_eq!(page.results.len(), 9);
    let both = page
        .results
        .iter()
        .find(|h| h.result.message.ts == ts(1))
        .unwrap();
    assert_eq!(both.source, "both");
    assert_eq!(both.result.message.text, "hello complete local content");
    // Search payloads omit history attachments; complete evidence survives.
    assert_eq!(both.result.message.files[0].id, "FLOCAL");
    assert_eq!(both.result.message.reactions[0].name, "wave");
    assert_eq!(
        page.results.iter().filter(|h| h.source == "remote").count(),
        2
    );
    db.with_read(|c| {
        assert!(!views::conversation(c, "CNEW")?.unwrap().is_member);
        assert!(views::conversation(c, "CFAKE")?.unwrap().is_member);
        assert!(!directory::list_conversations(c, "TFAKE")?
            .iter()
            .any(|v| v.id == "CNEW"));
        assert!(!views::conversations(c, "TFAKE", true)?
            .iter()
            .any(|v| v.id == "CNEW"));
        assert!(views::conversations_including(c, "TFAKE", true, true)?
            .iter()
            .any(|v| v.id == "CNEW"));
        assert!(!directory::conversation_activity(c, "TFAKE")?
            .iter()
            .any(|v| v.0 == "CNEW"));
        assert_eq!(
            views::user(c, "UNEW")?.unwrap().display_name.as_deref(),
            Some("New Person")
        );
        assert!(views::user(c, "UIDONLY")?.is_none());
        assert!(c.query_row::<bool, _, _>(
            "SELECT hidden_by_limit FROM files WHERE id='FHIDDEN'",
            [],
            |r| r.get(0)
        )?);
        assert!(messages::sync_state(c, "CNEW")?.newest_ts.is_none());
        Ok(())
    })
    .unwrap();
    db.with_conn(|c| {
        messages::set_sync_state(
            c,
            "CNEW",
            &messages::SyncState {
                newest_ts: Some(ts(21)),
                ..Default::default()
            },
        )?;
        c.execute(
            "UPDATE conversations SET remote_latest=?1 WHERE id='CNEW'",
            [ts(21)],
        )?;
        assert!(!directory::unchanged_conversations(c, "TFAKE")?.contains("CNEW"));
        Ok(())
    })
    .unwrap();
}
#[tokio::test]
async fn remote_failures_keep_local_results_but_auth_aborts() {
    for (code, auth) in [
        ("missing_scope", false),
        ("not_allowed_token_type", false),
        ("invalid_auth", true),
        ("token_revoked", true),
        ("not_authed", true),
    ] {
        let db = seed();
        let server = MockServer::start().await;
        let client = client(&server);
        Mock::given(path("/search.messages"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":code})),
            )
            .mount(&server)
            .await;
        let result = run(
            &db,
            "hello",
            &Options {
                limit: 20,
                ..Default::default()
            },
            Some(&client),
        )
        .await;
        if auth {
            assert!(result.err().unwrap().is_auth_error());
        } else {
            let p = result.unwrap();
            assert_eq!(p.results.len(), 7);
            assert_eq!(p.remote.error.unwrap().code, code);
            assert!(p.results.iter().all(|h| h.source == "local"));
        }
    }
    let db = seed();
    let server = MockServer::start().await;
    let client = client(&server);
    Mock::given(path("/search.messages"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let p = run(
        &db,
        "hello",
        &Options {
            limit: 20,
            ..Default::default()
        },
        Some(&client),
    )
    .await
    .unwrap();
    assert_eq!(p.results.len(), 7);
    assert_eq!(p.remote.error.unwrap().code, "network");
}
#[tokio::test]
async fn remote_pages_cap_at_three_and_partial_failures_keep_ingested_evidence() {
    let db = seed();
    let server = MockServer::start().await;
    let client = client(&server);
    for page in 1..=3 {
        Mock::given(path("/search.messages")).and(field("page",page.to_string())).and(field("count","100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"messages":{
                "total":1000,"pagination":{"page_count":10},"matches":[{"channel":{"id":"CNEW","name":"discovery"},"ts":ts(100+page),"text":"hello remote"}]
            }}))).expect(1).mount(&server).await;
    }
    let p = run(
        &db,
        "hello",
        &Options {
            limit: 500,
            ..Default::default()
        },
        Some(&client),
    )
    .await
    .unwrap();
    assert_eq!(p.remote.pages, 3);
    assert_eq!(p.results.len(), 10);
    let server = MockServer::start().await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    Mock::given(path("/search.messages")).and(field("page","1")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"messages":{
        "total":100,"paging":{"pages":5},"matches":[{"channel":{"id":"CNEW","name":"discovery"},"ts":ts(999),"text":"hello partial"}]
    }}))).mount(&server).await;
    Mock::given(path("/search.messages"))
        .and(field("page", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":"missing_scope"})),
        )
        .mount(&server)
        .await;
    let p = run(
        &db,
        "hello",
        &Options {
            limit: 101,
            ..Default::default()
        },
        Some(&client),
    )
    .await
    .unwrap();
    assert_eq!(p.remote.pages, 1);
    assert_eq!(p.remote.error.unwrap().code, "missing_scope");
    assert!(p
        .results
        .iter()
        .any(|h| h.result.message.ts == ts(999) && h.source == "remote"));
}

#[tokio::test]
async fn typed_search_recent_sort_and_count_are_bounded() {
    let server = MockServer::start().await;
    Mock::given(path("/search.messages"))
        .and(field("count", "100"))
        .and(field("sort", "timestamp"))
        .and(field("page", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"messages":{"total":0,"matches":[]}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let p = client(&server)
        .search_messages("hello", 0, 999, api::SearchSort::Timestamp)
        .await
        .unwrap();
    assert_eq!(p.total, 0);
    let value: Value = serde_json::to_value(
        run(
            &seed(),
            "hello",
            &Options {
                limit: 1,
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert!(value["results"][0]["text"].is_string());
}

#[tokio::test]
async fn continuation_search_observes_tier_two_spacing() {
    let server = MockServer::start().await;
    Mock::given(path("/search.messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok":true,"messages":{"total":0,"matches":[]}
        })))
        .expect(2)
        .mount(&server)
        .await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap();
    client
        .search_messages("hello", 1, 10, api::SearchSort::Score)
        .await
        .unwrap();
    let started = std::time::Instant::now();
    client
        .search_messages("hello", 2, 10, api::SearchSort::Score)
        .await
        .unwrap();
    assert!(started.elapsed() >= std::time::Duration::from_secs(3));
}

#[tokio::test]
async fn numbered_pages_use_a_fixed_size_with_a_small_fetch_margin() {
    let db = seed();
    let server = MockServer::start().await;
    for page in 1..=2 {
        let matches: Vec<_> = (0..53)
            .map(|i| {
                json!({
                    "channel":{"id":"CNEW","name":"discovery"},
                    "ts":ts(100 + (page - 1) * 53 + i), "text":"hello remote"
                })
            })
            .collect();
        Mock::given(path("/search.messages"))
            .and(field("page", page.to_string()))
            .and(field("count", "53"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ok":true,"messages":{"total":1000,"paging":{"pages":20},"matches":matches}
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    let page = run(
        &db,
        "hello",
        &Options {
            limit: 101,
            ..Default::default()
        },
        Some(&client(&server)),
    )
    .await
    .unwrap();
    assert_eq!(page.remote.pages, 2);
    assert_eq!(page.results.len(), 101);
    assert!(page.next_cursor.is_some());
}

#[tokio::test]
async fn equal_timestamp_pages_dedupe_per_conversation_and_tie_break_by_id() {
    let db = seed();
    db.with_conn(|c| {
        messages::upsert_messages(
            c,
            "COTHER",
            &[MessageRow {
                ts: ts(5),
                text: "hello complete local content".into(),
                raw_json: "{}".into(),
                ..Default::default()
            }],
        )
    })
    .unwrap();
    let mut o = Options {
        limit: 1,
        recent: true,
        since: Some(ts(5)),
        until: Some(ts(6)),
        ..Default::default()
    };
    let first = run(&db, "hello", &o, None).await.unwrap();
    assert_eq!(first.results[0].result.message.conversation_id, "CFAKE");
    o.cursor = first.next_cursor;
    let second = run(&db, "hello", &o, None).await.unwrap();
    assert_eq!(second.results[0].result.message.conversation_id, "COTHER");
    assert!(second.next_cursor.is_none());
}

#[test]
fn filter_resolution_preserves_case_ranking_and_sigils() {
    let db = seed();
    db.with_conn(|c| {
        let conversations: Vec<api::Conversation> = serde_json::from_value(json!([
            {"id":"CCASE","name":"General"},
            {"id":"DALICE","is_im":true,"user":"UFAKE"},
            {"id":"CALICE","name":"Alice"}
        ]))?;
        directory::upsert_conversations(c, "TFAKE", "USELF", &conversations)
    })
    .unwrap();
    let o = Options::default();
    for (input, expected) in [
        ("in:#General", "CCASE"),
        ("in:#general", "CFAKE"),
        ("in:@Alice", "DALICE"),
        ("in:#Alice", "CALICE"),
    ] {
        let p = db
            .with_read(|c| search::prepare(c, "TFAKE", input, &o))
            .unwrap();
        assert_eq!(p.query.in_conversations[0].id, expected);
    }
}
