//! Cursor pagination shared by list methods.

use serde::Deserialize;
use serde_json::Value;

use super::Client;
use crate::Result;

const PAGE_SIZE: &str = "200";

#[derive(Deserialize)]
struct Meta {
    #[serde(default)]
    next_cursor: String,
}

impl Client {
    /// Calls `method` page by page and collects the array under `key`.
    /// Items stay as raw JSON so callers can keep `raw_json`.
    pub async fn paginate(
        &self,
        method: &str,
        params: &[(&str, &str)],
        key: &str,
    ) -> Result<Vec<Value>> {
        let mut items = Vec::new();
        let mut cursor = String::new();
        loop {
            let mut p: Vec<(&str, &str)> = params.to_vec();
            p.push(("limit", PAGE_SIZE));
            if !cursor.is_empty() {
                p.push(("cursor", &cursor));
            }
            let mut page: Value = self.call(method, &p).await?;
            if let Some(Value::Array(batch)) = page.get_mut(key).map(Value::take) {
                items.extend(batch);
            }
            let next = page
                .get("response_metadata")
                .cloned()
                .and_then(|m| serde_json::from_value::<Meta>(m).ok())
                .map(|m| m.next_cursor)
                .unwrap_or_default();
            if next.is_empty() {
                return Ok(items);
            }
            tracing::debug!(method, fetched = items.len(), "next page");
            cursor = next;
        }
    }
}
