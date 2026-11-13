//! `users.list`.

use serde::Deserialize;
use serde_json::Value;

use super::{history::null_default, Client};
use crate::Result;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Profile {
    #[serde(default, deserialize_with = "null_default")]
    pub display_name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub real_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: String,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default)]
    pub real_name: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub profile: Profile,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub deleted: bool,
    #[serde(skip)]
    pub raw: Value,
}

impl User {
    /// What Slack shows: display name, then real name, then handle.
    pub fn label(&self) -> &str {
        [
            self.profile.display_name.as_str(),
            self.profile.real_name.as_str(),
            self.real_name.as_deref().unwrap_or(""),
        ]
        .into_iter()
        .find(|s| !s.trim().is_empty())
        .unwrap_or(&self.name)
    }
}

impl Client {
    /// Every user in the workspace, including deleted ones and bots.
    pub async fn users_list(&self) -> Result<Vec<User>> {
        let raw = self.paginate("users.list", &[], "members").await?;
        let mut users = Vec::new();
        let mut skipped = 0usize;
        for raw in raw {
            match serde_json::from_value::<User>(raw.clone()) {
                Ok(mut user) => {
                    user.raw = raw;
                    users.push(user);
                }
                Err(_) => skipped += 1,
            }
        }
        if skipped > 0 {
            tracing::warn!(skipped, "skipped unparseable users.list items");
        }
        Ok(users)
    }
}
