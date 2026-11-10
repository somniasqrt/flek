//! Documented workspace metadata, fetched by directory sync.
use super::Client;
use crate::Result;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Info {
    team: Value,
}

impl Client {
    pub async fn team_info(&self) -> Result<Value> {
        Ok(self.call::<Info>("team.info", &[]).await?.team)
    }
}

pub fn icon_url(team: &Value) -> Option<&str> {
    let icon = &team["icon"];
    if icon["image_default"].as_bool() == Some(true) {
        return None;
    }
    ["image_132", "image_88"]
        .into_iter()
        .find_map(|key| icon[key].as_str().filter(|url| url.starts_with("https://")))
}
