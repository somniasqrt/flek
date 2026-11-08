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
        .title("Sign in to Slack")
        .inner_size(520.0, 760.0)
        .initialization_script(CAPTURE_SCRIPT)
        .on_navigation(move |url| {
            if url.host_str() != Some(DONE_HOST) {
                return true;
            }
            let fragment = url.fragment().unwrap_or_default();
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
    let result = async {
        let captured = captured.ok_or_else(|| "the sign-in page sent nothing usable".to_owned())?;
        let window = app
            .get_webview_window(LABEL)
            .ok_or_else(|| "the sign-in window is gone".to_owned())?;
        let cookie = session_cookie(&window)?;
        let origin = SessionOrigin::new(OriginKind::Webview, captured.user_agent);
        let creds = &app.state::<AppState>().creds;

        let mut first = None;
        let mut last_err = None;
        for team in captured.teams {
            match auth::validate(Secret::new(team.token), cookie.clone(), Some(origin.clone()))
                .await
            {
                Ok(session) => {
                    creds.save(&session.credentials).map_err(|e| e.to_string())?;
                    first.get_or_insert(session);
                }
                Err(e) => {
                    tracing::warn!(team = %team.id, name = %team.name, "sign-in validation failed: {e}");
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
