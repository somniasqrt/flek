use super::ActionDescriptor;
use crate::{Error, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    pub writes_per_minute: u32,
    /// Appended to send/schedule text, and to blocks as a visible context block.
    pub signature: Option<String>,
    pub allow: Vec<AllowRule>,
}
impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            writes_per_minute: 10,
            signature: None,
            allow: Vec::new(),
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowRule {
    pub team: String,
    pub channels: Vec<String>,
    pub actions: Vec<String>,
}
impl AgentConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        // Parser diagnostics may quote the source. Never propagate them.
        let config: Self = toml::from_str(&source)
            .map_err(|_| Error::Usage("invalid agent.toml; expected agent policy fields".into()))?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        if !(1..=60).contains(&self.writes_per_minute)
            || self.signature.as_ref().is_some_and(|s| {
                s.trim().is_empty() || s.len() > 256 || s.contains("xox") || s.contains("Cookie:")
            })
        {
            return Err(Error::Usage("agent policy requires 1..60 writes_per_minute and a nonempty signature of at most 256 bytes".into()));
        }
        Ok(())
    }
    pub fn authorize(&self, action: &ActionDescriptor, yes: bool) -> Result<()> {
        self.validate()?;
        action.validate()?;
        if yes
            || self.allow.iter().any(|rule| {
                rule.team == action.team_id
                    && rule.actions.iter().any(|a| a == &action.action || a == "*")
                    && action
                        .targets
                        .iter()
                        .all(|target| rule.channels.iter().any(|c| c == target || c == "*"))
            })
        {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }
}
