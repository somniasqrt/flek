//! RTM events we act on. Parsing is manual (not a serde tag) because
//! `message` events fork on `subtype` and we keep the raw JSON.

use serde_json::Value;

use crate::api::{Message, User};

#[derive(Debug, Clone)]
pub enum Event {
    PresenceChanged {
        users: Vec<String>,
        presence: String,
    },
    ManualPresenceChanged {
        user: Option<String>,
        presence: String,
    },
    DndUpdated {
        user: Option<String>,
        dnd: crate::api::Dnd,
    },
    Hello,
    /// Server is about to close; reconnect.
    Goodbye,
    Pong {
        reply_to: u64,
    },
    /// New message: plain, bot, file share, `thread_broadcast`, …
    Message {
        channel: String,
        message: Message,
    },
    /// `message_changed` (edit, unfurl) or `message_replied` (thread root
    /// with new reply counts): the full new version of an existing message.
    MessageChanged {
        channel: String,
        message: Message,
    },
    MessageDeleted {
        channel: String,
        ts: String,
    },
    ReactionAdded {
        event_ts: Option<String>,
        channel: String,
        ts: String,
        name: String,
        user: String,
    },
    ReactionRemoved {
        channel: String,
        ts: String,
        name: String,
        user: String,
    },
    /// Read marker moved (`channel_marked`, `im_marked`, …).
    Marked {
        channel: String,
        ts: String,
    },
    /// Thread read marker moved on some client (`thread_marked`).
    ThreadMarked {
        channel: String,
        thread_ts: String,
        ts: String,
    },
    Typing {
        channel: String,
        user: String,
    },
    /// A conversation was created, renamed, archived, joined, left…
    ConversationsChanged {
        kind: String,
    },
    UserChanged {
        user: Box<User>,
    },
    /// Anything else; logged at debug level.
    Unknown {
        kind: String,
    },
}

