//! HTTP plumbing: auth headers, `ok`/`error` envelope, rate limits.

use std::time::Duration;

use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, COOKIE, RETRY_AFTER, USER_AGENT,
};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::ClientProfile;
use crate::{Error, Result, Secret};

pub const DEFAULT_BASE_URL: &str = "https://slack.com/api";

const MAX_RETRIES: u32 = 3;
const MAX_RETRY_WAIT: Duration = Duration::from_secs(120);

/// Talks to Slack as the user: `xoxc` token plus the `d` cookie.
pub struct Client {
    http: reqwest::Client,
    file_http: reqwest::Client,
    base_url: String,
    token: Secret,
    cookie: Secret,
    /// Wait between pages of list methods to stay inside their tier.
    pub(crate) pacing: bool,
    profile: ClientProfile,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.base_url)
            .field("token", &self.token)
            .field("cookie", &self.cookie)
            .finish()
    }
}

impl Client {
    pub fn new(token: Secret, cookie: Secret) -> Result<Self> {
        Self::with_base_url(token, cookie, DEFAULT_BASE_URL)
    }

    /// Same as [`Client::new`] with another API root (tests point it at a mock server).
    pub fn with_base_url(token: Secret, cookie: Secret, base_url: &str) -> Result<Self> {
        Self::with_profile(token, cookie, base_url, ClientProfile::detect())
    }

    /// A client presenting `profile` (the session issuer's identity).
    pub fn with_profile(
        token: Secret,
        cookie: Secret,
        base_url: &str,
        profile: ClientProfile,
    ) -> Result<Self> {
        let base =
            reqwest::Url::parse(base_url).map_err(|_| Error::File("invalid API URL".into()))?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::custom({
                let base = base.clone();
                move |attempt| {
                    if attempt.previous().len() > 5 {
                        attempt.error("too many redirects")
                    } else if api_redirect_allowed(attempt.url(), &base) {
                        attempt.follow()
                    } else {
                        attempt.stop()
                    }
                }
            }))
            .default_headers(session_headers(&cookie, &profile)?)
            .timeout(Duration::from_secs(30))
            .build()?;
        let file_http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() > 5 {
                    attempt.error("too many redirects")
                } else if crate::net_policy::slack_file(attempt.url())
                    || crate::net_policy::same_origin(attempt.url(), &base)
                {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .default_headers(session_headers(&cookie, &profile)?)
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            file_http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            token,
            cookie,
            pacing: true,
            profile,
        })
    }

    /// `(name, value)` headers for the websocket handshake: the same cookie and
    /// User-Agent as HTTP calls, plus the web client's Origin.
    pub(crate) fn ws_headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("cookie", format!("d={}", self.cookie.expose())),
            ("user-agent", self.profile.user_agent.clone()),
            ("origin", self.profile.origin.clone()),
        ]
    }

    pub(crate) fn validate_ws_url(&self, url: &str) -> Result<()> {
        let url =
            reqwest::Url::parse(url).map_err(|_| Error::WebSocket("invalid RTM URL".into()))?;
        let base = reqwest::Url::parse(&self.base_url)
            .map_err(|_| Error::WebSocket("invalid API URL".into()))?;
        if !crate::net_policy::websocket(&url, &base) {
            return Err(Error::WebSocket(
                "refusing session for untrusted RTM URL".into(),
            ));
        }
        Ok(())
    }

    pub fn profile(&self) -> &ClientProfile {
        &self.profile
    }

    /// No waiting between pages (tests against a mock server).
    pub fn without_pacing(mut self) -> Self {
        self.pacing = false;
        self
    }

    /// Calls `method` with form params and deserializes the successful body.
    /// Retries on HTTP 429 (honouring `Retry-After`) and on `ratelimited`.
    ///
    /// Shaped like Slack desktop's calls (docs/api-notes.md): multipart body
    /// with the token in it, no `Authorization` header.
    pub async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: &[(&str, &str)],
    ) -> Result<T> {
        let url = format!("{}/{}", self.base_url, method);
        let form = || {
            params.iter().fold(
                reqwest::multipart::Form::new().text("token", self.token.expose().to_owned()),
                |f, (k, v)| f.text(k.to_string(), v.to_string()),
            )
        };

        let mut attempt = 0;
        loop {
            let mut req = self.http.post(&url).multipart(form());
            for (name, value) in self.profile.api_headers() {
                req = req.header(name, value);
            }
            let resp = req.send().await?;
            let status = resp.status();

            if status == StatusCode::TOO_MANY_REQUESTS {
                let wait = retry_after(resp.headers());
                if attempt >= MAX_RETRIES {
                    return Err(Error::Slack {
                        code: "ratelimited".into(),
                        needed: None,
                    });
                }
                attempt += 1;
                tracing::warn!(
                    method,
                    wait_secs = wait.as_secs(),
                    attempt,
                    "rate limited, retrying"
                );
                tokio::time::sleep(wait).await;
                continue;
            }
            if !status.is_success() {
                return Err(Error::HttpStatus(status.as_u16()));
            }

            let headers = resp.headers().clone();
            let body: Value = resp.json().await?;
            if body.get("ok").and_then(Value::as_bool) == Some(true) {
                return Ok(serde_json::from_value(body)?);
            }

            let code = body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown_error")
                .to_owned();
            if code == "ratelimited" && attempt < MAX_RETRIES {
                attempt += 1;
                let wait = retry_after(&headers);
                tracing::warn!(
                    method,
                    wait_secs = wait.as_secs(),
                    attempt,
                    "ratelimited, retrying"
                );
                tokio::time::sleep(wait).await;
                continue;
            }
            let needed = body
                .get("needed")
                .and_then(Value::as_str)
                .map(str::to_owned);
            tracing::debug!(method, %code, "Slack returned an error");
            return Err(Error::Slack { code, needed });
        }
    }
}

