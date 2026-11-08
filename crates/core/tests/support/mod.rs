//! Helpers shared by the integration tests.
#![allow(dead_code)]

use wiremock::{Match, Request};

/// Matches a `multipart/form-data` request whose field `name` equals `value`
/// (Flek sends API calls the way Slack desktop does: multipart).
pub fn field(name: &str, value: impl Into<String>) -> FormField {
    FormField {
        name: name.to_owned(),
        value: Some(value.into()),
    }
}

/// Matches a multipart request that has the field `name` at all.
pub fn has_field(name: &str) -> FormField {
    FormField {
        name: name.to_owned(),
        value: None,
    }
}

pub struct FormField {
    name: String,
    value: Option<String>,
}

impl Match for FormField {
    fn matches(&self, req: &Request) -> bool {
        let body = String::from_utf8_lossy(&req.body);
        let marker = format!("name=\"{}\"\r\n\r\n", self.name);
        let Some(start) = body.find(&marker).map(|i| i + marker.len()) else {
            return false;
        };
        let end = body[start..].find("\r\n").map_or(body.len(), |i| start + i);
        self.value.as_deref().is_none_or(|v| &body[start..end] == v)
    }
}
