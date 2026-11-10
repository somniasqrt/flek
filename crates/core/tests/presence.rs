//! Presence stays local in these tests: fake HTTP, websocket and SQLite only.

mod common;
mod support;
use support::field;

use flek_core::api::{Client, CustomStatus, Dnd, SetPresence, UserPresence};
use flek_core::rtm::Event;
use flek_core::store::{directory, presence, Store};
use flek_core::sync::live::{apply, Applied, Change};
use flek_core::Secret;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    Client::with_base_url(
        Secret::new("xoxc-fake"),
        Secret::new("xoxd-fake"),
        &server.uri(),
    )
    .unwrap()
    .without_pacing()
}

#[tokio::test]
async fn documented_methods_send_exact_fields_and_parse() {
    let server = MockServer::start().await;
    let client = client(&server);
    let status = CustomStatus {
        status_text: "Reading".into(),
        status_emoji: ":books:".into(),
        status_expiration: 1_795_000_000,
    };
    let mocks = [
        (
            "users.getPresence",
            vec![field("user", "UFAKE")],
            json!({"ok":true,"presence":"active","online":true,"auto_away":false,"manual_away":false,"connection_count":2}),
        ),
        (
            "users.setPresence",
            vec![field("presence", "away")],
            json!({"ok":true}),
        ),
        (
            "users.profile.set",
            vec![field("profile", serde_json::to_string(&status).unwrap())],
            json!({"ok":true,"profile":serde_json::to_value(&status).unwrap()}),
        ),
        (
            "dnd.info",
            vec![],
            json!({"ok":true,"dnd_enabled":false,"snooze_enabled":true,"snooze_endtime":2000}),
        ),
        (
            "dnd.teamInfo",
            vec![field("users", "UFAKE,UOTHER")],
            json!({"ok":true,"users":{"UFAKE":{"dnd_enabled":true,"next_dnd_start_ts":1000,"next_dnd_end_ts":3000}}}),
        ),
        (
            "dnd.setSnooze",
            vec![field("num_minutes", "30")],
            json!({"ok":true,"snooze_enabled":true,"snooze_endtime":4000}),
        ),
        ("dnd.endSnooze", vec![], json!({"ok":true})),
    ];
    for (endpoint, fields, body) in mocks {
        let mut mock = Mock::given(method("POST"))
            .and(path(format!("/{endpoint}")))
            .and(field("token", "xoxc-fake"));
        for f in fields {
            mock = mock.and(f);
        }
        mock.respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
    }
    let p = client.users_get_presence("UFAKE").await.unwrap();
    assert_eq!(p.presence, "active");
    assert_eq!(p.connection_count, Some(2));
    assert_eq!(p.manual_away, Some(false));
    client.users_set_presence(SetPresence::Away).await.unwrap();
    let returned = client.users_profile_set(&status).await.unwrap();
    assert_eq!(returned["status_expiration"], 1_795_000_000i64);
    assert_eq!(client.dnd_info().await.unwrap().snooze_endtime, 2000);
    assert_eq!(
        client
            .dnd_team_info(&["UFAKE".into(), "UOTHER".into()])
            .await
            .unwrap()["UFAKE"]
            .next_dnd_end_ts,
        3000
    );
    assert_eq!(
        client.dnd_set_snooze(30).await.unwrap().snooze_endtime,
        4000
    );
    assert!(!client.dnd_end_snooze().await.unwrap().snooze_enabled);
}

#[tokio::test]
async fn events_parse_and_write_single_batch_manual_and_dnd() {
    let store = Store::open_in_memory().unwrap();
    let events = [
        json!({"type":"presence_change","user":"U1","presence":"active"}),
        json!({"type":"presence_change","users":["U2","U3"],"presence":"away"}),
        json!({"type":"manual_presence_change","user":"U1","presence":"away"}),
        json!({"type":"dnd_updated","user":"U1","dnd_status":{"snooze_enabled":true,"snooze_endtime":200}}),
        json!({"type":"dnd_updated_user","user":"U2","dnd_status":{"dnd_enabled":true,"next_dnd_start_ts":10,"next_dnd_end_ts":200}}),
    ];
    for event in events {
        assert_eq!(
            apply(&store, Event::parse(event)).await.unwrap(),
            Applied::Changed(Change::Presence)
        );
    }
    store
        .with_read(|c| {
            let p = presence::get(c, "U1")?.unwrap();
            assert_eq!(p.indicator(100), "away");
            assert_eq!(p.manual_away, Some(true));
            assert!(p.snooze_enabled);
            assert_eq!(presence::get(c, "U2")?.unwrap().indicator(100), "dnd");
            assert_eq!(presence::get(c, "U3")?.unwrap().indicator(100), "away");
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        Event::parse(json!({"type":"manual_presence_change","presence":"active"})),
        Event::ManualPresenceChanged { user: None, .. }
    ));
    assert!(matches!(
        Event::parse(json!({"type":"dnd_updated","dnd_status":{"dnd_enabled":false}})),
        Event::DndUpdated { user: None, .. }
    ));
}

