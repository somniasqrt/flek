//! `conversations.history`.

use serde::Deserialize;
use serde_json::Value;

use super::Client;
use crate::Result;

pub(super) fn null_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

const PAGE_SIZE: &str = "200";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Edited {
    #[serde(default)]
    pub ts: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Reaction {
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
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
    #[serde(default, deserialize_with = "null_default")]
    pub text: String,
    #[serde(default)]
    pub blocks: Option<Value>,
    #[serde(default)]
    pub attachments: Option<Value>,
    #[serde(default, deserialize_with = "null_default")]
    pub files: Vec<Value>,
    #[serde(default)]
    pub reply_count: i64,
    /// For a thread root: ts of the newest reply.
    #[serde(default)]
    pub latest_reply: Option<String>,
    #[serde(default)]
    pub edited: Option<Edited>,
    #[serde(default, deserialize_with = "null_default")]
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
            && self.subtype.as_deref().is_none_or(carries_user_content);
        file_hidden || empty
    }
}

/// Subtypes whose messages always carry what a person (or bot) wrote, so an
/// empty version of one is content-stripped, not legitimately empty. Other
/// subtypes (`tombstone`, `huddle_thread`, system notices…) may be empty for
/// real and are taken as they come.
fn carries_user_content(subtype: &str) -> bool {
    matches!(
        subtype,
        "thread_broadcast" | "reply_broadcast" | "bot_message" | "me_message" | "file_share"
    )
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
    // Until deletion reconciliation understands incomplete pages, a decode
    // failure must abort the page rather than look like a missing message.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn null_stubs_preserve_content_and_malformed_pages_fail_closed() {
        let page = parse_page(json!({"messages": [
            {"ts": "1.0", "text": null, "files": null, "reactions": null},
            {"ts": "2.0", "text": "valid", "files": null, "reactions": null}
        ], "has_more": true, "response_metadata": {"next_cursor": "next"}}))
        .unwrap();
        assert_eq!(page.messages.len(), 2);
        assert!(page.has_more);
        assert_eq!(page.next_cursor.as_deref(), Some("next"));
        assert!(page.messages[0].is_hidden_by_limit());
        assert!(!page.messages[1].is_hidden_by_limit());
        assert!(parse_page(json!({"messages":[{"ts":"3.0","text":42}]})).is_err());
        let store = crate::store::Store::open_in_memory().unwrap();
        store.with_conn(|c| {
            c.execute_batch("INSERT INTO teams(id,name) VALUES('T1','Fake');
                INSERT INTO conversations(id,team_id,kind) VALUES('C1','T1','public_channel');")?;
            let full: Message = serde_json::from_value(json!({"ts":"1.0", "text":"saved", "reactions":[{"name":"wave","users":["U1"]}],"files":[{"id":"F1","name":"saved.txt"}]}))?;
            crate::store::messages::upsert_messages(c,"C1", &[crate::sync::backfill::to_row(&full)])?;
            crate::store::messages::upsert_messages(c,"C1", &[crate::sync::backfill::to_row(&page.messages[0])])?;
            assert_eq!(crate::store::messages::recent(c,"C1",10)?[0].text,"saved");
            let count: i64 = c.query_row("SELECT COUNT(*) FROM reactions",[],|r| r.get(0))?;
            assert_eq!(count,1);
            let files: i64 = c.query_row("SELECT COUNT(*) FROM message_files",[],|r| r.get(0))?;
            assert_eq!(files,1);
            Ok(())
        }).unwrap();
    }

    fn hidden(v: Value) -> bool {
        serde_json::from_value::<Message>(v)
            .unwrap()
            .is_hidden_by_limit()
    }

    #[test]
    fn empty_content_subtypes_are_hidden_but_tombstones_are_not() {
        assert!(hidden(json!({"ts": "1.0", "text": ""})));
        assert!(hidden(
            json!({"ts": "1.0", "text": "", "subtype": "thread_broadcast"})
        ));
        assert!(hidden(
            json!({"ts": "1.0", "text": " ", "subtype": "bot_message"})
        ));
        assert!(!hidden(
            json!({"ts": "1.0", "text": "", "subtype": "tombstone"})
        ));
        assert!(!hidden(
            json!({"ts": "1.0", "text": "", "subtype": "huddle_thread"})
        ));
        assert!(!hidden(
            json!({"ts": "1.0", "text": "hi", "subtype": "thread_broadcast"})
        ));
    }
}
