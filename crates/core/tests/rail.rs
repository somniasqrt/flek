//! Rail regressions: synthetic SQLite and wiremock only.
mod support;
use flek_core::{
    api::{self, Client},
    rail, reads,
    rtm::Event,
    store::{
        directory,
        files::FileRow,
        messages::{self, MessageRow},
        migrations, Store,
    },
    sync::live,
    Secret,
};
use rusqlite::params;
use serde_json::json;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn ts(n: usize) -> String {
    format!("1800000000.{n:06}")
}
fn row(n: usize, user: &str, text: &str) -> MessageRow {
    MessageRow {
        ts: ts(n),
        user_id: Some(user.into()),
        text: text.into(),
        raw_json: "{}".into(),
        ..Default::default()
    }
}
fn seed(db: Store) -> Store {
    db.with_conn(|c| {
        directory::upsert_team(c, "TFAKE", "Fake", "https://fake.slack.com/", "UME")?;
        let users: Vec<api::User> = serde_json::from_value(json!([
            {"id":"UME","name":"me"}, {"id":"UONE","name":"one"},
            {"id":"UTWO","name":"two"}, {"id":"UGONE","name":"gone","deleted":true}
        ]))?;
        directory::upsert_users(c, "TFAKE", &users)?;
        let convs = serde_json::from_value::<Vec<api::Conversation>>(json!([
            {"id":"C1","name":"general"},
            {"id":"D1","is_im":true,"user":"UONE"},
            {"id":"DSELF","is_im":true,"user":"UME"},
            {"id":"DGONE","is_im":true,"user":"UGONE"},
            {"id":"DARCHIVE","is_im":true,"user":"UTWO","is_archived":true},
            {"id":"G1","is_mpim":true,"name":"mpdm-me--one--two-1"}
        ]))?;
        directory::upsert_conversations(c, "TFAKE", "UME", &convs)?;
        c.execute("INSERT INTO conversation_members VALUES ('G1','UONE')", [])?;
        c.execute("INSERT INTO conversation_members VALUES ('G1','UTWO')", [])?;
        for id in ["C1", "D1", "DSELF", "DGONE", "DARCHIVE", "G1"] {
            directory::set_read_markers(c, &[(id.into(), Some(ts(5)))])?;
        }
        messages::upsert_messages(
            c,
            "D1",
            &[
                row(4, "UONE", "old"),
                row(10, "UONE", "hi"),
                row(11, "UME", "mine"),
            ],
        )?;
        messages::upsert_messages(c, "DSELF", &[row(12, "UME", "note to self")])?;
        messages::upsert_messages(c, "DGONE", &[row(99, "UGONE", "gone")])?;
        messages::upsert_messages(c, "DARCHIVE", &[row(98, "UTWO", "archived")])?;
        messages::upsert_messages(c, "G1", &[row(14, "UTWO", "group")])?;
        let mut reply = row(9, "UONE", "reply to my root");
        reply.thread_ts = Some(ts(1));
        let mut mine = row(8, "UME", "my reply");
        mine.thread_ts = Some(ts(2));
        let mut reply2 = row(15, "UTWO", "reply to my reply");
        reply2.thread_ts = Some(ts(2));
        let mut unrelated = row(16, "UTWO", "unrelated thread");
        unrelated.thread_ts = Some(ts(3));
        let mut reacted = row(6, "UME", "reacted message");
        reacted.reactions = vec![
            ("eyes".into(), "UONE".into()),
            ("heart".into(), "UME".into()),
        ];
        messages::upsert_messages(
            c,
            "C1",
            &[
                row(1, "UME", "my root"),
                row(2, "UONE", "another root"),
                row(3, "UONE", "unrelated root"),
                row(4, "UTWO", "old <@UME>"),
                row(7, "UTWO", "hello <@UME>"),
                row(17, "UME", "my own <@UME>"),
                row(18, "UTWO", "<!here> no direct mention"),
                reacted,
                reply,
                mine,
                reply2,
                unrelated,
            ],
        )?;
        Ok(())
    })
    .unwrap();
    db
}
fn db() -> Store {
    seed(Store::open_in_memory().unwrap())
}