#[test]
fn mapping_and_stale_sources_are_independent() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            presence::set_presence(
                c,
                "U1",
                &UserPresence {
                    presence: "active".into(),
                    ..Default::default()
                },
            )?;
            assert_eq!(presence::get(c, "U1")?.unwrap().indicator(100), "active");
            presence::set_dnd(
                c,
                "U1",
                &Dnd {
                    dnd_enabled: true,
                    next_dnd_start_ts: 50,
                    next_dnd_end_ts: 150,
                    ..Default::default()
                },
            )?;
            let p = presence::get(c, "U1")?.unwrap();
            assert_eq!(p.indicator(49), "active");
            assert_eq!(p.indicator(50), "dnd");
            assert_eq!(p.indicator(150), "active");
            presence::mark_stale(c)?;
            assert_eq!(presence::get(c, "U1")?.unwrap().indicator(100), "unknown");
            presence::set_dnd(
                c,
                "U1",
                &Dnd {
                    snooze_enabled: true,
                    snooze_endtime: 500,
                    ..Default::default()
                },
            )?;
            assert_eq!(presence::get(c, "U1")?.unwrap().indicator(100), "unknown");
            presence::set_presence(
                c,
                "U1",
                &UserPresence {
                    presence: "active".into(),
                    manual_away: Some(true),
                    ..Default::default()
                },
            )?;
            assert_eq!(presence::get(c, "U1")?.unwrap().indicator(100), "away");
            presence::set_presence(
                c,
                "U1",
                &UserPresence {
                    presence: "away".into(),
                    manual_away: Some(false),
                    ..Default::default()
                },
            )?;
            let p = presence::get(c, "U1")?.unwrap();
            assert_eq!(p.indicator(100), "dnd");
            assert_eq!(p.indicator(500), "away");
            for p in [
                UserPresence {
                    presence: "active".into(),
                    online: Some(false),
                    ..Default::default()
                },
                UserPresence {
                    presence: "active".into(),
                    connection_count: Some(0),
                    ..Default::default()
                },
                UserPresence {
                    presence: "active".into(),
                    auto_away: Some(true),
                    ..Default::default()
                },
            ] {
                presence::set_presence(c, "U2", &p)?;
                assert_eq!(presence::get(c, "U2")?.unwrap().indicator(100), "away");
            }
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn status_write_preserves_directory_and_failure_preserves_status() {
    let store = Store::open_in_memory().unwrap();
    apply(&store, Event::parse(json!({"type":"user_change","user":{"id":"UFAKE","team_id":"TFAKE","name":"fake","profile":{"real_name":"Fake","image_72":"https://example.invalid/avatar","status_text":"old","status_emoji":":books:"}}}))).await.unwrap();
    let server = MockServer::start().await;
    Mock::given(path("/users.profile.set")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"profile":{"status_text":"new","status_emoji":":smile:","status_expiration":12345}}))).expect(1).mount(&server).await;
    let bus = flek_core::sync::live::bus();
    let mut changes = bus.subscribe();
    flek_core::sync::presence::set_status(
        &client(&server),
        &store,
        &bus,
        "UFAKE",
        CustomStatus {
            status_text: "new".into(),
            status_emoji: ":smile:".into(),
            status_expiration: 12345,
        },
    )
    .await
    .unwrap();
    assert_eq!(changes.recv().await.unwrap(), Change::Directory);
    store
        .with_read(|c| {
            let p = directory::user_profile(c, "UFAKE")?.unwrap();
            assert_eq!(p.status_text.as_deref(), Some("new"));
            assert_eq!(p.real_name.as_deref(), Some("Fake"));
            assert_eq!(p.status_expiration, Some(12345));
            Ok(())
        })
        .unwrap();
    server.reset().await;
    Mock::given(path("/users.profile.set"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":false,"error":"not_allowed_token_type"})),
        )
        .mount(&server)
        .await;
    assert!(flek_core::sync::presence::set_status(
        &client(&server),
        &store,
        &bus,
        "UFAKE",
        CustomStatus {
            status_text: "lost".into(),
            status_emoji: String::new(),
            status_expiration: 0
        }
    )
    .await
    .is_err());
    assert_eq!(
        store
            .with_read(|c| Ok(directory::user_profile(c, "UFAKE")?.unwrap().status_text))
            .unwrap()
            .as_deref(),
        Some("new")
    );
    // Self user_change often omits team_id: never reassign the user to an empty team.
    apply(&store, Event::parse(json!({"type":"user_change","user":{"id":"UFAKE","name":"fake","profile":{"status_text":"live"}}}))).await.unwrap();
    store
        .with_read(|c| {
            assert_eq!(
                directory::user_profile(c, "UFAKE")?
                    .unwrap()
                    .status_text
                    .as_deref(),
                Some("live")
            );
            assert_eq!(
                c.query_row("SELECT team_id FROM users WHERE id='UFAKE'", [], |r| r
                    .get::<_, String>(
                    0
                ))?,
                "TFAKE"
            );
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn dnd_batches_are_bounded_and_empty_never_requests_the_team() {
    let server = MockServer::start().await;
    Mock::given(path("/dnd.teamInfo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"users":{}})))
        .expect(2)
        .mount(&server)
        .await;
    let client = client(&server);
    assert!(client.dnd_team_info(&[]).await.unwrap().is_empty());
    let ids = (0..51).map(|i| format!("UFAKE{i}")).collect::<Vec<_>>();
    client.dnd_team_info(&ids).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(wiremock::Match::matches(
        &field("users", ids[..50].join(",")),
        &requests[0]
    ));
    assert!(wiremock::Match::matches(
        &field("users", "UFAKE50"),
        &requests[1]
    ));
}

#[tokio::test]
async fn websocket_replaces_subscriptions_resubscribes_and_resolves_self_events() {
    use flek_core::api::AuthTest;
    use flek_core::sync::follow::{follow, FollowConfig};
    use flek_core::sync::live::{bus, ConnectionState};
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message as Frame;

    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let (frames_tx, mut frames_rx) = tokio::sync::mpsc::channel(8);
        let server_task = tokio::spawn(async move {
            for connection in 0..2 {
                let (tcp,_) = listener.accept().await.unwrap();
                let mut ws=tokio_tungstenite::accept_async(tcp).await.unwrap();
                ws.send(Frame::text(json!({"type":"hello"}).to_string())).await.unwrap();
                let sets=if connection==0 { 2 } else { 1 };
                for _ in 0..sets {
                    for kind in ["presence_sub","presence_query"] {
                        let frame=ws.next().await.unwrap().unwrap();
                        let value:serde_json::Value=serde_json::from_str(frame.to_text().unwrap()).unwrap();
                        assert_eq!(value["type"],kind);
                        frames_tx.send(value).await.unwrap();
                    }
                    ws.send(Frame::text(json!({"type":"presence_change","users":["UME","U1","U2"],"presence":"active"}).to_string())).await.unwrap();
                    ws.send(Frame::text(json!({"type":"dnd_updated","dnd_status":{"snooze_enabled":true,"snooze_endtime":9999999999i64}}).to_string())).await.unwrap();
                    ws.send(Frame::text(json!({"type":"manual_presence_change","presence":"away"}).to_string())).await.unwrap();
                }
                if connection==0 {
                    ws.send(Frame::text(json!({"type":"goodbye"}).to_string())).await.unwrap();
                } else { while ws.next().await.is_some() {} }
            }
        });
        let server=MockServer::start().await;
        for (endpoint,response) in [
            ("rtm.connect",json!({"ok":true,"url":url})),
            ("dnd.teamInfo",json!({"ok":true,"users":{}})),
            ("dnd.info",json!({"ok":true,"snooze_enabled":false})),
            ("users.getPresence",json!({"ok":true,"presence":"active","manual_away":false})),
        ] { Mock::given(path(format!("/{endpoint}"))).respond_with(ResponseTemplate::new(200).set_body_json(response)).mount(&server).await; }
        let client=common::websocket_client(&server, &url).await;
        let store=Store::open_in_memory().unwrap();
        let bus=bus(); let mut changes=bus.subscribe();
        let (interest_tx,interest_rx)=tokio::sync::watch::channel(vec!["U1".into(),"UME".into()]);
        let (stop_tx,stop_rx)=tokio::sync::watch::channel(false);
        let cfg=FollowConfig { presence:Some(interest_rx),backoff_min:Duration::from_millis(100),backoff_max:Duration::from_millis(100),..Default::default() };
        let me=AuthTest {url:"https://fake.slack.com".into(),team:"Fake".into(),user:"me".into(),team_id:"TFAKE".into(),user_id:"UME".into(),enterprise_id:None};
        let drive=async {
            for kind in ["presence_sub","presence_query"] {
                let frame=frames_rx.recv().await.unwrap();
                assert_eq!(frame["type"],kind);assert_eq!(frame["ids"],json!(["U1","UME"]));
            }
            loop {
                changes.recv().await.unwrap();
                if store.with_read(|c| Ok(presence::get(c,"UME")?.is_some_and(|p| p.manual_away==Some(true) && p.snooze_enabled))).unwrap() { break; }
            }
            interest_tx.send(vec!["U2".into(),"UME".into()]).unwrap();
            for _ in 0..2 {
                assert_eq!(frames_rx.recv().await.unwrap()["ids"],json!(["U2","UME"]));
            }
            loop {
                if matches!(changes.recv().await.unwrap(),Change::Connection(ConnectionState::Reconnecting {..})) {
                    assert_eq!(store.with_read(|c|Ok(presence::get(c,"UME")?.unwrap().indicator(100))).unwrap(),"unknown");break;
                }
            }
            // New session keeps the replacement set, never merges U1 back in.
            for _ in 0..2 { assert_eq!(frames_rx.recv().await.unwrap()["ids"],json!(["U2","UME"])); }
            stop_tx.send(true).unwrap();
        };
        let (result,())=tokio::join!(follow(&client,&store,&me,&bus,&cfg,async || Ok(()),stop_rx),drive);
        result.unwrap();server_task.await.unwrap();
        assert_eq!(store.with_read(|c|Ok(presence::get(c,"UME")?.unwrap().indicator(100))).unwrap(),"unknown");
    }).await.unwrap();
}

#[tokio::test]
async fn slow_snapshot_does_not_undo_live_updates_and_self_survives_denied_batch() {
    use std::time::Duration;
    let server = MockServer::start().await;
    Mock::given(path("/dnd.teamInfo"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":"missing_scope"})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/dnd.info"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"snooze_enabled":false}))
                .set_delay(Duration::from_millis(100)),
        )
        .mount(&server)
        .await;
    Mock::given(path("/users.getPresence"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"presence":"active","manual_away":false}))
                .set_delay(Duration::from_millis(100)),
        )
        .mount(&server)
        .await;
    let client = client(&server);
    let store = Store::open_in_memory().unwrap();
    let bus = flek_core::sync::live::bus();
    let live = async {
        // Wait until snapshot requests were issued, rather than racing a sleep.
        while server.received_requests().await.unwrap().len() < 3 {
            tokio::task::yield_now().await;
        }
        apply(
            &store,
            Event::parse(json!({"type":"manual_presence_change","user":"UME","presence":"away"})),
        )
        .await
        .unwrap();
        apply(&store,Event::parse(json!({"type":"dnd_updated","user":"UME","dnd_status":{"snooze_enabled":true,"snooze_endtime":500}}))).await.unwrap();
    };
    let (result, ()) = tokio::join!(
        flek_core::sync::presence::snapshot(&client, &store, &bus, "UME", vec!["UME".into()]),
        live
    );
    assert!(result.is_err());
    store
        .with_read(|c| {
            let p = presence::get(c, "UME")?.unwrap();
            assert_eq!(p.manual_away, Some(true));
            assert_eq!(p.snooze_end, 500);
            Ok(())
        })
        .unwrap();
    // Without live changes, successful self endpoints still persist when the batch is denied.
    assert!(flek_core::sync::presence::snapshot(
        &client,
        &store,
        &bus,
        "UOTHER",
        vec!["UOTHER".into()]
    )
    .await
    .is_err());
    assert_eq!(
        store
            .with_read(|c| Ok(presence::get(c, "UOTHER")?.unwrap().indicator(100)))
            .unwrap(),
        "active"
    );
}

