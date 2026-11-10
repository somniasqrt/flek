//! Documented server-side search. Search payloads are partial history messages.
use super::{Client, Conversation, Message, User};
use crate::Result;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug)]
pub struct SearchMatch {
    pub channel: Conversation,
    pub message: Message,
    pub user: Option<User>,
}
#[derive(Debug)]
pub struct SearchPage {
    pub matches: Vec<SearchMatch>,
    pub total: usize,
    pub pages: usize,
}
#[derive(Debug, Clone, Copy)]
pub enum SearchSort {
    Score,
    Timestamp,
}
impl Client {
    /// A numbered page (1-based), at most 100 matches. Numbered paging is
    /// bounded by the caller; continuing pages observe the Tier 2 interval.
    pub async fn search_messages(
        &self,
        query: &str,
        page: usize,
        count: usize,
        sort: SearchSort,
    ) -> Result<SearchPage> {
        if page > 1 && self.pacing {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
        let page = page.clamp(1, 100).to_string();
        let count = count.clamp(1, 100).to_string();
        let sort = match sort {
            SearchSort::Score => "score",
            SearchSort::Timestamp => "timestamp",
        };
        let raw = self
            .call(
                "search.messages",
                &[
                    ("query", query),
                    ("page", &page),
                    ("count", &count),
                    ("sort", sort),
                    ("sort_dir", "desc"),
                    ("highlight", "false"),
                ],
            )
            .await?;
        #[derive(Deserialize)]
        struct Envelope {
            messages: Matches,
        }
        #[derive(Deserialize)]
        struct Matches {
            #[serde(default)]
            matches: Vec<Value>,
            total: usize,
            #[serde(default)]
            paging: Paging,
            #[serde(default)]
            pagination: Pagination,
        }
        #[derive(Default, Deserialize)]
        struct Paging {
            #[serde(default)]
            pages: usize,
        }
        #[derive(Default, Deserialize)]
        struct Pagination {
            #[serde(default)]
            page_count: usize,
        }
        let raw: Envelope = serde_json::from_value(raw)?;
        let matches = raw
            .messages
            .matches
            .into_iter()
            .map(parse_match)
            .collect::<Result<_>>()?;
        Ok(SearchPage {
            matches,
            total: raw.messages.total,
            pages: raw
                .messages
                .paging
                .pages
                .max(raw.messages.pagination.page_count)
                .max(1),
        })
    }
}

fn parse_match(v: Value) -> Result<SearchMatch> {
    let mut channel: Conversation = serde_json::from_value(v["channel"].clone())?;
    channel.raw = v["channel"].clone();
    let mut body = v.clone();
    // Search often returns only a user id/username. Save any supplied profile
    // without making a separate directory call or inventing missing fields.
    let user_value = if v["user"].is_object() {
        Some(v["user"].clone())
    } else if v["user"].is_string() && (v["user_profile"].is_object() || v["username"].is_string())
    {
        Some(serde_json::json!({
            "id": v["user"],
            "name": v["username"].as_str().unwrap_or(v["user"].as_str().unwrap()),
            "profile": v["user_profile"].as_object().cloned().unwrap_or_default(),
        }))
    } else {
        None
    };
    let user = user_value
        .map(|value| {
            let mut user: User = serde_json::from_value(value.clone())?;
            user.raw = value;
            body["user"] = Value::String(user.id.clone());
            Ok::<_, crate::Error>(user)
        })
        .transpose()?;
    let mut message: Message = serde_json::from_value(body.clone())?;
    message.raw = body;
    Ok(SearchMatch {
        channel,
        message,
        user,
    })
}
