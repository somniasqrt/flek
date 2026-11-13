//! Error type shared by the whole core.
//!
//! Messages must never contain a token or cookie: only Slack error codes,
//! paths and descriptions of what went wrong.

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    NotFound(String),
    #[error("ambiguous conversation reference: {}", .candidates.iter().map(|c| format!("{} ({}, {})", c.label, c.id, c.kind)).collect::<Vec<_>>().join(", "))]
    Ambiguous {
        candidates: Vec<crate::references::Candidate>,
    },
    #[error("{0}")]
    Usage(String),
    #[error("operation forbidden")]
    Forbidden,
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

    #[error("incomplete pagination from {0}; sync progress was not advanced")]
    IncompleteSync(&'static str),

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

    #[error("websocket: {0}")]
    WebSocket(String),

    #[error("file download: {0}")]
    File(String),

    #[error("database: {0}")]
    Store(String),

    #[error("LevelDB: {0}")]
    LevelDb(String),
}

impl Error {
    /// Slack could not be reached or answered with a server error; worth
    /// retrying later, the session itself may be fine.
    pub fn is_unreachable(&self) -> bool {
        match self {
            Error::Http(e) => e.is_connect() || e.is_timeout() || e.is_request(),
            Error::HttpStatus(s) => *s >= 500,
            _ => false,
        }
    }

    /// The stored session is no longer accepted by Slack.
    pub fn is_auth_error(&self) -> bool {
        matches!(self, Error::Slack { code, .. } if code == "invalid_auth" || code == "token_revoked" || code == "not_authed" || code == "account_inactive")
    }
}

/// Stable machine error categories, also usable outside the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum ErrorKind {
    Internal = 1,
    Usage = 2,
    NotFound = 3,
    Ambiguous = 4,
    Auth = 5,
    RateLimited = 6,
    Network = 7,
    Forbidden = 8,
}
impl ErrorKind {
    pub fn exit_code(self) -> i32 {
        self as i32
    }
}
impl Error {
    pub fn kind(&self) -> ErrorKind {
        if self.is_auth_error() {
            return ErrorKind::Auth;
        }
        match self {
            Self::NotFound(_) => ErrorKind::NotFound,
            Self::Ambiguous { .. } | Self::TeamSelection(_) => ErrorKind::Ambiguous,
            Self::Usage(_) => ErrorKind::Usage,
            Self::Forbidden => ErrorKind::Forbidden,
            Self::NoCredentials
            | Self::DesktopNotFound { .. }
            | Self::DesktopNotLoggedIn
            | Self::Keychain(_)
            | Self::CookieDecrypt(_) => ErrorKind::Auth,
            Self::Slack { code, .. } if code == "ratelimited" || code == "rate_limited" => {
                ErrorKind::RateLimited
            }
            Self::HttpStatus(429) => ErrorKind::RateLimited,
            Self::Slack { .. }
            | Self::Http(_)
            | Self::HttpStatus(_)
            | Self::WebSocket(_)
            | Self::IncompleteSync(_) => ErrorKind::Network,
            _ => ErrorKind::Internal,
        }
    }
}
