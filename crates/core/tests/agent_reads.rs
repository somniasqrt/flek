//! M8.2 reads use synthetic identities and SQLite only.
mod support;
use flek_core::{
    api,
    reads::{self, FileFilter, Window},
    references::{self, Match},
    store::{
        directory,
        messages::{self, MessageRow},
        Store,
    },
    Error,
};
use serde_json::json;

fn seed() -> Store {
    seed_store(Store::open_in_memory().unwrap())
}
fn seed_store(db: Store) -> Store {
    db.with_conn(|c| {
        directory::upsert_team(c,"TFAKE","Fake","https://fake.slack.com/","USELF")?;
        let users=serde_json::from_value::<Vec<api::User>>(json!([
            {"id":"USELF","name":"me"},
            {"id":"UONE","name":"alice","real_name":"Alice Example","profile":{"display_name":"Alice","email":"alice@example.test","title":"Engineer","status_text":"Reading","status_emoji":":book:","status_expiration":123},"tz":"Europe/Kyiv","is_admin":true},
            {"id":"UTWO","name":"alicia","profile":{"display_name":"Ally"}},
            {"id":"WBOT","name":"robot","is_bot":true},
            {"id":"UGONE","name":"gone","deleted":true}
        ]))?;
        // Typed APIs retain raw_json explicitly.
        let raw=json!({"id":"UONE","name":"alice","profile":{"email":"alice@example.test","title":"Engineer","status_text":"Reading"}});
        let mut users=users; users[1].raw=raw;
        directory::upsert_users(c,"TFAKE",&users)?;
        let convs: Vec<api::Conversation>=serde_json::from_value(json!([
            {"id":"CFAKE","name":"general"},{"id":"COTHER","name":"random"},
            {"id":"DFAKE","is_im":true,"user":"UONE"},{"id":"CUNKNOWN","name":"unknown"}
        ]))?;
        directory::upsert_conversations(c,"TFAKE","USELF",&convs)?;
        let mut rows=Vec::new();
        for i in 0..10 {
            rows.push(MessageRow { ts:ts(i),user_id:Some(if i==8 {"USELF"} else {"UONE"}.into()),text:match i {3|8=>"hello <@USELF>",4=>"hello <!here>",_=>"ordinary"}.into(),raw_json:"{}".into(),..Default::default() });
        }
        rows.push(MessageRow {ts:ts(11),thread_ts:Some(ts(3)),user_id:Some("UONE".into()),text:"reply".into(),raw_json:"{}".into(),..Default::default()});
        rows.push(MessageRow {ts:ts(12),thread_ts:Some(ts(3)),user_id:Some("UONE".into()),text:"reply <@USELF>".into(),raw_json:"{}".into(),..Default::default()});
        rows[2].files.push(flek_core::store::files::FileRow {id:"FFAKE".into(),name:Some("photo.png".into()),mimetype:Some("image/png".into()),raw_json:"{}".into(),..Default::default()});
        messages::upsert_messages(c,"CFAKE",&rows)?;
        for id in ["DFAKE","COTHER"] {messages::upsert_messages(c,id,&[MessageRow {ts:ts(20),user_id:Some("UONE".into()),text:"ordinary".into(),raw_json:"{}".into(),..Default::default()}])?;}
        directory::set_read_markers(c,&[ ("CFAKE".into(),Some(ts(1))),("COTHER".into(),Some(ts(1))),("DFAKE".into(),Some(ts(1))) ])?;
        Ok(())
    }).unwrap();
    db
}
fn ts(i: usize) -> String {
    format!("1700000000.{i:06}")
}
#[test]
fn user_resolution_profiles_and_filters() {
    let db = seed();
    db.with_read(|c| {
        for q in [
            "UONE",
            "@alice",
            "ALICE",
            "alice@example.test",
            "Alice Example",
            "AlicE",
        ] {
            assert_eq!(
                references::resolve_user(c, "TFAKE", q, Match::AllowPrefix)?.id,
                "UONE"
            );
        }
        assert!(matches!(
            references::resolve_user(c, "TFAKE", "ali", Match::AllowPrefix),
            Err(Error::Ambiguous { .. })
        ));
        assert!(matches!(
            references::resolve_user(c, "TFAKE", "nobody", Match::AllowPrefix),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            references::resolve_user(c, "TFAKE", "alic", Match::Exact),
            Err(Error::Ambiguous { .. })
        ));
        let user = references::resolve_user(c, "TFAKE", "alice", Match::AllowPrefix)?;
        assert_eq!(user.title.as_deref(), Some("Engineer"));
        assert_eq!(user.dm_conversation_id.as_deref(), Some("DFAKE"));
        assert_eq!(reads::users(c, "TFAKE", None, false, false, 100)?.len(), 3);
        assert_eq!(
            reads::users(c, "TFAKE", Some("alic"), true, true, 1)?.len(),
            1
        );
        Ok(())
    })
    .unwrap();
}
#[test]
fn windows_cursors_around_and_thread_paging() {
    let db = seed();
    db.with_read(|c| {
        let mut w = Window {
            limit: 3,
            ..Default::default()
        };
        let p = reads::history(c, "CFAKE", &w)?;
        assert_eq!(
            p.messages.iter().map(|m| m.ts.clone()).collect::<Vec<_>>(),
            vec![ts(7), ts(8), ts(9)]
        );
        assert!(p.prev_cursor.is_none());
        w.cursor = p.next_cursor.clone();
        let older = reads::history(c, "CFAKE", &w)?;
        assert_eq!(older.messages[0].ts, ts(4));
        w.cursor = older.prev_cursor;
        assert_eq!(reads::history(c, "CFAKE", &w)?.messages[0].ts, ts(7));
        w.cursor = Some("garbage".into());
        assert!(matches!(
            reads::history(c, "CFAKE", &w),
            Err(Error::Usage(_))
        ));
        w.cursor = None;
        w.around = Some(ts(3));
        let around = reads::history(c, "CFAKE", &w)?;
        assert_eq!(around.messages[1].ts, ts(3));
        w.around = None;
        w.since = Some(ts(2));
        w.until = Some(ts(4));
        assert_eq!(reads::history(c, "CFAKE", &w)?.messages.len(), 3);
        w = Window {
            thread: Some(ts(3)),
            limit: 2,
            ..Default::default()
        };
        let p = reads::history(c, "CFAKE", &w)?;
        assert_eq!(p.messages[0].ts, ts(11));
        w.cursor = p.next_cursor;
        assert_eq!(reads::history(c, "CFAKE", &w)?.messages[0].ts, ts(3));
        let r = references::message_reference(
            c,
            "TFAKE",
            "https://fake.slack.com/archives/CFAKE/p1700000000000012",
            None,
        )?;
        assert_eq!(reads::thread_root(c, &r.conversation_id, &r.ts)?, ts(3));
        assert_eq!(reads::thread(c, &r.conversation_id, &r.ts)?.len(), 3);
        let context = reads::message(c, "CFAKE", &ts(11), 1)?;
        assert_eq!(context.before[0].ts, ts(3));
        assert_eq!(context.after[0].ts, ts(12));
        let context = reads::message(c, "CFAKE", &ts(3), 1)?;
        assert_eq!(context.before[0].ts, ts(2));
        assert_eq!(context.after[0].ts, ts(4));
        assert!(matches!(
            reads::message(c, "CFAKE", &ts(99), 0),
            Err(Error::NotFound(_))
        ));
        Ok(())
    })
    .unwrap();
}
#[test]
fn unread_mentions_files_and_unknown_sets() {
    let db = seed();
    db.with_read(|c| {
        let (u, unknown) = reads::unread(c, "TFAKE", 2)?;
        assert_eq!(
            u.iter()
                .map(|u| u.conversation.id.as_str())
                .collect::<Vec<_>>(),
            vec!["DFAKE", "CFAKE", "COTHER"]
        );
        assert_eq!(u[1].mentions, 1);
        assert_eq!(u[1].first_unread_ts, Some(ts(2)));
        assert_eq!(u[1].messages.len(), 2);
        assert_eq!(unknown.len(), 1);
        let m = reads::mentions(c, "TFAKE", &ts(0), None, false, 50)?;
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].ts, ts(12));
        assert_eq!(
            reads::mentions(c, "TFAKE", &ts(0), None, true, 50)?.len(),
            3
        );
        assert_eq!(
            reads::mentions(c, "TFAKE", &ts(4), Some(&ts(9)), false, 50)?.len(),
            0
        );
        let f = reads::files(
            c,
            "TFAKE",
            &FileFilter {
                kind: Some("image".into()),
                user: Some("UONE".into()),
                limit: 50,
                ..Default::default()
            },
        )?;
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].ts, Some(ts(2)));
        assert!(f[0].permalink.is_some());
        assert!(reads::files(
            c,
            "TFAKE",
            &FileFilter {
                kind: Some("pdf".into()),
                limit: 50,
                ..Default::default()
            }
        )?
        .is_empty());
        assert!(reads::files(
            c,
            "TFAKE",
            &FileFilter {
                since: Some(ts(3)),
                limit: 50,
                ..Default::default()
            }
        )?
        .is_empty());
        let info = reads::channel_info(c, "CFAKE")?;
        assert!(info.members.is_none());
        assert!(info.pins.is_none());
        assert!(info.members_synced_at.is_none());
        Ok(())
    })
    .unwrap();
}
#[test]
#[ignore = "100k synthetic message performance benchmark"]
fn mentions_100k_benchmark() {
    let temp = tempfile::tempdir().unwrap();
    let db = seed_store(Store::open(&temp.path().join("benchmark.db")).unwrap());
    db.with_conn(|c| {
        let tx=c.transaction()?;
        {let mut stmt=tx.prepare("INSERT INTO messages(conversation_id,ts,user_id,text) VALUES ('CFAKE',?1,'UONE',?2)")?;
        for i in 0..100000 {stmt.execute(rusqlite::params![format!("1700000001.{i:06}"),if i%1000==0 {"ping <@USELF>"} else {"ordinary benchmark content"}])?;}}
        tx.commit()?;Ok(())
    }).unwrap();
    db.with_read(|c| {
        let start = std::time::Instant::now();
        let m = reads::mentions(c, "TFAKE", "1700000000.000000", None, false, 50)?;
        let elapsed = start.elapsed();
        eprintln!(
            "mentions 100k: {:.3} ms, {} hydrated results",
            elapsed.as_secs_f64() * 1000.,
            m.len()
        );
        assert_eq!(m.len(), 50);
        assert!(elapsed < std::time::Duration::from_millis(50));
        let start = std::time::Instant::now();
        reads::unread(c, "TFAKE", 3)?;
        eprintln!(
            "unread 100k: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.
        );
        let start = std::time::Instant::now();
        reads::history(
            c,
            "CFAKE",
            &Window {
                since: Some("1700000001.000000".into()),
                until: Some("1700000001.050000".into()),
                limit: 30,
                ..Default::default()
            },
        )?;
        eprintln!(
            "history bounded 100k: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.
        );

        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn members_pins_pagination_replacement_and_payload_storage() {
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let db = seed();
    let client = api::Client::with_base_url(
        flek_core::Secret::new("xoxc-fake"),
        flek_core::Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    // Match parsed form data, including cursor forwarding and channel selection.

    Mock::given(path("/conversations.members"))
        .and(support::field("channel", "CFAKE"))
        .and(support::field("cursor", "next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"ok":true,"members":["UTWO"],"response_metadata":{"next_cursor":""}}),
        ))
        .expect(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.members"))
        .and(support::field("channel", "CFAKE"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"ok":true,"members":["UONE"],"response_metadata":{"next_cursor":"next"}}),
        ))
        .expect(1)
        .with_priority(2)
        .mount(&server)
        .await;
    Mock::given(path("/pins.list")).and(support::field("channel","CFAKE"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"items":[{"type":"message","created_by":"UONE","created":123,"message":{"ts":ts(50),"text":"pinned from payload","user":"UONE"}}]}))).expect(1).mount(&server).await;
    flek_core::sync::channel_details::fresh(&client, &db, "CFAKE")
        .await
        .unwrap();
    db.with_read(|c| {
        let info = reads::channel_info(c, "CFAKE")?;
        assert_eq!(info.members.unwrap().len(), 2);
        assert_eq!(info.pins.unwrap()[0].text, "pinned from payload");
        assert!(info.members_synced_at.is_some());
        let pin: (String, i64) = c.query_row(
            "SELECT pinned_by,pinned_at FROM pins WHERE conversation_id='CFAKE'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(pin, ("UONE".into(), 123));
        Ok(())
    })
    .unwrap();
    server.reset().await;
    for (method, key) in [("conversations.members", "members"), ("pins.list", "items")] {
        Mock::given(path(format!("/{method}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,key:[]})))
            .mount(&server)
            .await;
    }
    flek_core::sync::channel_details::fresh(&client, &db, "CFAKE")
        .await
        .unwrap();
    db.with_read(|c| {
        let info = reads::channel_info(c, "CFAKE")?;
        assert!(info.members.unwrap().is_empty());
        assert!(info.pins.unwrap().is_empty());
        assert!(flek_core::views::message(c, "CFAKE", &ts(50))?.is_some());
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn cache_reconciliation_and_hidden_files_need_no_session() {
    let db = seed();
    let temp = tempfile::tempdir().unwrap();
    let path = flek_core::files::cache_path(temp.path(), "FFAKE", Some("photo.png"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "fake image").unwrap();
    // Cache lookups return the resolved path (/var is a symlink on macOS).
    let path = path.canonicalize().unwrap();
    assert_eq!(
        flek_core::files::stored_cached_path(&db, temp.path(), "FFAKE")
            .await
            .unwrap(),
        Some(path.clone())
    );
    db.with_read(|c| {
        assert_eq!(
            reads::file(c, "TFAKE", "FFAKE")?.file.local_path.as_deref(),
            path.to_str()
        );
        Ok(())
    })
    .unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(
        flek_core::files::stored_cached_path(&db, temp.path(), "FFAKE")
            .await
            .unwrap()
            .is_none()
    );
    db.with_conn(|c| {
        c.execute("UPDATE files SET hidden_by_limit=1 WHERE id='FFAKE'", [])?;
        Ok(())
    })
    .unwrap();
    let error = db
        .with_read(|c| reads::file(c, "TFAKE", "FFAKE"))
        .err()
        .unwrap();
    assert!(matches!(error,Error::NotFound(ref s) if s.contains("hidden")));
}

#[tokio::test]
async fn failed_details_fetch_keeps_previous_sets() {
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
    let db = seed();
    db.with_conn(|c| {
        c.execute(
            "INSERT INTO conversation_members VALUES ('CFAKE','UONE')",
            [],
        )?;
        c.execute(
            "INSERT INTO conversation_details_sync VALUES ('CFAKE',123)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let server = MockServer::start().await;
    let client = api::Client::with_base_url(
        flek_core::Secret::new("xoxc-fake"),
        flek_core::Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    Mock::given(path("/conversations.members"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"members":[]})))
        .mount(&server)
        .await;
    Mock::given(path("/pins.list"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":"invalid_auth"})),
        )
        .mount(&server)
        .await;
    assert!(
        flek_core::sync::channel_details::fresh(&client, &db, "CFAKE")
            .await
            .unwrap_err()
            .is_auth_error()
    );
    db.with_read(|c| {
        assert_eq!(reads::channel_info(c, "CFAKE")?.members.unwrap().len(), 1);
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn fresh_unread_catches_changed_remote_activity_even_without_local_unreads() {
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
    let db = seed();
    db.with_conn(|c| {
        for id in ["CFAKE", "COTHER", "DFAKE", "CUNKNOWN"] {
            directory::set_read_markers(c, &[(id.into(), Some(ts(99)))])?;
            messages::set_sync_state(
                c,
                id,
                &messages::SyncState {
                    oldest_ts: Some(ts(0)),
                    newest_ts: Some(ts(99)),
                    complete: true,
                },
            )?;
        }
        Ok(())
    })
    .unwrap();
    let server = MockServer::start().await;
    let client = api::Client::with_base_url(
        flek_core::Secret::new("xoxc-fake"),
        flek_core::Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    for (endpoint, payload) in [
        ("users.list", json!({"members":[]})),
        (
            "users.conversations",
            json!({"channels":[{"id":"CFAKE","name":"general"},{"id":"COTHER","name":"random"},{"id":"CUNKNOWN","name":"unknown"},{"id":"DFAKE","is_im":true,"user":"UONE"}]}),
        ),
        (
            "client.counts",
            json!({"channels":[{"id":"CFAKE","last_read":ts(99),"latest":ts(100)},{"id":"COTHER","last_read":ts(99),"latest":ts(99)},{"id":"CUNKNOWN","last_read":ts(99),"latest":ts(99)}],"ims":[{"id":"DFAKE","last_read":ts(99),"latest":ts(99)}]}),
        ),
    ] {
        let mut payload = payload;
        payload["ok"] = json!(true);
        Mock::given(path(format!("/{endpoint}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(payload))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(path("/conversations.history")).and(support::field("channel","CFAKE")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"messages":[{"ts":ts(100),"text":"remote new <@USELF>","user":"UONE"}],"has_more":false}))).expect(1).mount(&server).await;
    let me = api::AuthTest {
        team_id: "TFAKE".into(),
        team: "Fake".into(),
        url: "https://fake.slack.com/".into(),
        user_id: "USELF".into(),
        user: "me".into(),
        enterprise_id: None,
    };
    flek_core::sync::fresh_unread(&client, &db, &me)
        .await
        .unwrap();
    db.with_read(|c| {
        let (u, _) = reads::unread(c, "TFAKE", 1)?;
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].messages[0].ts, ts(100));
        Ok(())
    })
    .unwrap();
}

#[test]
fn all_file_groups_mime_prefix_and_window_filters() {
    let db = seed();
    db.with_conn(|c| {
        for (i, mime) in [
            "application/pdf",
            "video/mp4",
            "audio/ogg",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "application/zip",
        ]
        .iter()
        .enumerate()
        {
            messages::upsert_messages(
                c,
                "CFAKE",
                &[MessageRow {
                    ts: ts(100 + i),
                    text: "file share".into(),
                    user_id: Some("UONE".into()),
                    raw_json: "{}".into(),
                    files: vec![flek_core::store::files::FileRow {
                        id: format!("FTYPE{i}"),
                        mimetype: Some((*mime).into()),
                        raw_json: "{}".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
            )?;
        }
        Ok(())
    })
    .unwrap();
    db.with_read(|c| {
        for kind in [
            "image",
            "pdf",
            "video",
            "audio",
            "doc",
            "other",
            "application/vnd.",
        ] {
            assert_eq!(
                reads::files(
                    c,
                    "TFAKE",
                    &FileFilter {
                        kind: Some(kind.into()),
                        limit: 20,
                        ..Default::default()
                    }
                )?
                .len(),
                1,
                "{kind}"
            );
        }
        assert_eq!(
            reads::files(
                c,
                "TFAKE",
                &FileFilter {
                    channel: Some("COTHER".into()),
                    limit: 20,
                    ..Default::default()
                }
            )?
            .len(),
            0
        );
        assert_eq!(
            reads::files(
                c,
                "TFAKE",
                &FileFilter {
                    until: Some(ts(101)),
                    since: Some(ts(100)),
                    limit: 20,
                    ..Default::default()
                }
            )?
            .len(),
            2
        );
        assert!(matches!(
            reads::files(
                c,
                "TFAKE",
                &FileFilter {
                    kind: Some("typo".into()),
                    limit: 20,
                    ..Default::default()
                }
            ),
            Err(Error::Usage(_))
        ));
        assert_eq!(
            reads::files(
                c,
                "TFAKE",
                &FileFilter {
                    limit: 0,
                    ..Default::default()
                }
            )?
            .len(),
            0
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn profile_fields_are_additive_and_unknown_members_are_retained() {
    let db = seed();
    db.with_conn(|c| {
        c.execute("UPDATE users SET raw_json=?1 WHERE id='UONE'",[json!({"tz":"Europe/Kyiv","tz_label":"Eastern European Time","is_admin":true,"is_owner":false,"profile":{"title":"Engineer","status_text":"Reading","status_emoji":":book:","status_expiration":123,"email":"alice@example.test"}}).to_string()])?;
        c.execute("INSERT INTO conversation_members VALUES ('CFAKE','UMISSING')",[])?;
        c.execute("INSERT INTO conversation_details_sync VALUES ('CFAKE',123)",[])?;Ok(())
    }).unwrap();
    db.with_read(|c| {
        let u = references::resolve_user(c, "TFAKE", "alice@example.test", Match::AllowPrefix)?;
        assert_eq!(u.status_text.as_deref(), Some("Reading"));
        assert_eq!(u.status_emoji.as_deref(), Some(":book:"));
        assert_eq!(u.status_expiration, Some(123));
        assert_eq!(u.is_admin, Some(true));
        assert_eq!(u.is_owner, Some(false));
        assert_eq!(u.tz.as_deref(), Some("Europe/Kyiv"));
        assert_eq!(u.tz_label.as_deref(), Some("Eastern European Time"));
        assert_eq!(
            references::resolve_user(c, "TFAKE", "WBOT", Match::AllowPrefix)?.id,
            "WBOT"
        );
        let members = reads::channel_info(c, "CFAKE")?.members.unwrap();
        assert_eq!(members[0].id, "UMISSING");
        assert_eq!(members[0].name, "UMISSING");
        assert!(members[0].title.is_none());
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn fresh_reply_reference_fetches_its_root_thread_without_history() {
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
    let db = seed();
    let server = MockServer::start().await;
    let client = api::Client::with_base_url(
        flek_core::Secret::new("xoxc-fake"),
        flek_core::Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    Mock::given(path("/conversations.replies")).and(support::field("channel","CFAKE")).and(support::field("ts",ts(3))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"messages":[{"ts":ts(3),"text":"root refreshed"},{"ts":ts(11),"thread_ts":ts(3),"text":"reply refreshed"}],"has_more":false}))).expect(1).mount(&server).await;
    let reference = references::MessageReference {
        conversation_id: "CFAKE".into(),
        ts: ts(11),
        thread_ts: None,
    };
    flek_core::sync::fresh_message_reference(&client, &db, &reference, true)
        .await
        .unwrap();
    db.with_read(|c| {
        let t = reads::thread(c, "CFAKE", &ts(11))?;
        assert_eq!(t[0].text, "root refreshed");
        assert_eq!(t[1].text, "reply refreshed");
        Ok(())
    })
    .unwrap();
}

async fn unread_refresh_with_error(
    code: &str,
) -> (flek_core::Result<flek_core::sync::FreshUnreadStats>, Store) {
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
    let db = seed();
    db.with_conn(|c| {
        for id in ["CFAKE", "COTHER", "DFAKE", "CUNKNOWN"] {
            messages::set_sync_state(
                c,
                id,
                &messages::SyncState {
                    oldest_ts: Some(ts(0)),
                    newest_ts: Some(ts(99)),
                    complete: true,
                },
            )?;
        }
        Ok(())
    })
    .unwrap();
    let server = MockServer::start().await;
    let client = api::Client::with_base_url(
        flek_core::Secret::new("xoxc-fake"),
        flek_core::Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    for (endpoint, payload) in [
        ("users.list", json!({"members": []})),
        (
            "users.conversations",
            json!({"channels":[{"id":"CFAKE","name":"general"},{"id":"COTHER","name":"random"},{"id":"CUNKNOWN","name":"unknown"},{"id":"DFAKE","is_im":true,"user":"UONE"}]}),
        ),
        (
            "client.counts",
            json!({
                "channels": [
                    {"id": "CFAKE", "last_read": ts(99), "latest": ts(100)},
                    {"id": "COTHER", "last_read": ts(99), "latest": ts(100)},
                    {"id": "CUNKNOWN", "last_read": ts(99), "latest": ts(99)}
                ],
                "ims": [{"id": "DFAKE", "last_read": ts(1), "latest": ts(99)}]
            }),
        ),
    ] {
        let mut payload = payload;
        payload["ok"] = json!(true);
        Mock::given(path(format!("/{endpoint}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(payload))
            .expect(1)
            .mount(&server)
            .await;
    }
    // COTHER's newest local activity sorts it before CFAKE, so this error
    // happens before the successful catch-up. DFAKE is unread but unchanged.
    Mock::given(path("/conversations.history"))
        .and(support::field("channel", "COTHER"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": false, "error": code})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/conversations.history"))
        .and(support::field("channel", "CFAKE"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true, "messages": [{"ts": ts(100), "user": "UONE", "text": "caught up after error"}], "has_more": false
        })))
        .expect(if code == "invalid_auth" { 0 } else { 1 }).mount(&server).await;
    Mock::given(path("/conversations.history"))
        .and(support::field("channel", "DFAKE"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let me = api::AuthTest {
        team_id: "TFAKE".into(),
        team: "Fake".into(),
        url: "https://fake.slack.com/".into(),
        user_id: "USELF".into(),
        user: "me".into(),
        enterprise_id: None,
    };
    let result = flek_core::sync::fresh_unread(&client, &db, &me).await;
    db.with_read(|c| {
        assert!(
            flek_core::views::conversation(c, "DFAKE")?
                .unwrap()
                .unread_count
                .unwrap()
                > 0
        );
        Ok(())
    })
    .unwrap();
    (result, db)
}

#[tokio::test]
async fn fresh_unread_continues_after_failure_and_skips_unchanged_unreads() {
    let (result, db) = unread_refresh_with_error("channel_not_found").await;
    let stats = result.unwrap();
    assert_eq!(stats.caught_up, 1);
    assert_eq!(stats.failed, 1);
    db.with_read(|c| {
        assert_eq!(messages::sync_state(c, "CFAKE")?.newest_ts, Some(ts(100)));
        assert_eq!(
            flek_core::views::message(c, "CFAKE", &ts(100))?
                .unwrap()
                .text,
            "caught up after error"
        );
        assert_eq!(messages::sync_state(c, "COTHER")?.newest_ts, Some(ts(99)));
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn fresh_unread_auth_failure_aborts_remaining_catchups() {
    let (result, db) = unread_refresh_with_error("invalid_auth").await;
    assert!(result.unwrap_err().is_auth_error());
    assert_eq!(
        db.with_read(|c| messages::sync_state(c, "CFAKE"))
            .unwrap()
            .newest_ts,
        Some(ts(99))
    );
}

#[tokio::test]
async fn channel_details_database_failure_rolls_back_messages_and_entire_snapshot() {
    use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};
    let db = seed();
    let before = db
        .with_conn(|c| {
            c.execute(
                "INSERT INTO conversation_members VALUES ('CFAKE', 'UONE')",
                [],
            )?;
            c.execute(
                "INSERT INTO pins VALUES ('CFAKE', ?1, 'UONE', 123)",
                [ts(3)],
            )?;
            c.execute(
                "INSERT INTO conversation_details_sync VALUES ('CFAKE', 123)",
                [],
            )?;
            c.execute_batch(
                "CREATE TRIGGER reject_new_pin BEFORE INSERT ON pins BEGIN
                 SELECT RAISE(ABORT, 'synthetic pin write failure');
             END;",
            )?;
            Ok((
                flek_core::views::conversation(c, "CFAKE")?.unwrap(),
                flek_core::views::message(c, "CFAKE", &ts(3))?.unwrap(),
            ))
        })
        .unwrap();
    let server = MockServer::start().await;
    let client = api::Client::with_base_url(
        flek_core::Secret::new("xoxc-fake"),
        flek_core::Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    Mock::given(path("/conversations.members"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok": true, "members": ["UTWO"]})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/pins.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true, "items": [
            {"type": "message", "created_by": "UTWO", "created": 456, "message": {
                "ts": ts(3), "text": "must roll back update", "user": "UTWO",
                "files": [{"id": "FROLLBACK", "name": "fake.txt"}],
                "reactions": [{"name": "wave", "users": ["UTWO"]}]
            }},
            {"type": "message", "message": {"ts": ts(50), "text": "must roll back insert", "user": "UTWO"}}
        ]})))
        .mount(&server).await;
    assert!(
        flek_core::sync::channel_details::fresh(&client, &db, "CFAKE")
            .await
            .is_err()
    );
    db.with_read(|c| {
        let info = reads::channel_info(c, "CFAKE")?;
        assert_eq!(info.members.unwrap()[0].id, "UONE");
        assert_eq!(info.pins.unwrap()[0].ts, ts(3));
        assert_eq!(info.conversation.latest_ts, before.0.latest_ts);
        assert_eq!(info.conversation.unread_count, before.0.unread_count);
        let message = flek_core::views::message(c, "CFAKE", &ts(3))?.unwrap();
        assert_eq!(message.text, before.1.text);
        assert!(message.files.is_empty());
        assert!(message.reactions.is_empty());
        assert!(flek_core::views::message(c, "CFAKE", &ts(50))?.is_none());
        let (marker, files, pinned_at): (i64, i64, i64) = c.query_row(
            "SELECT (SELECT synced_at FROM conversation_details_sync WHERE conversation_id='CFAKE'),
                    (SELECT COUNT(*) FROM files WHERE id='FROLLBACK'),
                    (SELECT pinned_at FROM pins WHERE conversation_id='CFAKE')",
            [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert_eq!((marker, files, pinned_at), (123, 0, 123));
        Ok(())
    })
    .unwrap();
}

#[test]
fn unread_excludes_left_conversations_without_panicking() {
    let db = seed();
    db.with_conn(|c| {
        c.execute_batch("UPDATE conversations SET is_member=0 WHERE id='CFAKE';")?;
        let (known, unknown) = reads::unread(c, "TFAKE", 3)?;
        assert!(known.iter().all(|r| r.conversation.id != "CFAKE"));
        assert!(unknown.iter().all(|r| r.id != "CFAKE"));
        Ok(())
    })
    .unwrap();
}
