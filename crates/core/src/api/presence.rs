//! Documented presence, custom status and Do Not Disturb methods.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Client;
use crate::{Error, Result};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserPresence {
    pub presence: String,
    pub online: Option<bool>,
    pub auto_away: Option<bool>,
    pub manual_away: Option<bool>,
    pub connection_count: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Dnd {
    pub dnd_enabled: bool,
    pub next_dnd_start_ts: i64,
    pub next_dnd_end_ts: i64,
    pub snooze_enabled: bool,
    pub snooze_endtime: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomStatus {
    pub status_text: String,
    pub status_emoji: String,
    pub status_expiration: i64,
}

#[derive(Debug, Clone, Copy)]
pub enum SetPresence {
    Auto,
    Away,
}

impl Client {
    pub async fn users_get_presence(&self, user: &str) -> Result<UserPresence> {
        Ok(serde_json::from_value(
            self.call::<Value>("users.getPresence", &[("user", user)])
                .await?,
        )?)
    }

    pub async fn users_set_presence(&self, presence: SetPresence) -> Result<()> {
        let value = match presence {
            SetPresence::Auto => "auto",
            SetPresence::Away => "away",
        };
        self.call::<Value>("users.setPresence", &[("presence", value)])
            .await?;
        Ok(())
    }

    pub async fn users_profile_set(&self, status: &CustomStatus) -> Result<Value> {
        if status.status_text.chars().count() > 100 || status.status_expiration < 0 {
            return Err(Error::Usage(
                "status must be at most 100 characters with a non-negative expiration".into(),
            ));
        }
        let profile = serde_json::to_string(status)?;
        let response = self
            .call::<Value>("users.profile.set", &[("profile", profile.as_str())])
            .await?;
        response
            .get("profile")
            .filter(|p| p.is_object())
            .cloned()
            .ok_or_else(|| Error::Store("Slack returned no updated profile".into()))
    }

    pub async fn dnd_info(&self) -> Result<Dnd> {
        Ok(serde_json::from_value(
            self.call::<Value>("dnd.info", &[]).await?,
        )?)
    }

    /// Slack limits each teamInfo request to 50 explicitly named users.
    pub async fn dnd_team_info(&self, users: &[String]) -> Result<HashMap<String, Dnd>> {
        let mut result = HashMap::new();
        for chunk in users.chunks(50) {
            let ids = chunk.join(",");
            let response = self
                .call::<Value>("dnd.teamInfo", &[("users", ids.as_str())])
                .await?;
            let rows: HashMap<String, Dnd> = serde_json::from_value(response["users"].clone())?;
            result.extend(rows.into_iter().filter(|(id, _)| chunk.contains(id)));
        }
        Ok(result)
    }

    pub async fn dnd_set_snooze(&self, minutes: u32) -> Result<Dnd> {
        if minutes == 0 {
            return Err(Error::Usage("snooze must last at least one minute".into()));
        }
        let minutes = minutes.to_string();
        Ok(serde_json::from_value(
            self.call::<Value>("dnd.setSnooze", &[("num_minutes", minutes.as_str())])
                .await?,
        )?)
    }

    pub async fn dnd_end_snooze(&self) -> Result<Dnd> {
        Ok(serde_json::from_value(
            self.call::<Value>("dnd.endSnooze", &[]).await?,
        )?)
    }
}
