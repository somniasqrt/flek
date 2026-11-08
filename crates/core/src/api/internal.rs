//! Undocumented endpoints the Slack web client uses. Each one is described in
//! `docs/api-notes.md`, and every caller must cope with it failing.

use serde::Deserialize;

use super::Client;
use crate::Result;

/// Read marker of one conversation, from `client.counts`.
#[derive(Debug, Clone, Deserialize)]
pub struct ReadMarker {
    pub id: String,
    #[serde(default)]
    pub last_read: Option<String>,
    #[serde(default)]
    pub mention_count: i64,
    #[serde(default)]
    pub has_unreads: bool,
}

impl Client {
    /// `client.counts`: read markers of every conversation in one call
    /// (instead of one `conversations.info` per conversation).
    pub async fn client_counts(&self) -> Result<Vec<ReadMarker>> {
        #[derive(Deserialize)]
        struct Counts {
            #[serde(default)]
            channels: Vec<ReadMarker>,
            #[serde(default)]
            mpims: Vec<ReadMarker>,
            #[serde(default)]
            ims: Vec<ReadMarker>,
        }
        let c: Counts = self.call("client.counts", &[]).await?;
        Ok(c.channels.into_iter().chain(c.mpims).chain(c.ims).collect())
    }
}
