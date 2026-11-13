//! Cursor pagination shared by list methods.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::Client;
use crate::{Error, Result};

const PAGE_SIZE: &str = "200";

#[derive(Deserialize)]
struct Meta {
    #[serde(default)]
    next_cursor: String,
}

/// Spacing between pages so a long listing stays inside the method's tier:
/// Tier 2 (`users.list`, ~20/min) → 3 s, Tier 3 (~50/min) → 1.2 s.
fn page_interval(method: &str) -> Duration {
    match method {
        "users.list" => Duration::from_secs(3),
        _ => Duration::from_millis(1200),
    }
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
            // Membership reconciliation requires a complete, well-formed list;
            // a malformed success envelope must never look like an empty list.
            if method == "users.conversations"
                && (!page.get(key).is_some_and(Value::is_array)
                    || page.get("response_metadata").is_some_and(|meta| {
                        !meta.is_object() || meta.get("next_cursor").is_some_and(|c| !c.is_string())
                    }))
            {
                return Err(Error::Store("invalid users.conversations page".into()));
            }
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
            if self.pacing {
                tokio::time::sleep(page_interval(method)).await;
            }
            cursor = next;
        }
    }
}