impl Client {
    /// GETs a Slack-hosted file with the session headers into `dest`
    /// (written to a temp file first, then renamed). Refuses hosts outside
    /// Slack so the session never leaks, and HTML pages unless `html_ok`
    /// (Slack answers a broken session with its login page). Uses the default
    /// file cap; stored-file callers pass current settings to download_bounded.
    pub async fn download(&self, url: &str, dest: &std::path::Path, html_ok: bool) -> Result<u64> {
        let limit = crate::settings::Settings::default()
            .max_file_bytes
            .and_then(|n| u64::try_from(n).ok());
        self.download_bounded(url, dest, html_ok, limit).await
    }

    /// Stream bound chosen from user settings and untrusted declared metadata.
    pub async fn download_bounded(
        &self,
        url: &str,
        dest: &std::path::Path,
        html_ok: bool,
        limit: Option<u64>,
    ) -> Result<u64> {
        let parsed =
            reqwest::Url::parse(url).map_err(|_| Error::File("invalid file URL".into()))?;
        let base = reqwest::Url::parse(&self.base_url)
            .map_err(|_| Error::File("invalid API URL".into()))?;
        if !(crate::net_policy::slack_file(&parsed)
            || crate::net_policy::same_origin(&parsed, &base))
        {
            return Err(Error::File(
                "refusing session for untrusted file URL".into(),
            ));
        }

        let mut attempt = 0;
        let resp = loop {
            let resp = self
                .file_http
                .get(parsed.clone())
                .header(AUTHORIZATION, bearer(&self.token)?)
                .timeout(Duration::from_secs(600))
                .send()
                .await?;
            if resp.status() != StatusCode::TOO_MANY_REQUESTS || attempt >= MAX_RETRIES {
                break resp;
            }
            attempt += 1;
            let wait = retry_after(resp.headers());
            tracing::warn!(
                wait_secs = wait.as_secs(),
                attempt,
                "file download rate limited, retrying"
            );
            tokio::time::sleep(wait).await;
        };
        if !resp.status().is_success() {
            return Err(Error::HttpStatus(resp.status().as_u16()));
        }
        let is_html = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/html"));
        if is_html && !html_ok {
            return Err(Error::File(
                "got an HTML page instead of the file (session expired?)".into(),
            ));
        }

        let written = crate::files::write_response(resp, dest, limit).await?;
        if let Err(error) = crate::files::quarantine(dest) {
            let _ = std::fs::remove_file(dest);
            return Err(error);
        }
        Ok(written)
    }
}

fn invalid(what: &str) -> Error {
    Error::DesktopData(format!(
        "{what} contains characters not allowed in a header"
    ))
}

/// `Authorization: Bearer` for file downloads only. Slack desktop sends none
/// at all; whether files load with the cookie alone is not verified yet.
fn bearer(token: &Secret) -> Result<HeaderValue> {
    let mut v = HeaderValue::from_str(&format!("Bearer {}", token.expose()))
        .map_err(|_| invalid("token"))?;
    v.set_sensitive(true);
    Ok(v)
}

/// Headers on every request: the session cookie and the issuer's identity.
fn session_headers(cookie: &Secret, profile: &ClientProfile) -> Result<HeaderMap> {
    let mut cookie =
        HeaderValue::from_str(&format!("d={}", cookie.expose())).map_err(|_| invalid("cookie"))?;
    cookie.set_sensitive(true);

    let mut headers = HeaderMap::new();
    headers.insert(COOKIE, cookie);
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&profile.user_agent).map_err(|_| invalid("user agent"))?,
    );
    // Chromium sends its client hints on every request, file downloads too.
    for (name, value) in profile.hint_headers() {
        headers.insert(
            name,
            HeaderValue::from_str(&value).map_err(|_| invalid("client hint"))?,
        );
    }
    Ok(headers)
}

fn retry_after(headers: &HeaderMap) -> Duration {
    headers
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(1))
        .min(MAX_RETRY_WAIT)
}

fn api_redirect_allowed(url: &reqwest::Url, base: &reqwest::Url) -> bool {
    crate::net_policy::same_origin(url, base)
}

#[cfg(test)]
mod redirect_tests {
    use super::*;

    #[test]
    fn api_redirects_require_the_exact_base_origin() {
        let base = reqwest::Url::parse("https://workspace.slack.com/api").unwrap();
        for url in [
            "https://files.slack.com/file",
            "https://other.slack.com/api",
            "https://a.slack-edge.com/file",
            "https://a.slack-files.com/file",
            "https://workspace.slack.com:8443/api",
            "http://workspace.slack.com/api",
            "https://user@workspace.slack.com/api",
        ] {
            assert!(
                !api_redirect_allowed(&reqwest::Url::parse(url).unwrap(), &base),
                "{url}"
            );
        }
        assert!(api_redirect_allowed(
            &reqwest::Url::parse("https://workspace.slack.com:443/api/next").unwrap(),
            &base
        ));
    }
}
