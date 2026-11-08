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
        })
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
