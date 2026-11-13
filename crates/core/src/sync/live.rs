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
    Presence,
    Connection(ConnectionState),
    /// Preferences failed to load (or have recovered); no file content.
    SettingsError {
        error: Option<String>,
    },
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
            let posted = matches!(event, Event::Message { .. });
            Ok(match event {
                Event::Message { channel, message }
                | Event::MessageChanged { channel, message } => {
                    if !messages::conversation_exists(c, &channel)? {
                        return Ok(Applied::NeedsDirectory);
                    }
                    let row = to_row(&message);
                    // Events can arrive out of order: a late edit (or a
                    // redelivered message) must not bring back a message
                    // deleted live. History fetches still may: they are
                    // Slack's current state. A tombstone update goes through.
                    if !row.deleted && messages::is_deleted(c, &channel, &row.ts)? {
                        return Ok(Applied::Ignored);
                    }
                    // Sync markers are left alone on purpose: after a reconnect a
                    // live message can arrive before catch-up reaches this
                    // conversation, and moving newest_ts past it would skip
                    // whatever was sent while we were offline.
                    messages::upsert_messages(c, &channel, std::slice::from_ref(&row))?;
                    // Slack does not reliably send message_replied over RTM:
                    // count a new reply on its root right away.
                    if let Some(root) = row.thread_ts.as_deref().filter(|t| *t != row.ts) {
                        if posted {
                            messages::note_reply(c, &channel, root, &row.ts)?;
                        }
                    }
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
                    event_ts,
                    channel,
                    ts,
                    name,
                    user,
                } => {
                    let applied = reaction(c, channel.clone(), ts.clone(), &name, &user, true)?;
                    if let Some(time) = event_ts {
                        c.execute(
                            "
                                UPDATE reactions
                                SET event_ts = COALESCE(event_ts, ?5)
                                WHERE conversation_id = ?1
                                AND ts = ?2
                                AND name = ?3
                                AND user_id = ?4
                            ",
                            rusqlite::params![channel, ts, name, user, time],
                        )?;
                    }
                    applied
                }
                Event::ReactionRemoved {
                    channel,
                    ts,
                    name,
                    user,
                } => reaction(c, channel, ts, &name, &user, false)?,
                Event::Marked { channel, ts } => {
                    directory::set_read_markers(c, &[(channel.clone(), Some(ts))])?;
                    // Some client just looked at it: Activity reactions up to now are seen.
                    crate::rail::seen(c, &channel, None, &crate::sync::ts_days_ago(0))?;
                    Applied::Changed(Change::ReadMarker {
                        conversation_id: channel,
                    })
                }
                Event::ThreadMarked {
                    channel,
                    thread_ts,
                    ts,
                } => {
                    crate::rail::seen(c, &channel, Some(&thread_ts), &ts)?;
                    Applied::Changed(Change::ReadMarker {
                        conversation_id: channel,
                    })
                }
                Event::Typing { channel, user } => Applied::Changed(Change::Typing {
                    conversation_id: channel,
                    user_id: user,
                }),
                Event::PresenceChanged {
                    users,
                    presence: value,
                } => {
                    let tx = c.transaction()?;
                    for id in users {
                        if !id.is_empty() && matches!(value.as_str(), "active" | "away") {
                            crate::store::presence::set_presence(
                                &tx,
                                &id,
                                &crate::api::UserPresence {
                                    presence: value.clone(),
                                    ..Default::default()
                                },
                            )?;
                        }
                    }
                    tx.commit()?;
                    Applied::Changed(Change::Presence)
                }
                Event::ManualPresenceChanged {
                    user: Some(id),
                    presence: value,
                } => {
                    let mut p = crate::store::presence::get(c, &id)?.unwrap_or_default();
                    let away = value == "away";
                    if away {
                        p.presence = Some("away".into());
                    }
                    crate::store::presence::set_presence(
                        c,
                        &id,
                        &crate::api::UserPresence {
                            presence: p.presence.unwrap_or_else(|| "away".into()),
                            manual_away: Some(away),
                            ..Default::default()
                        },
                    )?;
                    Applied::Changed(Change::Presence)
                }
                Event::DndUpdated {
                    user: Some(id),
                    dnd,
                } => {
                    crate::store::presence::set_dnd(c, &id, &dnd)?;
                    Applied::Changed(Change::Presence)
                }
                Event::ManualPresenceChanged { user: None, .. }
                | Event::DndUpdated { user: None, .. } => Applied::Ignored,
                Event::UserChanged { user } => {
                    let team = match user.team_id.clone() {
                        Some(team) => team,
                        None => c
                            .query_row(
                                "
                                    SELECT team_id
                                    FROM users
                                    WHERE id = ?1
                                ",
                                [&user.id],
                                |r| r.get::<_, String>(0),
                            )
                            .unwrap_or_default(),
                    };
                    directory::upsert_user_change(c, &team, &user)?;
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
