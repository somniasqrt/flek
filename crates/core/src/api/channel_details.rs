//! Documented conversation membership and pins endpoints.
use super::Client;
use crate::Result;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct Pin {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub message: Option<super::Message>,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub created: Option<i64>,
}
impl Client {
    pub async fn conversations_members(&self, channel: &str) -> Result<Vec<String>> {
        self.paginate("conversations.members", &[("channel", channel)], "members")
            .await?
            .into_iter()
            .map(|v| Ok(serde_json::from_value(v)?))
            .collect()
    }
    pub async fn pins_list(&self, channel: &str) -> Result<Vec<Pin>> {
        #[derive(Deserialize)]
        struct Response {
            items: Vec<Value>,
        }
        let response: Response = self.call("pins.list", &[("channel", channel)]).await?;
        response
            .items
            .into_iter()
            .map(|mut raw| {
                let message = raw.get("message").cloned();
                let mut pin: Pin = serde_json::from_value(raw.take())?;
                if let Some(m) = pin.message.as_mut() {
                    m.raw = message.unwrap_or_default();
                }
                Ok(pin)
            })
            .collect()
    }
}
