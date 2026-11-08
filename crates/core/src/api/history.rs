//! `conversations.history`.

use serde::Deserialize;
use serde_json::Value;

use super::Client;
use crate::Result;

const PAGE_SIZE: &str = "200";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Edited {
    #[serde(default)]
    pub ts: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub ts: String,
    #[serde(default)]
    pub thread_ts: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub bot_id: Option<String>,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub blocks: Option<Value>,
    #[serde(default)]
    pub attachments: Option<Value>,
    #[serde(default)]
    pub files: Vec<Value>,
    #[serde(default)]
    pub reply_count: i64,
    #[serde(default)]
    pub edited: Option<Edited>,
    #[serde(skip)]
    pub raw: Value,
}

impl Message {
    /// The Free plan returns old messages stripped of content; their files
    /// carry `mode: hidden_by_limit`.
    pub fn is_hidden_by_limit(&self) -> bool {
        let file_hidden = self
            .files
            .iter()
            .any(|f| f.get("mode").and_then(Value::as_str) == Some("hidden_by_limit"));
        let non_empty = |v: &Option<Value>| {
            v.as_ref()
                .and_then(Value::as_array)
                .is_some_and(|a| !a.is_empty())
        };
        let empty = self.text.trim().is_empty()
            && !non_empty(&self.blocks)
            && !non_empty(&self.attachments)
            && self.files.is_empty()
            && self.subtype.is_none();
        file_hidden || empty
    }
}

#[derive(Debug, Clone, Default)]
pub struct HistoryPage {
    /// Newest first, as Slack returns them.
    pub messages: Vec<Message>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// Window and page position for [`Client::conversations_history`].
/// `latest` and `oldest` are exclusive.
#[derive(Debug, Clone, Copy, Default)]
pub struct HistoryQuery<'a> {
    pub latest: Option<&'a str>,
    pub oldest: Option<&'a str>,
    pub cursor: Option<&'a str>,
}

impl Client {
    /// One page of a conversation's history, newest first.
    pub async fn conversations_history(
        &self,
        channel: &str,
        q: HistoryQuery<'_>,
    ) -> Result<HistoryPage> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            messages: Vec<Value>,
            #[serde(default)]
            has_more: bool,
            #[serde(default)]
            response_metadata: Option<Meta>,
        }
        #[derive(Deserialize)]
        struct Meta {
            #[serde(default)]
            next_cursor: String,
        }

        let mut params = vec![("channel", channel), ("limit", PAGE_SIZE)];
        for (k, v) in [
            ("latest", q.latest),
            ("oldest", q.oldest),
            ("cursor", q.cursor),
        ] {
            if let Some(v) = v {
                params.push((k, v));
            }
        }
        let raw: Raw = self.call("conversations.history", &params).await?;
        let messages = raw
            .messages
            .into_iter()
            .map(|v| {
                let mut m: Message = serde_json::from_value(v.clone())?;
                m.raw = v;
                Ok(m)
            })
            .collect::<Result<_>>()?;
        Ok(HistoryPage {
            messages,
            has_more: raw.has_more,
            next_cursor: raw
                .response_metadata
                .map(|m| m.next_cursor)
                .filter(|c| !c.is_empty()),
        })
    }
}
