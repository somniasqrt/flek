//! The client identity Flek presents to Slack (HTTP and websocket alike).
//!
//! Nothing is hard-coded per platform: the OS part comes from the machine
//! Flek runs on, and the browser version from today's date. An xoxc + d
//! session is the web client's, so the profile is the Slack web client
//! (Origin app.slack.com) in a current desktop Chrome on this OS.
//!
//! Next step (ROADMAP M2.6.5): present exactly the client that issued the
//! session, e.g. the installed Slack desktop's own user agent.

use std::time::{SystemTime, UNIX_EPOCH};

/// Chrome 131 reached stable on 2024-11-12; a new major ships every 4 weeks.
const CHROME_ANCHOR_MAJOR: u64 = 131;
const CHROME_ANCHOR_UNIX: u64 = 1_731_369_600;
const CHROME_CYCLE_SECS: u64 = 28 * 86_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientProfile {
    pub user_agent: String,
    pub origin: String,
}

impl ClientProfile {
    /// Current desktop Chrome on the OS Flek runs on.
    pub fn detect() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Self::chrome(platform_token(), chrome_major_at(now))
    }

    /// Chrome's reduced user agent: only the major version, frozen tokens.
    pub fn chrome(platform: &str, major: u64) -> Self {
        Self {
            user_agent: format!(
                "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36"
            ),
            origin: "https://app.slack.com".to_owned(),
        }
    }
}

/// The platform part Chrome sends; frozen by Chrome itself (macOS always
/// reads "Intel Mac OS X 10_15_7", Windows 11 reads "Windows NT 10.0").
pub fn platform_token() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows NT 10.0; Win64; x64"
    } else if cfg!(target_os = "linux") {
        "X11; Linux x86_64"
    } else {
        "Macintosh; Intel Mac OS X 10_15_7"
    }
}

/// Stable Chrome major version at a unix time (±1 around release days).
pub fn chrome_major_at(unix_secs: u64) -> u64 {
    CHROME_ANCHOR_MAJOR + unix_secs.saturating_sub(CHROME_ANCHOR_UNIX) / CHROME_CYCLE_SECS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_follows_the_calendar() {
        assert_eq!(chrome_major_at(CHROME_ANCHOR_UNIX), 131);
        // 2025-11-08 → 143, 2026-11-08 → 156.
        assert_eq!(chrome_major_at(1_762_560_000), 143);
        assert_eq!(chrome_major_at(1_794_096_000), 156);
        assert_eq!(chrome_major_at(0), 131);
    }

    #[test]
    fn user_agent_shape() {
        let p = ClientProfile::chrome("Windows NT 10.0; Win64; x64", 156);
        assert_eq!(
            p.user_agent,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36"
        );
        assert!(ClientProfile::detect()
            .user_agent
            .contains(platform_token()));
    }
}
