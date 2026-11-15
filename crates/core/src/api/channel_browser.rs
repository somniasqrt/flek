//! Documented workspace directory and explicit channel writes.
use super::{Client, Conversation};
use crate::Result;
use serde_json::Value;

pub struct ChannelDirectory {
    pub channels: Vec<Conversation>,
    pub truncated: bool,
}

fn conversation(raw: Value) -> Result<Conversation> {
    let mut channel: Conversation = serde_json::from_value(raw.clone())?;
    channel.raw = raw;
    Ok(channel)
}

impl Client {
    /// One person per explicit action; bulk invitations are intentionally unsupported.
    pub async fn conversations_invite(&self, channel: &str, user: &str) -> Result<()> {
        self.call::<Value>(
            "conversations.invite",
            &[("channel", channel), ("users", user)],
        )
        .await?;
        Ok(())
    }

    pub async fn conversations_kick(&self, channel: &str, user: &str) -> Result<()> {
        self.call::<Value>(
            "conversations.kick",
            &[("channel", channel), ("user", user)],
        )
        .await?;
        Ok(())
    }

    pub async fn conversations_set_topic(&self, channel: &str, topic: &str) -> Result<String> {
        let response: Value = self
            .call(
                "conversations.setTopic",
                &[("channel", channel), ("topic", topic)],
            )
            .await?;
        Ok(response["channel"]["topic"]["value"]
            .as_str()
            .or_else(|| response["topic"].as_str())
            .unwrap_or(topic)
            .to_owned())
    }

    pub async fn conversations_rename(&self, channel: &str, name: &str) -> Result<String> {
        let response: Value = self
            .call(
                "conversations.rename",
                &[("channel", channel), ("name", name)],
            )
            .await?;
        Ok(response["channel"]["name"]
            .as_str()
            .unwrap_or(name)
            .to_owned())
    }

    pub async fn conversations_archive(&self, channel: &str) -> Result<()> {
        self.call::<Value>("conversations.archive", &[("channel", channel)])
            .await?;
        Ok(())
    }

    /// At most 4,000 results; even empty pages can carry a next cursor.
    pub async fn conversations_list(&self, include_archived: bool) -> Result<ChannelDirectory> {
        let mut channels = Vec::new();
        let mut cursor = String::new();
        for page_number in 0..20 {
            let page: Value = self
                .call(
                    "conversations.list",
                    &[
                        ("types", "public_channel,private_channel"),
                        (
                            "exclude_archived",
                            if include_archived { "false" } else { "true" },
                        ),
                        ("limit", "200"),
                        ("cursor", &cursor),
                    ],
                )
                .await?;
            for raw in page["channels"].as_array().into_iter().flatten() {
                channels.push(conversation(raw.clone())?);
            }
            cursor = page["response_metadata"]["next_cursor"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_owned();
            if cursor.is_empty() {
                return Ok(ChannelDirectory {
                    channels,
                    truncated: false,
                });
            }
            if self.pacing && page_number < 19 {
                tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            }
        }
        Ok(ChannelDirectory {
            channels,
            truncated: true,
        })
    }

    pub async fn conversations_join(&self, id: &str) -> Result<Conversation> {
        let page: Value = self.call("conversations.join", &[("channel", id)]).await?;
        conversation(page["channel"].clone())
    }

    pub async fn conversations_create(&self, name: &str, private: bool) -> Result<Conversation> {
        let page: Value = self
            .call(
                "conversations.create",
                &[
                    ("name", name),
                    ("is_private", if private { "true" } else { "false" }),
                ],
            )
            .await?;
        conversation(page["channel"].clone())
    }

    pub async fn conversations_set_purpose(&self, id: &str, purpose: &str) -> Result<String> {
        let page: Value = self
            .call(
                "conversations.setPurpose",
                &[("channel", id), ("purpose", purpose)],
            )
            .await?;
        Ok(page["purpose"].as_str().unwrap_or(purpose).to_owned())
    }
}
