//! The system webview Flek signs in with (ADR 0001, webview login).

/// User agent for the sign-in webview. WKWebView's default lacks Safari's
/// `Version/x Safari/y` tokens and Slack treats it as an unsupported
/// browser, so on macOS it presents the Safari it is part of (version read
/// from Safari.app at runtime). `None` elsewhere: the webview's own user
/// agent is already a browser's (WebView2 is Edge).
pub fn sign_in_user_agent() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let version = super::desktop_profile::plist_string(
        std::path::Path::new("/Applications/Safari.app/Contents/Info.plist"),
        "CFBundleShortVersionString",
    )?;
    Some(safari_user_agent(&version))
}

/// Safari's user agent: WebKit froze the OS and engine tokens.
pub fn safari_user_agent(version: &str) -> String {
    format!(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/{version} Safari/605.1.15"
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn safari_sends_no_client_hints() {
        let ua = super::safari_user_agent("26.1");
        assert!(ua.ends_with("Version/26.1 Safari/605.1.15"));
        assert!(crate::api::ClientProfile::from_user_agent(ua)
            .hints
            .is_none());
    }
}
