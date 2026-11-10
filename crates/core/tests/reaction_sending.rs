//! Reaction writes against a fake Slack, including RTM echo and failure paths.
mod support;
use flek_core::{
    api::Client,
    rtm::Event,
    store::{
        messages::{self, MessageRow},
        Store,
    },
    sync::{live, reactions},
    Secret,
};
use serde_json::json;
use support::field;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.with_conn(|c| {
        c.execute_batch("INSERT INTO teams (id,name,self_user_id) VALUES ('T1','Team','U1'); INSERT INTO conversations (id,team_id,kind,name) VALUES ('C1','T1','public_channel','general');")?;
        messages::upsert_messages(c, "C1", &[MessageRow { ts: "1.000000".into(), text: "test".into(), raw_json: "{}".into(), ..Default::default() }])?;
        Ok(())
    }).unwrap();
    s
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

#[tokio::test]
async fn add_echo_remove_and_ownership() {
    let server = MockServer::start().await;
    for endpoint in ["reactions.add", "reactions.remove"] {
        Mock::given(method("POST"))
            .and(path(format!("/{endpoint}")))
            .and(field("channel", "C1"))
            .and(field("timestamp", "1.000000"))
            .and(field("name", "heart"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
            .expect(1)
            .mount(&server)
            .await;
    }
    let s = store();
    let client = client(&server);
    // Another user's reaction must survive removing ours.
    live::apply(
        &s,
        Event::ReactionAdded {
            event_ts: None,
            channel: "C1".into(),
            ts: "1.000000".into(),
            name: "heart".into(),
            user: "U2".into(),
        },
    )
    .await
    .unwrap();
    reactions::set(&client, &s, "U1", "C1", "1.000000", "heart", true)
        .await
        .unwrap();
    live::apply(
        &s,
        Event::ReactionAdded {
            event_ts: None,
            channel: "C1".into(),
            ts: "1.000000".into(),
            name: "heart".into(),
            user: "U1".into(),
        },
    )
    .await
    .unwrap();
    let row = s
        .with_read(|c| messages::message(c, "C1", "1.000000"))
        .unwrap()
        .unwrap();
    assert_eq!(row.own_reactions, ["heart"]);
    assert_eq!(row.reactions.as_deref(), Some(":heart: 2"));
    reactions::set(&client, &s, "U1", "C1", "1.000000", "heart", false)
        .await
        .unwrap();
    let row = s
        .with_read(|c| messages::message(c, "C1", "1.000000"))
        .unwrap()
        .unwrap();
    assert!(row.own_reactions.is_empty());
    assert_eq!(row.reactions.as_deref(), Some(":heart: 1"));
}

#[tokio::test]
async fn refused_reaction_does_not_change_database() {
    let server = MockServer::start().await;
    Mock::given(path("/reactions.add"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":"invalid_name"})),
        )
        .mount(&server)
        .await;
    let s = store();
    assert!(
        reactions::set(&client(&server), &s, "U1", "C1", "1.000000", "bad", true)
            .await
            .is_err()
    );
    assert!(s
        .with_read(|c| messages::reactions(c, "C1", "1.000000"))
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn already_in_requested_state_reconciles_local_copy() {
    let server = MockServer::start().await;
    for (endpoint, code) in [
        ("reactions.add", "already_reacted"),
        ("reactions.remove", "no_reaction"),
    ] {
        Mock::given(path(format!("/{endpoint}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"ok":false,"error":code})),
            )
            .mount(&server)
            .await;
    }
    let s = store();
    let client = client(&server);
    reactions::set(&client, &s, "U1", "C1", "1.000000", "heart", true)
        .await
        .unwrap();
    assert_eq!(
        s.with_read(|c| messages::reactions(c, "C1", "1.000000"))
            .unwrap()
            .len(),
        1
    );
    reactions::set(&client, &s, "U1", "C1", "1.000000", "heart", false)
        .await
        .unwrap();
    assert!(s
        .with_read(|c| messages::reactions(c, "C1", "1.000000"))
        .unwrap()
        .is_empty());
}
