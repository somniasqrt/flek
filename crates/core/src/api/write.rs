//! One documented write request, prepared without credentials for exact dry runs.
use super::Client;
use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Serialize)]
pub struct WriteRequest {
    pub method: String,
    /// Form fields before URL encoding. Authentication is added only by Client.
    pub params: BTreeMap<String, String>,
}
impl WriteRequest {
    pub(crate) fn new(method: &str, params: &[(&str, &str)]) -> Self {
        Self {
            method: method.into(),
            params: params
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
        }
    }
    pub async fn execute(&self, client: &Client) -> Result<Value> {
        if !matches!(
            self.method.as_str(),
            "chat.postMessage"
                | "chat.scheduleMessage"
                | "conversations.open"
                | "conversations.mark"
                | "reactions.add"
                | "reactions.remove"
                | "pins.add"
                | "pins.remove"
                | "stars.add"
                | "stars.remove"
        ) || self.params.keys().any(|k| {
            matches!(
                k.to_ascii_lowercase().as_str(),
                "token" | "cookie" | "authorization"
            )
        }) {
            return Err(Error::Forbidden);
        }
        let params: Vec<_> = self
            .params
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        match client.call(&self.method, &params).await {
            // These documented writes are idempotent in the requested state.
            Err(Error::Slack { code, .. })
                if matches!(
                    (self.method.as_str(), code.as_str()),
                    ("reactions.add", "already_reacted")
                        | ("reactions.remove", "no_reaction")
                        | ("pins.add", "already_pinned")
                        | ("pins.remove", "no_pin")
                        | ("stars.add", "already_starred")
                        | ("stars.remove", "not_starred" | "no_star")
                ) =>
            {
                Ok(serde_json::json!({"ok":true}))
            }
            result => result,
        }
    }
}

impl Client {
    pub async fn chat_get_permalink(&self, channel: &str, ts: &str) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Response {
            permalink: String,
        }
        let response: Response = self
            .call(
                "chat.getPermalink",
                &[("channel", channel), ("message_ts", ts)],
            )
            .await?;
        Ok(response.permalink)
    }
}
