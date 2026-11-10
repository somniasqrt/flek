//! "Sign in to Slack": an embedded webview on app.slack.com (ADR 0001).
//!
//! A script on app.slack.com waits for `localConfig_v2` (written once the
//! user is signed in) and hands the tokens and `navigator.userAgent` over by
//! navigating to `https://flek.invalid/…#<json>`. That navigation is caught
//! and cancelled here, so it never leaves the machine. The `d` cookie is
//! HttpOnly, so it is read from the webview's native cookie store. The
//! window has no IPC access (no capability names it).

use flek_core::auth::{self, OriginKind, SessionOrigin};
use flek_core::Secret;
use serde::Deserialize;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::state::{self, AppState, Status};

pub const LABEL: &str = "signin";
const SLACK_URL: &str = "https://app.slack.com/";
const DONE_HOST: &str = "flek.invalid";

const CAPTURE_SCRIPT: &str = r#"
(function () {
  if (location.origin !== "https://app.slack.com") return;
  function tick() {
    try {
      var raw = localStorage.getItem("localConfig_v2");
      var cfg = raw ? JSON.parse(raw) : null;
      var teams = cfg && cfg.teams ? Object.keys(cfg.teams).map(function (k) {
        var t = cfg.teams[k];
        return { id: t.id || k, name: t.name || "", token: t.token };
      }).filter(function (t) {
        return typeof t.token === "string" && t.token.indexOf("xoxc-") === 0;
      }) : [];
      if (teams.length) {
        var payload = JSON.stringify({ teams: teams, user_agent: navigator.userAgent });
        location.href = "https://flek.invalid/signed-in#" + encodeURIComponent(payload);
        return;
      }
    } catch (e) {}
    setTimeout(tick, 1000);
  }
  tick();
})();
"#;

#[derive(Deserialize)]
struct Captured {
    teams: Vec<CapturedTeam>,
    user_agent: String,
}

#[derive(Deserialize)]
struct CapturedTeam {
    id: String,
    name: String,
    token: String,
}

/// Opens (or focuses) the sign-in window.
pub fn open(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(LABEL) {
        w.show()?;
        return w.set_focus();
    }
    let handle = app.clone();
    let url: Url = SLACK_URL.parse().expect("valid URL");
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
        .title(crate::i18n::t(
            app.state::<AppState>().settings().language,
            crate::i18n::Key::SignIn,
        ))
        .inner_size(520.0, 760.0)
        .incognito(true)
        .initialization_script(CAPTURE_SCRIPT)
        .on_navigation(move |url| {
            if url.host_str() != Some(DONE_HOST) {
                return true;
            }
            // Read the source window, not a substring of the destination URL.
            let current = handle.get_webview_window(LABEL).and_then(|w| w.url().ok());
            if !completion_allowed(current.as_ref(), url) {
                return false;
            }
            let fragment = url.fragment().unwrap_or_default();
            if fragment.len() > 256 * 1024 {
                return false;
            }
            let decoded = percent_encoding::percent_decode_str(fragment).decode_utf8_lossy();
            let captured = serde_json::from_str::<Captured>(&decoded).ok();
            let app = handle.clone();
            tauri::async_runtime::spawn(async move { finish(app, captured).await });
            false
        });
    if let Some(ua) = auth::webview::sign_in_user_agent() {
        builder = builder.user_agent(&ua);
    }
    builder.build()?;
    state::set_status(app, Status::SigningIn);
    Ok(())
}

/// The window was closed by hand before signing in finished.
pub fn closed(app: &AppHandle) {
    if app.state::<AppState>().status() == Status::SigningIn {
        state::set_status(
            app,
            Status::SignIn {
                desktop: false,
                reason: None,
            },
        );
    }
}

