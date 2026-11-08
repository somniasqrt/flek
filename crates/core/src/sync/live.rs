//! Live events → database, and the change bus the UI and CLI listen to.
//!
//! Every write goes through the same `store` functions as backfill, so the
//! hidden-by-limit rule and reaction handling are identical.

use tokio::sync::broadcast;

use crate::rtm::Event;
use crate::store::directory;
use crate::store::messages;
use crate::store::Store;
use crate::sync::backfill::to_row;
use crate::Result;

/// What changed in the database (or in the session). Listeners re-read the
/// database; the bus carries no message content.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Change {
    /// New or updated message.
    Message {
        conversation_id: String,
        ts: String,
    },
    MessageDeleted {
        conversation_id: String,
        ts: String,
    },
    Reactions {
        conversation_id: String,
        ts: String,
    },
    ReadMarker {
        conversation_id: String,
    },
    /// Not stored; only for a "typing…" hint.
    Typing {
        conversation_id: String,
        user_id: String,
    },
    /// Sync wrote new or changed messages into a conversation (backfill,
    /// catch-up); listeners re-read what they show of it.
    History {
        conversation_id: String,
    },
    /// Users or conversations changed.
    Directory,
    Connection(ConnectionState),
    /// Background sync progress, for a "Syncing messages" bar.
    Progress(Progress),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum Progress {
    /// Users and conversations.
    Directory,
    /// `done` of `total` conversations; `deep` is the background backfill pass.
    Conversations {
        done: usize,
        total: usize,
        current: String,
        deep: bool,
    },
    Files {
        done: usize,
        total: usize,
    },
    /// A full pass finished.
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionState {
    Connected,
    /// Lost; retrying after the given delay in seconds.
    Reconnecting {
        in_secs: u64,
    },
}

pub type Bus = broadcast::Sender<Change>;

pub fn bus() -> Bus {
    broadcast::channel(1024).0
}

/// Result of applying one event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    Changed(Change),
    /// The event names a conversation we do not know yet (new DM, channel
    /// just joined): refresh the directory and apply it again.
    NeedsDirectory,
    /// The event refers to a conversation change; refresh the directory.
    DirectoryStale,
    Ignored,
}

/// Writes one event to the database.
pub async fn apply(store: &Store, event: Event) -> Result<Applied> {
    store
        .call(move |c| {
            Ok(match event {
                Event::Message { channel, message }
                | Event::MessageChanged { channel, message } => {
                    if !messages::conversation_exists(c, &channel)? {
                        return Ok(Applied::NeedsDirectory);
                    }
                    let row = to_row(&message);
                    // Sync markers are left alone on purpose: after a reconnect a
                    // live message can arrive before catch-up reaches this
                    // conversation, and moving newest_ts past it would skip
                    // whatever was sent while we were offline.
                    messages::upsert_messages(c, &channel, std::slice::from_ref(&row))?;
                    Applied::Changed(Change::Message {
                        conversation_id: channel,
                        ts: row.ts,
                    })
                }
                Event::MessageDeleted { channel, ts } => {
                    messages::mark_deleted(c, &channel, std::slice::from_ref(&ts))?;
                    Applied::Changed(Change::MessageDeleted {
                        conversation_id: channel,
                        ts,
                    })
                }
                Event::ReactionAdded {
                    channel,
                    ts,
                    name,
                    user,
                } => reaction(c, channel, ts, &name, &user, true)?,
                Event::ReactionRemoved {
                    channel,
                    ts,
                    name,
                    user,
                } => reaction(c, channel, ts, &name, &user, false)?,
                Event::Marked { channel, ts } => {
                    directory::set_read_markers(c, &[(channel.clone(), Some(ts))])?;
                    Applied::Changed(Change::ReadMarker {
                        conversation_id: channel,
                    })
                }
                Event::Typing { channel, user } => Applied::Changed(Change::Typing {
                    conversation_id: channel,
                    user_id: user,
                }),
                Event::UserChanged { user } => {
                    let team = user.team_id.clone().unwrap_or_default();
                    directory::upsert_users(c, &team, std::slice::from_ref(&*user))?;
                    Applied::Changed(Change::Directory)
                }
                Event::ConversationsChanged { .. } => Applied::DirectoryStale,
                Event::Hello | Event::Goodbye | Event::Pong { .. } | Event::Unknown { .. } => {
                    Applied::Ignored
                }
            })
        })
        .await
}

fn reaction(
    c: &rusqlite::Connection,
    channel: String,
    ts: String,
    name: &str,
    user: &str,
    on: bool,
) -> Result<Applied> {
    // Unknown message (older than our history): nothing to attach to.
    if !messages::set_reaction(c, &channel, &ts, name, user, on)? {
        return Ok(Applied::Ignored);
    }
    Ok(Applied::Changed(Change::Reactions {
        conversation_id: channel,
        ts,
    }))
}
