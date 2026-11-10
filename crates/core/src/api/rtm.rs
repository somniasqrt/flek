//! `rtm.connect`: a one-time websocket URL for live events.

use serde::Deserialize;

use super::Client;
use crate::Result;

#[derive(Debug, Clone, Deserialize)]
pub struct RtmConnect {
    /// `wss://…`, valid for a short time and only once.
    pub url: String,
}

impl Client {
    pub async fn rtm_connect(&self) -> Result<RtmConnect> {
        self.call("rtm.connect", &[("batch_presence_aware", "1")])
            .await
    }
}