async fn finish(app: AppHandle, captured: Option<Captured>) {
    let state = app.state::<AppState>();
    // Sign-out cannot race a pending auth.test and let it save credentials later.
    let _maintenance = state.maintenance.lock().await;
    let result = async {
        let mut captured =
            captured.ok_or_else(|| "the sign-in page sent nothing usable".to_owned())?;
        captured.user_agent = bounded_page_text(&captured.user_agent, 512);
        if captured.teams.is_empty() || captured.teams.len() > 64 {
            return Err("invalid sign-in workspace count".to_owned());
        }
        let window = app
            .get_webview_window(LABEL)
            .ok_or_else(|| "the sign-in window is gone".to_owned())?;
        let cookie = session_cookie(&window)?;
        let origin = SessionOrigin::new(OriginKind::Webview, captured.user_agent);
        let creds = &app.state::<AppState>().creds;

        let mut first = None;
        let mut last_err = None;
        for mut team in captured.teams {
            team.id = bounded_page_text(&team.id, 128);
            team.name = bounded_page_text(&team.name, 256);
            match auth::validate(
                Secret::new(team.token),
                cookie.clone(),
                Some(origin.clone()),
            )
            .await
            {
                Ok(session) => {
                    creds
                        .save(&session.credentials)
                        .map_err(|e| e.to_string())?;
                    first.get_or_insert(session);
                }
                Err(e) => {
                    // Do not log metadata supplied by a page (it may contain secrets).
                    tracing::warn!("sign-in validation failed");
                    last_err = Some(e.to_string());
                }
            }
        }
        first.ok_or_else(|| last_err.unwrap_or_else(|| "no workspace signed in".into()))
    }
    .await;

    // Status first, then close: closing reports an abandoned sign-in.
    match result {
        Ok(session) => state::start(&app, session),
        Err(reason) => state::set_status(
            &app,
            Status::SignIn {
                desktop: false,
                reason: Some(reason),
            },
        ),
    }
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
    crate::show_main(&app);
}

/// The HttpOnly `d` cookie from the webview's cookie store.
fn session_cookie(window: &WebviewWindow) -> Result<Secret, String> {
    let url: Url = SLACK_URL.parse().expect("valid URL");
    window
        .cookies_for_url(url)
        .map_err(|e| format!("cannot read the webview cookies: {e}"))?
        .into_iter()
        .find(|c| c.name() == "d" && c.value().starts_with("xoxd-"))
        .map(|c| Secret::new(c.value().to_owned()))
        .ok_or_else(|| "signed in, but the session cookie is missing".to_owned())
}

/// Only the actual HTTPS Slack app page can hand off a captured session.
fn completion_allowed(current: Option<&Url>, destination: &Url) -> bool {
    current.is_some_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("app.slack.com")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
    }) && destination.scheme() == "https"
        && destination.host_str() == Some(DONE_HOST)
        && destination.port_or_known_default() == Some(443)
        && destination.username().is_empty()
        && destination.password().is_none()
        && destination.path() == "/signed-in"
}

/// Page metadata is display/header input only, never a path or command.
fn bounded_page_text(value: &str, max_bytes: usize) -> String {
    let mut result = String::new();
    for c in value.chars().filter(|c| !c.is_control()) {
        if result.len() + c.len_utf8() > max_bytes {
            break;
        }
        result.push(c);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_requires_the_exact_slack_source_origin() {
        let done = Url::parse("https://flek.invalid/signed-in#fake").unwrap();
        for source in [
            "https://app.slack.com/client/T",
            "https://app.slack.com:443/",
        ] {
            assert!(completion_allowed(
                Some(&Url::parse(source).unwrap()),
                &done
            ));
        }
        for source in [
            "https://app.slack.com.evil.test",
            "http://app.slack.com",
            "https://evil.test/?u=app.slack.com",
            "https://app.slack.com@evil.test",
            "https://evil@app.slack.com",
            "https://app.slack.com:8443/",
        ] {
            assert!(
                !completion_allowed(Some(&Url::parse(source).unwrap()), &done),
                "{source}"
            );
        }
        assert!(!completion_allowed(None, &done));
        let slack = Url::parse(SLACK_URL).unwrap();
        for target in [
            "http://flek.invalid/signed-in",
            "https://flek.invalid/other",
            "https://flek.invalid.evil.test/signed-in",
            "https://evil@flek.invalid/signed-in",
        ] {
            assert!(!completion_allowed(
                Some(&slack),
                &Url::parse(target).unwrap()
            ));
        }
    }

    #[test]
    fn untrusted_page_metadata_is_bounded_and_has_no_controls() {
        assert_eq!(bounded_page_text("team\r\n\0\tname", 256), "teamname");
        assert_eq!(bounded_page_text(&"я".repeat(300), 511).len(), 510);
        assert_eq!(bounded_page_text(&"a".repeat(1024), 512).len(), 512);
    }
}
