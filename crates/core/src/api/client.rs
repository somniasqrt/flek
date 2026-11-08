//! HTTP plumbing: auth headers, `ok`/`error` envelope, rate limits.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, COOKIE, RETRY_AFTER, USER_AGENT};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{Error, Result, Secret};

pub const DEFAULT_BASE_URL: &str = "https://slack.com/api";

const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
const MAX_RETRIES: u32 = 3;
const MAX_RETRY_WAIT: Duration = Duration::from_secs(120);

/// Talks to Slack as the user: `xoxc` token plus the `d` cookie.
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    token: Secret,
    cookie: Secret,
    /// Wait between pages of list methods to stay inside their tier.
    pub(crate) pacing: bool,
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
        let http = reqwest::Client::builder()
            .default_headers(auth_headers(&token, &cookie)?)
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            token,
            cookie,
            pacing: true,
        })
    }

    /// `(name, value)` headers for the websocket handshake: the same cookie and
    /// User-Agent as HTTP calls, plus the web client's Origin.
    pub(crate) fn ws_headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("cookie", format!("d={}", self.cookie.expose())),
            ("user-agent", DESKTOP_USER_AGENT.to_owned()),
            ("origin", "https://app.slack.com".to_owned()),
        ]
    }

    /// No waiting between pages (tests against a mock server).
    pub fn without_pacing(mut self) -> Self {
        self.pacing = false;
        self
    }

    /// Calls `method` with form params and deserializes the successful body.
    /// Retries on HTTP 429 (honouring `Retry-After`) and on `ratelimited`.
    pub async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: &[(&str, &str)],
    ) -> Result<T> {
        let url = format!("{}/{}", self.base_url, method);
        let mut form: Vec<(&str, &str)> = Vec::with_capacity(params.len() + 1);
        form.push(("token", self.token.expose()));
        form.extend_from_slice(params);

        let mut attempt = 0;
        loop {
            let resp = self.http.post(&url).form(&form).send().await?;
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
    /// (Slack answers a broken session with its login page).
    pub async fn download(&self, url: &str, dest: &std::path::Path, html_ok: bool) -> Result<u64> {
        let parsed =
            reqwest::Url::parse(url).map_err(|_| Error::File("invalid file URL".into()))?;
        let host = parsed.host_str().unwrap_or_default();
        let api_host = reqwest::Url::parse(&self.base_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default();
        if !(host == "slack.com" || host.ends_with(".slack.com") || host == api_host) {
            return Err(Error::File(format!(
                "refusing to send the session to {host}"
            )));
        }

        let mut attempt = 0;
        let mut resp = loop {
            let resp = self
                .http
                .get(parsed.clone())
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

        let dir = dest
            .parent()
            .ok_or_else(|| Error::File("destination has no parent".into()))?;
        std::fs::create_dir_all(dir)?;
        let tmp = dest.with_extension("part");
        let mut out = tokio::fs::File::create(&tmp).await?;
        let mut written = 0u64;
        while let Some(chunk) = resp.chunk().await? {
            tokio::io::AsyncWriteExt::write_all(&mut out, &chunk).await?;
            written += chunk.len() as u64;
        }
        tokio::io::AsyncWriteExt::flush(&mut out).await?;
        drop(out);
        std::fs::rename(&tmp, dest)?;
        Ok(written)
    }
}

fn auth_headers(token: &Secret, cookie: &Secret) -> Result<HeaderMap> {
    let invalid = |what: &str| {
        Error::DesktopData(format!(
            "{what} contains characters not allowed in a header"
        ))
    };
    let mut bearer = HeaderValue::from_str(&format!("Bearer {}", token.expose()))
        .map_err(|_| invalid("token"))?;
    bearer.set_sensitive(true);
    let mut cookie =
        HeaderValue::from_str(&format!("d={}", cookie.expose())).map_err(|_| invalid("cookie"))?;
    cookie.set_sensitive(true);

    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, bearer);
    headers.insert(COOKIE, cookie);
    headers.insert(USER_AGENT, HeaderValue::from_static(DESKTOP_USER_AGENT));
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
