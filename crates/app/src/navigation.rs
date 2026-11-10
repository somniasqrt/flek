//! Main is an app surface; external destinations only reach the OS opener.
use tauri::{webview::NewWindowResponse, AppHandle, Url, WebviewWindowBuilder};

pub fn create_main(app: &AppHandle) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == crate::MAIN)
        .expect("main window config");
    let dev_url = app.config().build.dev_url.clone();
    let handle = app.clone();
    WebviewWindowBuilder::from_config(app, config)?
        .on_navigation(move |url| {
            main_navigation_allowed(url, dev_url.as_ref(), cfg!(debug_assertions))
        })
        .on_new_window(move |url, _| {
            if popup_opens_externally(&url) {
                let _ = crate::commands::open_url(handle.clone(), url.to_string());
            }
            NewWindowResponse::Deny
        })
        .build()?;
    Ok(())
}

fn popup_opens_externally(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

fn main_navigation_allowed(url: &Url, dev: Option<&Url>, debug: bool) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let own =
        url.host_str() == Some("localhost") && url.scheme() == "tauri" && url.port().is_none()
            || url.host_str() == Some("tauri.localhost")
                && matches!(url.scheme(), "http" | "https")
                && url.port().is_none();
    own || debug && dev.is_some_and(|dev| same_origin(url, dev))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn main_only_navigates_within_app_origins() {
        let dev = Url::parse("http://localhost:5173").unwrap();
        for url in [
            "tauri://localhost/",
            "tauri://localhost/index.html?a=1#x",
            "http://tauri.localhost/",
            "https://tauri.localhost/a",
        ] {
            assert!(
                main_navigation_allowed(&Url::parse(url).unwrap(), Some(&dev), false),
                "{url}"
            );
        }
        for url in [
            "https://evil.test",
            "file:///tmp/x",
            "javascript:alert(1)",
            "data:text/html,hi",
            "flekfile://localhost/F1",
            "http://flekfile.localhost/F1",
            "tauri://localhost.evil.test/",
            "https://tauri.localhost:444/",
            "http://user@tauri.localhost/",
            "http://localhost:5174/",
        ] {
            let url = Url::parse(url).unwrap();
            for debug in [false, true] {
                assert!(!main_navigation_allowed(&url, Some(&dev), debug), "{url}");
            }
        }
        let path = Url::parse("http://localhost:5173/index.html#x").unwrap();
        assert!(main_navigation_allowed(&path, Some(&dev), true));
        assert!(!main_navigation_allowed(&path, Some(&dev), false));
        assert!(!main_navigation_allowed(&path, None, true));
    }

    #[test]
    fn popup_requests_only_open_web_urls_externally() {
        for url in ["https://example.test/", "http://example.test/"] {
            assert!(popup_opens_externally(&Url::parse(url).unwrap()));
        }
        for url in [
            "mailto:user@example.test",
            "javascript:alert(1)",
            "file:///tmp/a",
            "data:text/html,hi",
            "flekfile://localhost/F",
            "tauri://localhost/",
        ] {
            assert!(!popup_opens_externally(&Url::parse(url).unwrap()));
        }
    }

    #[test]
    fn config_keeps_native_window_guards_and_restrictive_csp() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["app"]["windows"][0]["create"], false);
        let csp = config["app"]["security"]["csp"].as_str().unwrap();
        for directive in [
            "object-src 'none'",
            "base-uri 'none'",
            "form-action 'none'",
            "frame-src 'none'",
        ] {
            assert!(csp.split(';').any(|d| d.trim() == directive));
        }
    }
}
