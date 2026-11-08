//! Error type shared by the whole core.
//!
//! Messages must never contain a token or cookie: only Slack error codes,
//! paths and descriptions of what went wrong.

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Slack desktop app not found (looked in: {searched})")]
    DesktopNotFound { searched: String },

    #[error("Slack desktop is installed but not signed in to any workspace")]
    DesktopNotLoggedIn,

    #[error("unexpected Slack desktop data: {0}")]
    DesktopData(String),

    #[error("could not decrypt the session cookie: {0}")]
    CookieDecrypt(String),

    #[error("keychain: {0}")]
    Keychain(String),

    #[error("no stored credentials; run `flek auth import`")]
    NoCredentials,

    #[error("{0}")]
    TeamSelection(String),

    #[error("Slack API error: {code}{}", needed.as_deref().map(|n| format!(" (needed scope: {n})")).unwrap_or_default())]
    Slack {
        code: String,
        needed: Option<String>,
    },

    #[error("unexpected HTTP status {0}")]
    HttpStatus(u16),

    #[error("not supported on this platform: {0}")]
    Unsupported(&'static str),

    #[error("HTTP: {0}")]
    Http(#[from] reqwest::Error),

    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("LevelDB: {0}")]
    LevelDb(String),
}

impl Error {
    /// The stored session is no longer accepted by Slack.
    pub fn is_auth_error(&self) -> bool {
        matches!(self, Error::Slack { code, .. } if code == "invalid_auth" || code == "token_revoked" || code == "not_authed")
    }
}
