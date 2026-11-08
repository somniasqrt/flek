//! `users.conversations`.

use serde::Deserialize;
use serde_json::Value;

use super::Client;
use crate::Result;

/// Kind of conversation as stored in `conversations.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationKind {
    PublicChannel,
    PrivateChannel,
    Mpim,
    Im,
}

impl ConversationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PublicChannel => "public_channel",
            Self::PrivateChannel => "private_channel",
            Self::Mpim => "mpim",
            Self::Im => "im",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TextValue {
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Conversation {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_im: bool,
    #[serde(default)]
    pub is_mpim: bool,
    #[serde(default)]
    pub is_private: bool,
    #[serde(default)]
    pub is_group: bool,
    #[serde(default)]
    pub is_archived: bool,
    /// The other person, for an `im`.
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub topic: TextValue,
    #[serde(default)]
    pub purpose: TextValue,
    #[serde(skip)]
    pub raw: Value,
}

impl Conversation {
    pub fn kind(&self) -> ConversationKind {
        if self.is_im {
            ConversationKind::Im
        } else if self.is_mpim {
            ConversationKind::Mpim
        } else if self.is_private || self.is_group {
            ConversationKind::PrivateChannel
        } else {
            ConversationKind::PublicChannel
        }
    }
}

impl Client {
    /// Every conversation the user is in: channels, private channels, group DMs, DMs.
    pub async fn users_conversations(&self) -> Result<Vec<Conversation>> {
        let params = [
            ("types", "public_channel,private_channel,mpim,im"),
            ("exclude_archived", "false"),
        ];
        self.paginate("users.conversations", &params, "channels")
            .await?
            .into_iter()
            .map(|raw| {
                let mut conv: Conversation = serde_json::from_value(raw.clone())?;
                conv.raw = raw;
                Ok(conv)
            })
            .collect()
    }
}
