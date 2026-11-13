//! Explicit send command.

use crate::{open_db, output::emit, Global};
use anyhow::{Context, Result};
use flek_core::{
    auth::{self, CredentialStore},
    references::{self, Match, Resolution},
    store::Store,
    views,
};
use serde_json::json;

/// Posts one message and stores Slack's copy of it.
pub(super) async fn send(
    creds: &CredentialStore,
    g: &Global,
    query: &str,
    text: String,
    thread: Option<String>,
    broadcast: bool,
) -> Result<()> {
    use flek_core::sync::send::{send, Outgoing};

    let session = auth::resolve_session(creds, g.team.as_deref()).await?;
    let db = open_db(g)?;
    let me = session.identity.clone();
    let client = session.client()?;
    let find =
        |db: &Store| db.with_read(|c| references::resolve(c, &me.team_id, query, Match::Exact));
    let conv = match find(&db)? {
        Resolution::Found(c) => c,
        Resolution::NotFound => {
            // Not stored yet (new DM, fresh database): fetch the directory.
            flek_core::sync::directory::sync_directory(&client, &db, &me).await?;
            find(&db)?.required(query)?
        }
        other => other.required(query)?,
    };
    let sent = send(
        &client,
        &db,
        &me.user_id,
        Outgoing {
            conversation_id: conv.id.clone(),
            blocks: None,
            text,
            thread_ts: thread,
            broadcast,
            client_msg_id: None,
        },
    )
    .await
    .map_err(|e| {
        let description = flek_core::api::chat::describe_send_error(&e);
        anyhow::Error::new(e).context(description)
    })
    .with_context(|| format!("sending to {}", conv.label))?;
    // Remote acceptance is final even if persistence or this local read fails.
    let message = if sent.not_stored_locally {
        None
    } else {
        db.with_read(|c| views::message(c, &conv.id, &sent.ts))
            .ok()
            .flatten()
    };
    let not_stored_locally = sent.not_stored_locally || message.is_none();
    if not_stored_locally {
        eprintln!(
            "warning: message sent to Slack (ts {}); local copy unavailable, sync will recover it",
            sent.ts
        );
    }
    if g.json {
        emit(json!({"message":message, "ts":sent.ts, "not_stored_locally":not_stored_locally}));
    } else {
        println!("sent to {} (ts {})", conv.label, sent.ts);
    }
    Ok(())
}
