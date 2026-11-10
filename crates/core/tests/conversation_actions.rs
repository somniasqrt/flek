//! Sidebar actions against fake Slack and temporary SQLite only.
mod support;
use flek_core::{
    api::Client,
    store::{directory, Store},
    sync::{
        conversation,
        live::{bus, Change},
        read,
    },
    Secret,
};
use serde_json::json;
use support::field;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn client(server: &MockServer) -> Client {
    Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing()
}
fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.with_conn(|c| {
        c.execute_batch(
            "INSERT INTO teams(id,name,self_user_id) VALUES ('T1','Fake','U1');
             INSERT INTO conversations(id,team_id,kind,name,last_read,is_member,raw_json)
             VALUES ('C1','T1','public_channel','test','100.000000',1,'{}'),
                    ('D1','T1','im','dm','100.000000',1,'{}');
             INSERT INTO messages(conversation_id,ts,user_id,text,raw_json)
             VALUES ('C1','101.000000','U2','fake','{}'),
                    ('D1','101.000000','U2','fake','{}');",
        )?;
        Ok(())
    })
    .unwrap();
    s
}

#[tokio::test]
async fn mark_is_forward_only_recounts_and_emits_after_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/conversations.mark"))
        .and(field("channel", "C1"))
        .and(field("ts", "101.000000"))
        .and(field("token", "xoxc-fake"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    let bus = bus();
    let mut rx = bus.subscribe();
    read::mark(&client(&server), &s, &bus, "C1", "101.000000")
        .await
        .unwrap();
    assert_eq!(
        rx.recv().await.unwrap(),
        Change::ReadMarker {
            conversation_id: "C1".into()
        }
    );
    read::mark(&client(&server), &s, &bus, "C1", "100.000000")
        .await
        .unwrap();
    read::mark(&client(&server), &s, &bus, "C1", "101.000000")
        .await
        .unwrap();
    assert!(rx.try_recv().is_err());
    s.with_read(|c| {
        let row = directory::list_conversations(c, "T1")?.remove(0);
        assert_eq!(row.last_read.as_deref(), Some("101.000000"));
        assert_eq!(row.unread, Some(0));
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn failure_leaves_marker_membership_and_closed_state_unchanged() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":"missing_scope"})),
        )
        .mount(&server)
        .await;
    let s = store();
    let bus = bus();
    let mut rx = bus.subscribe();
    let client = client(&server);
    assert!(read::mark(&client, &s, &bus, "C1", "101.000000")
        .await
        .is_err());
    for id in ["C1", "D1"] {
        assert!(conversation::dismiss(&client, &s, &bus, id).await.is_err());
    }
    assert!(rx.try_recv().is_err());
    s.with_read(|c| {
        assert_eq!(directory::list_conversations(c, "T1")?.len(), 2);
        let marker: String = c.query_row(
            "SELECT last_read FROM conversations WHERE id='C1'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(marker, "100.000000");
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn local_preferences_persist_and_invalid_choice_is_rejected() {
    let s = store();
    let bus = bus();
    let mut rx = bus.subscribe();
    conversation::preference(&s, &bus, "C1", Some(true), Some("muted".into()))
        .await
        .unwrap();
    assert_eq!(rx.recv().await.unwrap(), Change::Directory);
    assert!(
        conversation::preference(&s, &bus, "C1", Some(false), Some("invalid".into()))
            .await
            .is_err()
    );
    s.with_read(|c| {
        let row = directory::list_conversations(c, "T1")?.remove(0);
        assert!(row.starred);
        assert_eq!(row.notification, "muted");
        assert_eq!(row.unread, Some(0));
        let activity = flek_core::rail::activity(c, "T1", "all", true, 50, 0)?;
        assert!(activity.items.iter().all(|x| x.conversation_id != "C1"));
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn leave_and_close_update_db_after_confirmation_and_new_dm_reopens() {
    let server = MockServer::start().await;
    for (name, id) in [("leave", "C1"), ("close", "D1")] {
        Mock::given(method("POST"))
            .and(path(format!("/conversations.{name}")))
            .and(field("channel", id))
            .and(field("token", "xoxc-fake"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
            .expect(1)
            .mount(&server)
            .await;
    }
    let s = store();
    let bus = bus();
    let mut rx = bus.subscribe();
    let client = client(&server);
    for id in ["C1", "D1"] {
        conversation::dismiss(&client, &s, &bus, id).await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), Change::Directory);
    }
    s.with_conn(|c| {
        assert!(directory::list_conversations(c, "T1")?.is_empty());
        assert!(flek_core::rail::direct_messages(c, "T1")?.is_empty());
        // Replayed history must not reopen a DM.
        c.execute_batch(
            "INSERT INTO messages(conversation_id,ts,text,raw_json)
             VALUES ('D1','100.000001','older','{}');",
        )?;
        assert!(directory::list_conversations(c, "T1")?.is_empty());
        c.execute_batch(
            "INSERT INTO messages(conversation_id,ts,text,raw_json)
             VALUES ('D1','102.000000','new','{}');",
        )?;
        assert_eq!(directory::list_conversations(c, "T1")?.len(), 1);
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn general_cannot_be_left_and_external_read_events_refresh_counters() {
    let server = MockServer::start().await;
    let s = store();
    let bus = bus();
    s.with_conn(|c| {
        c.execute(
            "UPDATE conversations SET raw_json = '{\"is_general\":true}' WHERE id='C1'",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    assert!(conversation::dismiss(&client(&server), &s, &bus, "C1")
        .await
        .is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
    let applied = flek_core::sync::live::apply(
        &s,
        flek_core::rtm::Event::Marked {
            channel: "D1".into(),
            ts: "101.000000".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        applied,
        flek_core::sync::live::Applied::Changed(Change::ReadMarker {
            conversation_id: "D1".into()
        })
    );
    s.with_read(|c| {
        assert_eq!(
            flek_core::rail::direct_messages(c, "T1")?[0].unread,
            Some(0)
        );
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn concurrent_marks_cannot_send_an_older_cursor_after_a_newer_one() {
    let server = MockServer::start().await;
    Mock::given(path("/conversations.mark"))
        .and(field("ts", "102.000000"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(20))
                .set_body_json(json!({"ok":true})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    let bus = bus();
    let client = client(&server);
    let newer = read::mark(&client, &s, &bus, "C1", "102.000000");
    let older = read::mark(&client, &s, &bus, "C1", "101.000000");
    let (a, b) = tokio::join!(newer, older);
    a.unwrap();
    b.unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn slack_cant_leave_general_keeps_unknown_general_in_the_directory() {
    let server = MockServer::start().await;
    Mock::given(path("/conversations.leave"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":false,"error":"cant_leave_general"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let s = store();
    let bus = bus();
    let error = conversation::dismiss(&client(&server), &s, &bus, "C1")
        .await
        .unwrap_err();
    assert!(matches!(error, flek_core::Error::Slack { code, .. } if code == "cant_leave_general"));
    s.with_read(|c| {
        assert_eq!(directory::list_conversations(c, "T1")?.len(), 2);
        Ok(())
    })
    .unwrap();
}
