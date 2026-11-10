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
    if g.json {
        let message = db
            .with_read(|c| views::message(c, &conv.id, &sent.ts))?
            .ok_or_else(|| {
                flek_core::Error::NotFound(format!(
                    "no stored message {} in conversation {}",
                    sent.ts, conv.id
                ))
            })?;
        emit(json!({"message":message}));
    } else {
        println!("sent to {} (ts {})", conv.label, sent.ts);
    }
    Ok(())
}
