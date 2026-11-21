//! Documented conversation and message-action endpoints that have no generic write path.
//! Permalinks live in `write.rs`; pins and stars go through `WriteRequest`.
use super::Client;
use crate::Result;
use serde::Deserialize;
use serde_json::Value;

impl Client {
    /// Open (or fetch) a 1:1 direct message or group DM with 1 to 8 people.
    pub async fn open_conversation(&self, users: &[String]) -> Result<super::Conversation> {
        if users.is_empty() || users.len() > 8 {
            return Err(crate::Error::Usage(
                "a direct message takes 1 to 8 people".into(),
            ));
        }
        #[derive(Deserialize)]
        struct Response {
            channel: Value,
        }
        let joined = users.join(",");
        let response: Response = self
            .call(
                "conversations.open",
                &[("users", joined.as_str()), ("return_im", "true")],
            )
            .await?;
        let mut raw = response.channel;
        let Some(obj) = raw.as_object_mut() else {
            return Err(crate::Error::Store(
                "unexpected direct message returned by Slack".into(),
            ));
        };
        let mut channel: super::Conversation = serde_json::from_value(Value::Object(obj.clone()))?;
        if channel.id.is_empty() || !channel.id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(crate::Error::Store(
                "unexpected direct message returned by Slack".into(),
            ));
        }
        if users.len() == 1 {
            if !channel.id.starts_with('D')
                || channel.is_mpim
                || channel.user.as_deref().is_some_and(|id| id != users[0])
            {
                return Err(crate::Error::Store(
                    "unexpected direct message returned by Slack".into(),
                ));
            }
            if !channel.is_im {
                channel.is_im = true;
                obj.insert("is_im".into(), Value::Bool(true));
            }
            if channel.user.is_none() {
                channel.user = Some(users[0].clone());
                obj.insert("user".into(), users[0].clone().into());
            }
        } else {
            if !(channel.id.starts_with('G') || channel.id.starts_with('C')) || channel.is_im {
                return Err(crate::Error::Store(
                    "unexpected direct message returned by Slack".into(),
                ));
            }
            if !channel.is_mpim {
                channel.is_mpim = true;
                obj.insert("is_mpim".into(), Value::Bool(true));
            }
        }
        channel.raw = raw;
        Ok(channel)
    }
}