#[test]
fn dm_list_latest_unread_self_and_deleted_order() {
    db().with_read(|c| {
        let rows = rail::direct_messages(c, "TFAKE")?;
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].id, "G1");
        assert_eq!(rows[0].members, ["UONE", "UTWO"]);
        assert_eq!(rows.last().unwrap().id, "DGONE");
        let direct = rows.iter().find(|r| r.id == "D1").unwrap();
        assert_eq!(direct.ts.as_deref(), Some(ts(11).as_str()));
        assert_eq!(direct.unread, Some(1));
        assert_eq!(direct.user_id.as_deref(), Some("UME"));
        assert_eq!(direct.text, "mine");
        let self_dm = rows.iter().find(|r| r.id == "DSELF").unwrap();
        assert_eq!(self_dm.partner_id.as_deref(), Some("UME"));
        assert_eq!(self_dm.unread, Some(0));
        Ok(())
    })
    .unwrap();
}

#[test]
fn activity_kinds_own_actions_and_read_markers() {
    db().with_read(|c| {
        let all = rail::activity(c, "TFAKE", "all", false, 100, 0)?;
        assert!(all
            .items
            .iter()
            .any(|r| r.kind == "dm" && r.conversation_id == "D1" && r.ts == ts(10)));
        assert!(all
            .items
            .iter()
            .any(|r| r.kind == "mention" && r.ts == ts(7) && r.unread));
        assert!(all
            .items
            .iter()
            .any(|r| r.kind == "mention" && r.ts == ts(4) && !r.unread));
        assert!(all
            .items
            .iter()
            .any(|r| r.kind == "thread" && r.ts == ts(9)));
        assert!(all
            .items
            .iter()
            .any(|r| r.kind == "thread" && r.ts == ts(15)));
        assert!(!all
            .items
            .iter()
            .any(|r| r.kind == "thread" && r.ts == ts(16)));
        assert!(all
            .items
            .iter()
            .all(|r| r.user_id.as_deref() != Some("UME")));
        assert!(all.items.iter().all(|r| r.conversation_id != "DARCHIVE"));
        let reaction = all.items.iter().find(|r| r.kind == "reaction").unwrap();
        assert!(!reaction.unread);
        assert_eq!(reaction.event_ts, ts(6));
        let unread = rail::activity(c, "TFAKE", "all", true, 100, 0)?;
        assert!(unread.items.iter().all(|r| r.unread));
        assert_eq!(unread.items.len() as i64, all.unread_count);
        let first = rail::activity(c, "TFAKE", "all", false, 2, 0)?;
        let second = rail::activity(c, "TFAKE", "all", false, 2, 2)?;
        assert!(first.has_more);
        assert_eq!(first.items[0].ts, all.items[0].ts);
        assert_eq!(second.items[0].ts, all.items[2].ts);
        let mentions = rail::activity(c, "TFAKE", "mention", false, 100, 0)?;
        assert_eq!(mentions.items.len(), 2);
        assert!(mentions.items.iter().all(|r| r.kind == "mention"));
        assert_eq!(mentions.unread_count, all.unread_count);
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn live_reaction_time_survives_sync_and_duplicate_echo_then_removal() {
    let db = db();
    let event = || {
        Event::parse(
            json!({"type":"reaction_added","user":"UONE","reaction":"eyes",
        "event_ts":ts(30),"item":{"channel":"C1","ts":ts(6),"type":"message"}}),
        )
    };
    live::apply(&db, event()).await.unwrap();
    live::apply(&db, event()).await.unwrap();
    db.with_conn(|c| {
        let mut m = row(6, "UME", "reacted message");
        m.reactions = vec![("eyes".into(), "UONE".into())];
        messages::upsert_messages(c, "C1", &[m])?;
        let page = rail::activity(c, "TFAKE", "all", true, 50, 0)?;
        let reactions = page
            .items
            .iter()
            .filter(|r| r.kind == "reaction")
            .collect::<Vec<_>>();
        assert_eq!(reactions.len(), 1);
        assert_eq!(reactions[0].event_ts, ts(30));
        directory::set_read_markers(c, &[("C1".into(), Some(ts(30)))])?;
        assert!(!rail::activity(c, "TFAKE", "all", true, 50, 0)?
            .items
            .iter()
            .any(|r| r.kind == "reaction"));
        Ok(())
    })
    .unwrap();
    live::apply(
        &db,
        Event::parse(
            json!({"type":"reaction_removed","user":"UONE","reaction":"eyes",
        "item":{"channel":"C1","ts":ts(6),"type":"message"}}),
        ),
    )
    .await
    .unwrap();
    db.with_read(|c| {
        assert!(!rail::activity(c, "TFAKE", "all", false, 50, 0)?
            .items
            .iter()
            .any(|r| r.kind == "reaction"));
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn activity_read_state_for_threads_reactions_and_other_clients() {
    let db = db();
    let unread = |c: &rusqlite::Connection| -> Vec<(String, String)> {
        rail::activity(c, "TFAKE", "all", true, 100, 0)
            .unwrap()
            .items
            .into_iter()
            .map(|r| (r.kind, r.ts))
            .collect()
    };
    let has = |list: &[(String, String)], kind: &str, n: usize| {
        list.iter().any(|(k, t)| k == kind && *t == ts(n))
    };
    // Another client read the thread on root 1 up to reply 9.
    live::apply(
        &db,
        Event::parse(json!({"type":"thread_marked","subscription":{
            "type":"thread","channel":"C1","thread_ts":ts(1),"last_read":ts(9)}})),
    )
    .await
    .unwrap();
    live::apply(
        &db,
        Event::parse(
            json!({"type":"reaction_added","user":"UONE","reaction":"eyes",
        "event_ts":ts(30),"item":{"channel":"C1","ts":ts(6),"type":"message"}}),
        ),
    )
    .await
    .unwrap();
    db.with_conn(|c| {
        let mut reply = row(40, "UTWO", "new reply to my reply");
        reply.thread_ts = Some(ts(2));
        messages::upsert_messages(c, "C1", &[reply])?;
        let list = unread(c);
        assert!(!has(&list, "thread", 9));
        assert!(has(&list, "thread", 40));
        assert!(has(&list, "mention", 7));
        assert!(has(&list, "reaction", 6));
        // Opening the reaction (or the channel on any client) sees the
        // conversation; that does not read its threads.
        rail::seen(c, "C1", None, &ts(30))?;
        let list = unread(c);
        assert!(!has(&list, "reaction", 6));
        assert!(!has(&list, "mention", 7));
        assert!(has(&list, "thread", 40));
        // The thread panel reads the thread; a stale cursor never moves it back.
        rail::seen(c, "C1", Some(&ts(2)), &ts(40))?;
        rail::seen(c, "C1", Some(&ts(2)), &ts(15))?;
        assert!(!has(&unread(c), "thread", 40));
        let mut later = row(41, "UTWO", "later reply");
        later.thread_ts = Some(ts(2));
        messages::upsert_messages(c, "C1", &[later])?;
        assert!(has(&unread(c), "thread", 41));
        Ok(())
    })
    .unwrap();
}

#[test]
fn migration_preserves_reactors_as_read_history() {
    let mut c = rusqlite::Connection::open_in_memory().unwrap();
    migrations::migrate_to(&mut c, 14).unwrap();
    c.execute_batch("INSERT INTO teams(id,name,self_user_id) VALUES ('TFAKE','Fake','UME');
        INSERT INTO conversations(id,team_id,kind,last_read) VALUES ('C1','TFAKE','public_channel','0');
        INSERT INTO messages(conversation_id,ts,user_id,text) VALUES ('C1','1800000000.000001','UME','mine');
        INSERT INTO reactions VALUES ('C1','1800000000.000001','eyes','UONE');").unwrap();
    migrations::migrate(&mut c).unwrap();
    let page = rail::activity(&c, "TFAKE", "all", false, 50, 0).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].user_id.as_deref(), Some("UONE"));
    assert!(!page.items[0].unread);
}

#[test]
fn files_search_title_name_sort_mine_and_categories() {
    db().with_conn(|c| {
        let files = [
            ("F1", "z.png", "image/png", 20, "UTWO"),
            ("F2", "a.pdf", "application/pdf", 300, "UME"),
            ("F3", "b.docx", "application/msword", 100, "UONE"),
            ("F4", "d.zip", "application/zip", 200, "UME"),
            ("F5", "e.mp4", "video/mp4", 400, "UTWO"),
        ];
        for (i, (id, name, mime, size, user)) in files.into_iter().enumerate() {
            let mut m = row(50 + i, user, "");
            m.files = vec![FileRow {
                id: id.into(),
                name: Some(name.into()),
                title: Some(format!("Title {name}")),
                mimetype: Some(mime.into()),
                size: Some(size),
                raw_json: "{}".into(),
                ..Default::default()
            }];
            messages::upsert_messages(c, "C1", &[m])?;
        }
        let filter = reads::FileFilter {
            limit: 100,
            ..Default::default()
        };
        assert_eq!(reads::files(c, "TFAKE", &filter)?[0].file.id, "F5");
        for (kind, n) in [("image", 1), ("pdf", 1), ("doc", 1), ("other", 2)] {
            let f = reads::FileFilter {
                kind: Some(kind.into()),
                other_includes_media: true,
                limit: 100,
                ..Default::default()
            };
            assert_eq!(reads::files(c, "TFAKE", &f)?.len(), n);
        }
        let f = reads::FileFilter {
            mine: true,
            sort: Some("size".into()),
            limit: 100,
            ..Default::default()
        };
        let mine = reads::files(c, "TFAKE", &f)?;
        assert_eq!(
            mine.iter().map(|r| r.file.id.as_str()).collect::<Vec<_>>(),
            ["F2", "F4"]
        );
        let f = reads::FileFilter {
            query: Some("TITLE A".into()),
            limit: 100,
            ..Default::default()
        };
        let found = reads::files(c, "TFAKE", &f)?;
        assert_eq!(found[0].file.name.as_deref(), Some("a.pdf"));
        assert_eq!(found[0].title.as_deref(), Some("Title a.pdf"));
        let f = reads::FileFilter {
            sort: Some("name".into()),
            limit: 2,
            offset: 1,
            ..Default::default()
        };
        assert_eq!(
            reads::files(c, "TFAKE", &f)?
                .iter()
                .map(|r| r.file.id.as_str())
                .collect::<Vec<_>>(),
            ["F3", "F4"]
        );
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn documented_team_info_parsing_storage_and_cached_icon() {
    let server = MockServer::start().await;
    let info = json!({"id":"TFAKE","name":"Fake","icon":{
        "image_132":"https://cdn.example.test/132.png","image_88":"https://cdn.example.test/88.png",
        "image_default":false}});
    Mock::given(method("POST"))
        .and(path("/team.info"))
        .and(support::field("token", "xoxc-fake"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"team":info})))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing();
    let fetched = client.team_info().await.unwrap();
    let db = db();
    let cache = tempfile::NamedTempFile::new().unwrap();
    db.with_conn(|c| {
        directory::set_team_info(c, "TFAKE", &fetched)?;
        assert_eq!(
            directory::team_icon_url(c, "TFAKE")?.as_deref(),
            Some("https://cdn.example.test/132.png")
        );
        assert!(directory::team_icon(c, "TFAKE")?.is_none());
        flek_core::store::unfurls::set_image(
            c,
            "https://cdn.example.test/132.png",
            Some(cache.path().to_str().unwrap()),
        )?;
        assert!(directory::team_icon(c, "TFAKE")?.is_some());
        let mut default = info.clone();
        default["icon"]["image_default"] = json!(true);
        directory::set_team_info(c, "TFAKE", &default)?;
        assert!(directory::team_icon_url(c, "TFAKE")?.is_none());
        let fallback = json!({"icon":{"image_88":"https://cdn.example.test/88.png"}});
        assert_eq!(
            api::team::icon_url(&fallback),
            Some("https://cdn.example.test/88.png")
        );
        Ok(())
    })
    .unwrap();
}

#[test]
#[ignore = "100k synthetic message rail benchmark"]
fn rail_100k_benchmark() {
    let temp = tempfile::tempdir().unwrap();
    let db = seed(Store::open(&temp.path().join("rail.db")).unwrap());
    db.with_conn(|c| {
        let tx = c.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO messages(conversation_id,ts,user_id,text,thread_ts)
                VALUES (?1,?2,?3,?4,?5)",
            )?;
            for i in 0..100_000 {
                let conv = if i % 5 == 0 { "D1" } else { "C1" };
                let user = if i % 10 == 1 { "UME" } else { "UONE" };
                let text = if i % 1000 == 0 {
                    "mention <@UME>"
                } else {
                    "synthetic message"
                };
                let root = if i % 10 >= 2 {
                    Some(format!("1800000001.{:06}", i / 10 * 10 + 1))
                } else {
                    None
                };
                stmt.execute(params![
                    conv,
                    format!("1800000001.{i:06}"),
                    user,
                    text,
                    root
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
    .unwrap();
    db.with_read(|c| {
        let start = std::time::Instant::now();
        let page = rail::activity(c, "TFAKE", "all", false, 50, 0)?;
        let elapsed = start.elapsed();
        eprintln!(
            "Activity 100k: {:.3} ms ({} rows)",
            elapsed.as_secs_f64() * 1000.,
            page.items.len()
        );
        assert_eq!(page.items.len(), 50);
        assert!(elapsed < std::time::Duration::from_millis(50));
        let start = std::time::Instant::now();
        rail::direct_messages(c, "TFAKE")?;
        eprintln!("DMs 100k: {:.3} ms", start.elapsed().as_secs_f64() * 1000.);
        Ok(())
    })
    .unwrap();
}
