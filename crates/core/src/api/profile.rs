//! The client identity Flek presents to Slack (HTTP and websocket alike).
//!
//! Everything derives from one user agent: the one of the client that issued
//! the session (ADR 0001), recorded in [`crate::auth::SessionOrigin`]. Client
//! hints follow from it the way Chromium computes them, so nothing here is
//! hard-coded per platform. Without a recorded origin, [`ClientProfile::detect`]
//! builds a current desktop Chrome for the OS Flek runs on.

use std::time::{SystemTime, UNIX_EPOCH};

/// Chrome 131 reached stable on 2024-11-12; a new major ships every 4 weeks.
const CHROME_ANCHOR_MAJOR: u64 = 131;
const CHROME_ANCHOR_UNIX: u64 = 1_731_369_600;
const CHROME_CYCLE_SECS: u64 = 28 * 86_400;

/// The Slack web client's origin; xoxc sessions belong to it.
pub const WEB_ORIGIN: &str = "https://app.slack.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientProfile {
    pub user_agent: String,
    pub origin: String,
    /// `sec-ch-ua*` values; `None` for clients that send none (WebKit, Firefox).
    pub hints: Option<ClientHints>,
}

/// Low-entropy client hints, sent by Chromium on every HTTPS request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientHints {
    /// Value of `sec-ch-ua`, e.g. `"Not;A=Brand";v="8", "Chromium";v="150"`.
    pub brands: String,
    /// Value of `sec-ch-ua-mobile`: `?0` or `?1`.
    pub mobile: &'static str,
    /// Value of `sec-ch-ua-platform`, quoted, e.g. `"macOS"`.
    pub platform: String,
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
        Self::from_user_agent(format!(
            "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36"
        ))
    }

    /// The profile of whatever client sends `user_agent`.
    pub fn from_user_agent(user_agent: impl Into<String>) -> Self {
        let user_agent = user_agent.into();
        let hints = ClientHints::for_user_agent(&user_agent);
        Self {
            user_agent,
            origin: WEB_ORIGIN.to_owned(),
            hints,
        }
    }

    /// Client hints as `(name, value)` header pairs (empty without hints).
    pub fn hint_headers(&self) -> Vec<(&'static str, String)> {
        match &self.hints {
            Some(h) => vec![
                ("sec-ch-ua", h.brands.clone()),
                ("sec-ch-ua-mobile", h.mobile.to_owned()),
                ("sec-ch-ua-platform", h.platform.clone()),
            ],
            None => Vec::new(),
        }
    }

    /// Headers the web client's `fetch()` to the API carries besides the user
    /// agent and client hints: its Origin and fetch metadata (app.slack.com
    /// calling slack.com is same-site). Every engine that sends `Origin` on a
    /// cross-origin POST also sends `sec-fetch-*` (Chromium, WebKit 16.4+).
    pub fn api_headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("origin", self.origin.clone()),
            ("sec-fetch-site", "same-site".to_owned()),
            ("sec-fetch-mode", "cors".to_owned()),
            ("sec-fetch-dest", "empty".to_owned()),
        ]
    }
}

impl ClientHints {
    /// Hints a Chromium-based client with this user agent sends, or `None`
    /// when the user agent is not Chromium's.
    pub fn for_user_agent(ua: &str) -> Option<Self> {
        let major: u64 = token_version(ua, "Chrome/")?
            .split('.')
            .next()?
            .parse()
            .ok()?;
        // Plain Chromium builds (Electron included) carry no product brand.
        let brand = if ua.contains(" Edg/") {
            Some("Microsoft Edge")
        } else if ua.contains(" OPR/") {
            Some("Opera")
        } else if ua.contains(" Electron/") {
            None
        } else {
            Some("Google Chrome")
        };
        let platform = if ua.contains("Android") {
            "Android"
        } else if ua.contains("CrOS") {
            "Chrome OS"
        } else if ua.contains("Macintosh") {
            "macOS"
        } else if ua.contains("Windows") {
            "Windows"
        } else if ua.contains("Linux") {
            "Linux"
        } else {
            "Unknown"
        };
        let mobile = if ua.contains("Mobile") { "?1" } else { "?0" };
        Some(Self {
            brands: brand_list(major, brand),
            mobile,
            platform: format!("\"{platform}\""),
        })
    }
}

