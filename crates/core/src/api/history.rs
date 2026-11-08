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

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Reaction {
    pub name: String,
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub count: i64,
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
    /// For a thread root: ts of the newest reply.
    #[serde(default)]
    pub latest_reply: Option<String>,
    #[serde(default)]
    pub edited: Option<Edited>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
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
        let mut params = vec![("channel", channel), ("limit", PAGE_SIZE)];
        push_some(
            &mut params,
            &[
                ("latest", q.latest),
                ("oldest", q.oldest),
                ("cursor", q.cursor),
            ],
        );
        parse_page(self.call("conversations.history", &params).await?)
    }

    /// One page of a thread: the root first, then replies oldest first.
    pub async fn conversations_replies(
        &self,
        channel: &str,
        thread_ts: &str,
        cursor: Option<&str>,
    ) -> Result<HistoryPage> {
        let mut params = vec![
            ("channel", channel),
            ("ts", thread_ts),
            ("limit", PAGE_SIZE),
        ];
        push_some(&mut params, &[("cursor", cursor)]);
        parse_page(self.call("conversations.replies", &params).await?)
    }
}

fn push_some<'a>(params: &mut Vec<(&'a str, &'a str)>, optional: &[(&'a str, Option<&'a str>)]) {
    params.extend(optional.iter().filter_map(|&(k, v)| v.map(|v| (k, v))));
}

fn parse_page(raw: Value) -> Result<HistoryPage> {
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

    let raw: Raw = serde_json::from_value(raw)?;
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
