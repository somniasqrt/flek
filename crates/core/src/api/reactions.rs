//! Documented message reaction methods.
use super::Client;
use crate::{Error, Result};

impl Client {
    /// Sets the calling user's reaction. Already in the requested state is success.
    pub async fn set_message_reaction(
        &self,
        channel: &str,
        ts: &str,
        name: &str,
        on: bool,
    ) -> Result<()> {
        let method = if on {
            "reactions.add"
        } else {
            "reactions.remove"
        };
        let result = self
            .call::<serde_json::Value>(
                method,
                &[("channel", channel), ("timestamp", ts), ("name", name)],
            )
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(Error::Slack { code, .. })
                if (on && code == "already_reacted") || (!on && code == "no_reaction") =>
            {
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}
