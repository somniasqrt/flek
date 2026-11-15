//! Documented message-action endpoints that have no generic write path.
//! Permalinks live in `write.rs`; pins and stars go through `WriteRequest`.
use super::Client;
use crate::Result;
use serde::Deserialize;

impl Client {
    /// Open (or fetch) the direct message with one person so a forward can land there.
    pub async fn open_forward_dm(&self, user: &str) -> Result<super::Conversation> {
        #[derive(Deserialize)]
        struct Response {
            channel: serde_json::Value,
        }
        let response: Response = self
            .call(
                "conversations.open",
                &[("users", user), ("return_im", "true")],
            )
            .await?;
        let mut channel: super::Conversation = serde_json::from_value(response.channel.clone())?;
        if !channel.id.starts_with('D')
            || !channel.id.bytes().all(|b| b.is_ascii_alphanumeric())
            || channel.user.as_deref().is_some_and(|id| id != user)
        {
            return Err(crate::Error::Store(
                "unexpected direct message returned by Slack".into(),
            ));
        }
        channel.raw = response.channel;
        Ok(channel)
    }
}