#[tokio::test]
async fn snooze_writes_preserve_schedule_and_resume_can_return_only_ok() {
    let store = Store::open_in_memory().unwrap();
    let server = MockServer::start().await;
    store
        .with_conn(|c| {
            presence::set_presence(
                c,
                "UME",
                &UserPresence {
                    presence: "active".into(),
                    ..Default::default()
                },
            )?;
            presence::set_dnd(
                c,
                "UME",
                &Dnd {
                    dnd_enabled: true,
                    next_dnd_start_ts: 50,
                    next_dnd_end_ts: 200,
                    ..Default::default()
                },
            )
        })
        .unwrap();
    Mock::given(path("/dnd.setSnooze"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"snooze_enabled":true,"snooze_endtime":400})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/dnd.endSnooze"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
        .mount(&server)
        .await;
    let bus = flek_core::sync::live::bus();
    let client = client(&server);
    flek_core::sync::presence::snooze(&client, &store, &bus, "UME", Some(30))
        .await
        .unwrap();
    assert_eq!(
        store
            .with_read(|c| Ok(presence::get(c, "UME")?.unwrap().dnd_until(100)))
            .unwrap(),
        Some(400)
    );
    flek_core::sync::presence::snooze(&client, &store, &bus, "UME", None)
        .await
        .unwrap();
    assert_eq!(
        store
            .with_read(|c| Ok(presence::get(c, "UME")?.unwrap().dnd_until(100)))
            .unwrap(),
        Some(200)
    );
}

