//! `emoji.list`.

use std::collections::HashMap;

use serde::Deserialize;

use super::Client;
use crate::Result;

impl Client {
    /// The workspace's custom emoji: `name → image URL`, or `"alias:other"`
    /// for aliases (of custom or standard emoji).
    pub async fn emoji_list(&self) -> Result<HashMap<String, String>> {
        #[derive(Deserialize)]
        struct List {
            #[serde(default)]
            emoji: HashMap<String, String>,
        }
        let l: List = self.call("emoji.list", &[]).await?;
        Ok(l.emoji)
    }
}
