//! `chat.postMessage`: sending as the user.

use serde::Deserialize;
use serde_json::Value;

use super::{Client, Message};
use crate::{Error, Result};

/// A message to send. `text` is Slack mrkdwn or a plain fallback for `blocks`;
/// [`Client::chat_post_message`] escapes `&`, `<` and `>` itself.
#[derive(Debug, Clone, Default)]
pub struct PostMessage {
    pub channel: String,
    pub text: String,
    /// Optional structured rich text from the app composer.
    pub blocks: Option<Vec<Value>>,
    /// Root ts: reply in that thread.
    pub thread_ts: Option<String>,
    /// A thread reply also shown in the channel ("Also send to #channel").
    pub reply_broadcast: bool,
    /// Client-chosen id, echoed back by Slack in the stored message and the
    /// live event, so an optimistic copy can be matched to the real one.
    pub client_msg_id: String,
}

/// What Slack answered: the stored message as it will appear in history.
#[derive(Debug, Clone)]
pub struct Posted {
    pub channel: String,
    pub ts: String,
    pub message: Message,
}

impl Client {
    /// Posts a message as the user. Errors come back as [`Error::Slack`]
    /// with Slack's code (`channel_not_found`, `not_in_channel`, …);
    /// `ratelimited` is retried by [`Client::call`].
    pub async fn chat_post_message(&self, m: &PostMessage) -> Result<Posted> {
        if m.text.trim().is_empty() {
            return Err(Error::Slack {
                code: "no_text".into(),
                needed: None,
            });
        }
        let text = escape_text(&m.text);
        let mut params = vec![
            ("channel", m.channel.as_str()),
            ("text", text.as_str()),
            ("client_msg_id", m.client_msg_id.as_str()),
        ];
        let blocks = m.blocks.as_ref().map(serde_json::to_string).transpose()?;
        if let Some(blocks) = blocks.as_deref() {
            params.push(("blocks", blocks));
        }
        if let Some(t) = m.thread_ts.as_deref() {
            params.push(("thread_ts", t));
            if m.reply_broadcast {
                params.push(("reply_broadcast", "true"));
            }
        }

        #[derive(Deserialize)]
        struct Raw {
            channel: String,
            ts: String,
            #[serde(default)]
            message: Option<Value>,
        }
        let raw: Raw = self.call("chat.postMessage", &params).await?;

        // Slack returns the stored message; fill in what it may leave out so
        // the row written now matches the live event that follows.
        let mut v = raw.message.unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = v.as_object_mut() {
            obj.entry("ts").or_insert_with(|| raw.ts.clone().into());
            obj.entry("type").or_insert_with(|| "message".into());
            obj.entry("text").or_insert_with(|| text.clone().into());
            if let Some(blocks) = &m.blocks {
                obj.entry("blocks")
                    .or_insert_with(|| Value::Array(blocks.clone()));
            }
            obj.entry("client_msg_id")
                .or_insert_with(|| m.client_msg_id.clone().into());
            if let Some(t) = &m.thread_ts {
                obj.entry("thread_ts").or_insert_with(|| t.clone().into());
                if m.reply_broadcast {
                    obj.entry("subtype")
                        .or_insert_with(|| "thread_broadcast".into());
                }
            }
        }
        let mut message: Message = serde_json::from_value(v.clone())?;
        message.raw = v;
        Ok(Posted {
            channel: raw.channel,
            ts: raw.ts,
            message,
        })
    }
}

/// Escapes the three characters Slack's message format reserves.
/// <https://api.slack.com/reference/surfaces/formatting#escaping>
pub fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

/// A fresh `client_msg_id`: a random UUID (version 4), as Slack's own
/// clients send.
pub fn new_client_msg_id() -> String {
    let mut b: [u8; 16] = std::array::from_fn(|_| fastrand::u8(..));
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// A sentence for the user about why a send failed. Slack's code stays in
/// it for the cases without a better explanation.
pub fn describe_send_error(e: &Error) -> String {
    match e {
        Error::Slack { code, .. } => match code.as_str() {
            "channel_not_found" => {
                "Slack does not know this conversation (deleted, or no access)".into()
            }
            "not_in_channel" => "You are not a member of this channel".into(),
            "is_archived" => "This channel is archived".into(),
            "msg_too_long" => "The message is too long".into(),
            "no_text" => "The message is empty".into(),
            "restricted_action"
            | "restricted_action_read_only_channel"
            | "restricted_action_thread_only_channel"
            | "restricted_action_non_threadable_channel" => {
                "Posting here is restricted by the workspace".into()
            }
            "thread_not_found" => "The thread no longer exists".into(),
            "ratelimited" => "Slack is rate limiting; try again in a moment".into(),
            "invalid_auth" | "not_authed" | "token_revoked" | "account_inactive" => {
                "The Slack session expired; sign in again".into()
            }
            other => format!("Slack refused the message ({other})"),
        },
        e if e.is_unreachable() => "Slack is unreachable; check the connection".into(),
        e => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_reserved_characters_only() {
        assert_eq!(
            escape_text("a < b && c > d *bold* <@U1>"),
            "a &lt; b &amp;&amp; c &gt; d *bold* &lt;@U1&gt;"
        );
        assert_eq!(escape_text("привіт :wave:"), "привіт :wave:");
    }

    #[test]
    fn client_msg_ids_look_like_uuids() {
        let a = new_client_msg_id();
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert_ne!(a, new_client_msg_id());
    }
}