#[tokio::test]
async fn auto_override_and_failed_writes_do_not_manufacture_active() {
    let store = Store::open_in_memory().unwrap();
    let server = MockServer::start().await;
    store
        .with_conn(|c| {
            presence::set_presence(
                c,
                "UME",
                &UserPresence {
                    presence: "active".into(),
                    manual_away: Some(true),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    Mock::given(path("/users.setPresence"))
        .and(field("presence", "auto"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/users.getPresence"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":"missing_scope"})),
        )
        .mount(&server)
        .await;
    let bus = flek_core::sync::live::bus();
    let client = client(&server);
    flek_core::sync::presence::set_presence(&client, &store, &bus, "UME", SetPresence::Auto)
        .await
        .unwrap();
    store
        .with_read(|c| {
            let p = presence::get(c, "UME")?.unwrap();
            assert_eq!(p.manual_away, Some(false));
            assert_eq!(p.indicator(100), "unknown");
            Ok(())
        })
        .unwrap();
    server.reset().await;
    Mock::given(path("/users.setPresence"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":false,"error":"not_allowed_token_type"})),
        )
        .mount(&server)
        .await;
    assert!(flek_core::sync::presence::set_presence(
        &client,
        &store,
        &bus,
        "UME",
        SetPresence::Away
    )
    .await
    .is_err());
    assert_eq!(
        store
            .with_read(|c| Ok(presence::get(c, "UME")?.unwrap().manual_away))
            .unwrap(),
        Some(false)
    );
}

#[test]
fn cli_user_views_read_stored_presence_and_nullable_dnd_end() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            let raw = json!({"id":"UFAKE","team_id":"TFAKE","name":"fake"});
            let mut user: flek_core::api::User = serde_json::from_value(raw.clone())?;
            user.raw = raw;
            directory::upsert_users(c, "TFAKE", &[user])?;
            assert_eq!(
                flek_core::views::user(c, "UFAKE")?.unwrap().presence,
                "unknown"
            );
            presence::set_presence(
                c,
                "UFAKE",
                &UserPresence {
                    presence: "active".into(),
                    ..Default::default()
                },
            )?;
            let u = flek_core::reads::users(c, "TFAKE", None, false, false, 100)?.remove(0);
            assert_eq!(u.presence, "active");
            assert_eq!(u.dnd_until, None);
            presence::set_dnd(
                c,
                "UFAKE",
                &Dnd {
                    snooze_enabled: true,
                    snooze_endtime: 9999999999,
                    ..Default::default()
                },
            )?;
            let u = flek_core::views::user(c, "UFAKE")?.unwrap();
            assert_eq!(u.presence, "dnd");
            assert_eq!(u.dnd_until, Some(9999999999));
            presence::mark_stale(c)?;
            let u = flek_core::views::user(c, "UFAKE")?.unwrap();
            assert_eq!(u.presence, "unknown");
            assert_eq!(u.dnd_until, None);
            Ok(())
        })
        .unwrap();
}

#[test]
fn subscriptions_are_bounded_deduplicated_and_keep_self() {
    let ids = (0..600).map(|i| format!("U{i:04}")).collect();
    let ids = flek_core::sync::presence::subscription(ids, "ZME");
    assert_eq!(ids.len(), 500);
    assert_eq!(ids[0], "ZME");
    assert_eq!(
        flek_core::sync::presence::subscription(
            vec!["UME".into(), "U1".into(), "U1".into(), "".into()],
            "UME"
        ),
        vec!["UME", "U1"]
    );
}