impl Event {
    pub fn parse(v: Value) -> Self {
        let kind = str_at(&v, "type").unwrap_or_default();
        let channel = || str_at(&v, "channel").unwrap_or_default();
        match kind.as_str() {
            "presence_change" => {
                let users = v
                    .get("users")
                    .and_then(Value::as_array)
                    .map(|ids| {
                        ids.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_else(|| str_at(&v, "user").into_iter().collect());
                Event::PresenceChanged {
                    users,
                    presence: str_at(&v, "presence").unwrap_or_default(),
                }
            }
            "manual_presence_change" => Event::ManualPresenceChanged {
                user: str_at(&v, "user"),
                presence: str_at(&v, "presence").unwrap_or_default(),
            },
            "dnd_updated" | "dnd_updated_user" => match v
                .get("dnd_status")
                .filter(|d| d.is_object())
                .and_then(|d| serde_json::from_value(d.clone()).ok())
            {
                Some(dnd) if kind == "dnd_updated" || str_at(&v, "user").is_some() => {
                    Event::DndUpdated {
                        user: str_at(&v, "user"),
                        dnd,
                    }
                }
                _ => Event::Unknown { kind },
            },
            "hello" => Event::Hello,
            "goodbye" => Event::Goodbye,
            "pong" => Event::Pong {
                reply_to: v.get("reply_to").and_then(Value::as_u64).unwrap_or(0),
            },
            "message" => parse_message(&v, channel()).unwrap_or(Event::Unknown { kind }),
            "reaction_added" | "reaction_removed" => {
                let item = v.get("item");
                let (Some(ch), Some(ts)) = (
                    item.and_then(|i| str_at(i, "channel")),
                    item.and_then(|i| str_at(i, "ts")),
                ) else {
                    // Reactions on files and file comments are not stored.
                    return Event::Unknown { kind };
                };
                let name = str_at(&v, "reaction").unwrap_or_default();
                let user = str_at(&v, "user").unwrap_or_default();
                if kind == "reaction_added" {
                    Event::ReactionAdded {
                        event_ts: str_at(&v, "event_ts"),
                        channel: ch,
                        ts,
                        name,
                        user,
                    }
                } else {
                    Event::ReactionRemoved {
                        channel: ch,
                        ts,
                        name,
                        user,
                    }
                }
            }
            "channel_marked" | "group_marked" | "im_marked" | "mpim_marked" => Event::Marked {
                channel: channel(),
                ts: str_at(&v, "ts").unwrap_or_default(),
            },
            // Undocumented; see docs/api-notes.md. Anything unexpected is ignored.
            "thread_marked" => {
                let sub = v.get("subscription");
                let field = |key| sub.and_then(|s| str_at(s, key));
                match (field("channel"), field("thread_ts"), field("last_read")) {
                    (Some(channel), Some(thread_ts), Some(ts)) => Event::ThreadMarked {
                        channel,
                        thread_ts,
                        ts,
                    },
                    _ => Event::Unknown { kind },
                }
            }
            "user_typing" => Event::Typing {
                channel: channel(),
                user: str_at(&v, "user").unwrap_or_default(),
            },
            "user_change" => match v
                .get("user")
                .cloned()
                .map(|u| (serde_json::from_value::<User>(u.clone()), u))
            {
                Some((Ok(mut user), raw)) => {
                    user.raw = raw;
                    Event::UserChanged {
                        user: Box::new(user),
                    }
                }
                _ => Event::Unknown { kind },
            },
            k if is_conversation_change(k) => Event::ConversationsChanged { kind },
            _ => Event::Unknown { kind },
        }
    }
}

fn parse_message(v: &Value, channel: String) -> Option<Event> {
    match str_at(v, "subtype").as_deref() {
        Some("message_changed") | Some("message_replied") => Some(Event::MessageChanged {
            channel,
            message: message_from(v.get("message")?.clone())?,
        }),
        Some("message_deleted") => Some(Event::MessageDeleted {
            channel,
            ts: str_at(v, "deleted_ts")?,
        }),
        _ => Some(Event::Message {
            channel,
            message: message_from(v.clone())?,
        }),
    }
}

fn message_from(raw: Value) -> Option<Message> {
    let mut m: Message = serde_json::from_value(raw.clone()).ok()?;
    m.raw = raw;
    Some(m)
}

fn is_conversation_change(kind: &str) -> bool {
    matches!(
        kind,
        "channel_created"
            | "channel_deleted"
            | "channel_rename"
            | "channel_archive"
            | "channel_unarchive"
            | "channel_joined"
            | "channel_left"
            | "group_joined"
            | "group_left"
            | "group_rename"
            | "group_archive"
            | "group_unarchive"
            | "im_created"
            | "mpim_joined"
            | "member_joined_channel"
            | "member_left_channel"
    )
}

fn str_at(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: Value) -> Event {
        Event::parse(v)
    }

    #[test]
    fn messages_and_subtypes() {
        match parse(
            json!({"type": "message", "channel": "C1", "ts": "1.0", "user": "U1", "text": "hi"}),
        ) {
            Event::Message { channel, message } => {
                assert_eq!((channel.as_str(), message.text.as_str()), ("C1", "hi"));
                assert_eq!(message.raw["text"], "hi");
            }
            e => panic!("{e:?}"),
        }
        match parse(
            json!({"type": "message", "subtype": "thread_broadcast", "channel": "C1", "ts": "2.0", "thread_ts": "1.0", "text": "b"}),
        ) {
            Event::Message { message, .. } => {
                assert_eq!(message.subtype.as_deref(), Some("thread_broadcast"))
            }
            e => panic!("{e:?}"),
        }
        match parse(
            json!({"type": "message", "subtype": "message_changed", "channel": "C1", "ts": "9.0",
                           "message": {"ts": "1.0", "text": "edited", "edited": {"ts": "9.0"}}, "previous_message": {"ts": "1.0", "text": "old"}}),
        ) {
            Event::MessageChanged { channel, message } => assert_eq!(
                (channel.as_str(), message.ts.as_str(), message.text.as_str()),
                ("C1", "1.0", "edited")
            ),
            e => panic!("{e:?}"),
        }
        match parse(
            json!({"type": "message", "subtype": "message_deleted", "channel": "C1", "ts": "9.0", "deleted_ts": "1.0"}),
        ) {
            Event::MessageDeleted { ts, .. } => assert_eq!(ts, "1.0"),
            e => panic!("{e:?}"),
        }
        match parse(
            json!({"type": "message", "subtype": "message_replied", "channel": "C1", "message": {"ts": "1.0", "reply_count": 3, "latest_reply": "5.0"}}),
        ) {
            Event::MessageChanged { message, .. } => assert_eq!(message.reply_count, 3),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn reactions_marks_and_others() {
        assert!(matches!(
            parse(json!({"type": "reaction_added", "user": "U2", "reaction": "eyes", "item": {"type": "message", "channel": "C1", "ts": "1.0"}})),
            Event::ReactionAdded { ref name, ref ts, .. } if name == "eyes" && ts == "1.0"
        ));
        assert!(matches!(
            parse(
                json!({"type": "reaction_removed", "user": "U2", "reaction": "eyes", "item": {"type": "file", "file": "F1"}})
            ),
            Event::Unknown { .. }
        ));
        assert!(
            matches!(parse(json!({"type": "im_marked", "channel": "D1", "ts": "3.0"})), Event::Marked { ref ts, .. } if ts == "3.0")
        );
        assert!(matches!(
            parse(json!({"type": "thread_marked", "subscription": {"type": "thread", "channel": "C1", "thread_ts": "1.0", "last_read": "4.0", "unread_count": 0}})),
            Event::ThreadMarked { ref thread_ts, ref ts, .. } if thread_ts == "1.0" && ts == "4.0"
        ));
        assert!(matches!(
            parse(json!({"type": "thread_marked", "subscription": {"channel": "C1"}})),
            Event::Unknown { .. }
        ));
        assert!(matches!(
            parse(json!({"type": "channel_rename", "channel": {"id": "C1", "name": "new"}})),
            Event::ConversationsChanged { .. }
        ));
        assert!(matches!(
            parse(
                json!({"type": "user_change", "user": {"id": "U1", "name": "a", "profile": {"display_name": "A"}}})
            ),
            Event::UserChanged { .. }
        ));
        assert!(matches!(
            parse(json!({"type": "pong", "reply_to": 7})),
            Event::Pong { reply_to: 7 }
        ));
        assert!(
            matches!(parse(json!({"type": "future_unknown"})), Event::Unknown { ref kind } if kind == "future_unknown")
        );
        assert!(matches!(parse(json!({"type": "goodbye"})), Event::Goodbye));
    }
}
