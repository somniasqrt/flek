//! Who issued a session (ADR 0001). Stored next to the credentials, so every
//! request can present exactly that client.

use serde::{Deserialize, Serialize};

use crate::api::ClientProfile;

/// The client that issued a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OriginKind {
    /// Imported from Slack desktop on this machine.
    Desktop {
        app: String,
        electron: String,
        chrome: String,
    },
    /// Signed in through Flek's own webview.
    Webview,
    /// Pasted by hand; the issuer is unknown.
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOrigin {
    #[serde(flatten)]
    pub kind: OriginKind,
    pub user_agent: String,
    /// OS the session was obtained on (`std::env::consts::OS`).
    pub platform: String,
    /// Unix seconds.
    pub obtained_at: i64,
}

impl SessionOrigin {
    pub fn new(kind: OriginKind, user_agent: impl Into<String>) -> Self {
        Self {
            kind,
            user_agent: user_agent.into(),
            platform: std::env::consts::OS.to_owned(),
            obtained_at: now(),
        }
    }

    /// The identity requests made with this session present.
    pub fn profile(&self) -> ClientProfile {
        ClientProfile::from_user_agent(&self.user_agent)
    }

    /// Short human description, e.g. "Slack desktop 4.51.180".
    pub fn describe(&self) -> String {
        match &self.kind {
            OriginKind::Desktop { app, .. } => format!("Slack desktop {app}"),
            OriginKind::Webview => "Flek sign-in window".to_owned(),
            OriginKind::Manual => "manual token".to_owned(),
        }
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_flat() {
        let o = SessionOrigin {
            kind: OriginKind::Desktop {
                app: "4.51.180".into(),
                electron: "43.1.1".into(),
                chrome: "150.0.7871.114".into(),
            },
            user_agent: "UA".into(),
            platform: "macos".into(),
            obtained_at: 1,
        };
        let json = serde_json::to_value(&o).unwrap();
        assert_eq!(json["kind"], "desktop");
        assert_eq!(json["app"], "4.51.180");
        assert_eq!(serde_json::from_value::<SessionOrigin>(json).unwrap(), o);

        let w: SessionOrigin = serde_json::from_str(
            r#"{"kind":"webview","user_agent":"UA","platform":"macos","obtained_at":2}"#,
        )
        .unwrap();
        assert_eq!(w.kind, OriginKind::Webview);
    }
}