/// Chromium's `sec-ch-ua`: a GREASE brand, "Chromium" and the product brand,
/// in an order and spelling seeded by the major version
/// (`components/embedder_support/user_agent_utils.cc`).
pub fn brand_list(major: u64, brand: Option<&str>) -> String {
    const GREASE_CHARS: [&str; 11] = [" ", "(", ":", "-", ".", "/", ")", ";", "=", "?", "_"];
    const GREASE_VERSIONS: [&str; 3] = ["8", "99", "24"];
    const ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let seed = usize::try_from(major).unwrap_or(0);
    let grease = (
        format!(
            "Not{}A{}Brand",
            GREASE_CHARS[seed % 11],
            GREASE_CHARS[(seed + 1) % 11]
        ),
        GREASE_VERSIONS[seed % 3].to_owned(),
    );
    let chromium = ("Chromium".to_owned(), major.to_string());

    let list = match brand {
        Some(b) => {
            let order = ORDERS[seed % 6];
            let mut list = vec![(String::new(), String::new()); 3];
            list[order[0]] = grease;
            list[order[1]] = chromium;
            list[order[2]] = (b.to_owned(), major.to_string());
            list
        }
        None => {
            let mut list = vec![(String::new(), String::new()); 2];
            list[seed % 2] = grease;
            list[(seed + 1) % 2] = chromium;
            list
        }
    };
    list.iter()
        .map(|(b, v)| format!("\"{b}\";v=\"{v}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The version after `prefix` in a user agent (`Chrome/` → `150.0.7871.114`).
pub fn token_version<'a>(ua: &'a str, prefix: &str) -> Option<&'a str> {
    let start = ua.find(prefix)? + prefix.len();
    let rest = &ua[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
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
        assert_eq!(p.hints.unwrap().platform, "\"Windows\"");
        assert!(ClientProfile::detect()
            .user_agent
            .contains(platform_token()));
    }

    #[test]
    fn brands_match_real_chrome() {
        // Values Chrome actually sent in these versions.
        assert_eq!(
            brand_list(120, Some("Google Chrome")),
            r#""Not_A Brand";v="8", "Chromium";v="120", "Google Chrome";v="120""#
        );
        assert_eq!(
            brand_list(124, Some("Google Chrome")),
            r#""Chromium";v="124", "Google Chrome";v="124", "Not-A.Brand";v="99""#
        );
        assert_eq!(
            brand_list(131, Some("Google Chrome")),
            r#""Google Chrome";v="131", "Chromium";v="131", "Not_A Brand";v="24""#
        );
    }

    #[test]
    fn electron_hints_have_no_product_brand() {
        let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 27_0_1) AppleWebKit/537.36 (KHTML, like Gecko) Slack/4.51.180 Chrome/150.0.7871.114 Electron/43.1.1 Safari/537.36 AppleSilicon Sonic Slack_SSB/4.51.180";
        let h = ClientHints::for_user_agent(ua).unwrap();
        assert_eq!(h.brands, r#""Not;A=Brand";v="8", "Chromium";v="150""#);
        assert_eq!(h.platform, "\"macOS\"");
        assert_eq!(h.mobile, "?0");
        assert_eq!(token_version(ua, "Electron/"), Some("43.1.1"));
    }

    #[test]
    fn webkit_sends_no_hints() {
        let safari = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
        let p = ClientProfile::from_user_agent(safari);
        assert!(p.hints.is_none());
        assert!(p.hint_headers().is_empty());
        assert_eq!(p.api_headers()[0], ("origin", WEB_ORIGIN.to_owned()));
    }
}
