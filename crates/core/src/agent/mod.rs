//! Reusable write guard. Payloads never enter the audit database.
mod audit;
mod policy;

use crate::{Error, Result};
pub use audit::{ActionLog, LogEntry, Reservation};
pub use policy::{AgentConfig, AllowRule};
use serde::Serialize;
use std::path::PathBuf;

/// Stable action name and resolved destination identities, independent of transport.
#[derive(Clone, Serialize)]
pub struct ActionDescriptor {
    pub action: String,
    pub team_id: String,
    /// One channel, or up to eight recipient user ids for `dm`.
    pub targets: Vec<String>,
}
impl ActionDescriptor {
    pub fn validate(&self) -> Result<()> {
        if self.action.is_empty()
            || self.action.len() > 40
            || !self
                .action
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'-')
            || !valid_id(&self.team_id, "T")
            || self.targets.is_empty()
            || self.targets.len() > if self.action == "dm" { 8 } else { 1 }
            || self
                .targets
                .iter()
                .any(|t| !valid_id(t, if self.action == "dm" { "UW" } else { "CDG" }))
        {
            return Err(Error::Usage(
                "invalid action or destination; mass writes are not supported".into(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn valid_id(id: &str, prefixes: &str) -> bool {
    id.len() > 1
        && id.len() <= 64
        && id.starts_with(|c| prefixes.contains(c))
        && id
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

pub fn default_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| Error::Store("home directory is unknown".into()))?;
    Ok(home.join(".config/flek/agent.toml"))
}

pub fn default_log_path() -> Result<PathBuf> {
    Ok(crate::store::default_path()?.with_file_name("agent-actions.db"))
}
