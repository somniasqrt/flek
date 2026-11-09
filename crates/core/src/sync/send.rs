//! Sending a message: Slack first, then the database, through the same
//! path as a live event, so the echo Slack sends over the websocket for the
//! same `ts` is an idempotent rewrite of the same row.

use serde::Serialize;

use crate::api::chat::{new_client_msg_id, PostMessage};
use crate::api::Client;
use crate::rtm::Event;
use crate::store::Store;
use crate::sync::live::{self, Applied, Change};
use crate::Result;

/// What the UI or CLI asks to send.
#[derive(Debug, Clone, Default)]
pub struct Outgoing {
    pub conversation_id: String,
    /// As typed (mrkdwn); escaped on the way out.
    pub text: String,
    pub thread_ts: Option<String>,
    pub broadcast: bool,
    /// Chosen by the UI for its optimistic copy; generated when absent.
    pub client_msg_id: Option<String>,
}

/// A sent and stored message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sent {
    pub conversation_id: String,
    pub ts: String,
    pub client_msg_id: String,
    /// The database change to announce, if the message was written (it is
    /// not when the conversation is not stored yet; sync picks it up later).
    #[serde(skip)]
    pub change: Option<Change>,
}

/// Posts `out` as the user and stores Slack's copy of it.
/// `me` fills in the author when Slack's answer leaves it out.
pub async fn send(client: &Client, store: &Store, me: &str, out: Outgoing) -> Result<Sent> {
    let client_msg_id = out
        .client_msg_id
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(new_client_msg_id);
    let posted = client
        .chat_post_message(&PostMessage {
            channel: out.conversation_id.clone(),
            text: out.text,
            thread_ts: out.thread_ts.filter(|t| !t.is_empty()),
            reply_broadcast: out.broadcast,
            client_msg_id: client_msg_id.clone(),
        })
        .await?;

    let mut message = posted.message;
    if message.user.is_none() && message.bot_id.is_none() {
        message.user = Some(me.to_owned());
        if let Some(obj) = message.raw.as_object_mut() {
            obj.insert("user".into(), me.into());
        }
    }
    let ts = message.ts.clone();
    // A live event with the same ts may already have written this row (or
    // come later): both are upserts by (conversation, ts), and counting a
    // reply on its root is idempotent (`store::messages::note_reply`).
    let applied = live::apply(
        store,
        Event::Message {
            channel: posted.channel.clone(),
            message,
        },
    )
    .await?;
    let change = match applied {
        Applied::Changed(c) => Some(c),
        Applied::NeedsDirectory => {
            tracing::warn!("sent into a conversation that is not stored yet");
            None
        }
        _ => None,
    };
    Ok(Sent {
        conversation_id: posted.channel,
        ts,
        client_msg_id,
        change,
    })
}
