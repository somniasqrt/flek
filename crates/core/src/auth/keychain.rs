//! Credentials in the OS credential store via `keyring` (service `flek`,
//! account = team id): the macOS Keychain today; the Windows Credential
//! Manager and Linux Secret Service backends are ROADMAP M7.1.3,
//! plus reading Slack's own "Safe Storage" password.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::SessionOrigin;
use crate::{Error, Result, Secret};

const SERVICE: &str = "flek";
/// Account holding the JSON list of stored team ids (keyring cannot enumerate).
/// Team ids start with `T`/`E`, so this cannot collide.
const INDEX_ACCOUNT: &str = "_teams";

/// A validated session for one workspace.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub team_id: String,
    pub user_id: String,
    pub token: Secret,
    pub cookie: Secret,
    /// The client that issued the session (ADR 0001); `None` for sessions
    /// stored before origins were recorded.
    pub origin: Option<SessionOrigin>,
}

/// Minimal key/value secret storage, so tests can run without a keychain.
pub trait SecretBackend: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>>;
    fn set(&self, account: &str, value: &str) -> Result<()>;
    /// Returns whether something was deleted.
    fn delete(&self, account: &str) -> Result<bool>;
}

/// The OS keychain via `keyring`.
pub struct KeyringBackend {
    service: String,
}

impl KeyringBackend {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, account).map_err(keychain_err)
    }
}

impl SecretBackend for KeyringBackend {
    fn get(&self, account: &str) -> Result<Option<String>> {
        match self.entry(account)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_err(e)),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<()> {
        self.entry(account)?
            .set_password(value)
            .map_err(keychain_err)
    }

    fn delete(&self, account: &str) -> Result<bool> {
        match self.entry(account)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(keychain_err(e)),
        }
    }
}

/// In-process storage for tests.
#[derive(Default)]
pub struct MemoryBackend(Mutex<HashMap<String, String>>);

impl SecretBackend for MemoryBackend {
    fn get(&self, account: &str) -> Result<Option<String>> {
        Ok(self.0.lock().expect("poisoned").get(account).cloned())
    }

    fn set(&self, account: &str, value: &str) -> Result<()> {
        self.0
            .lock()
            .expect("poisoned")
            .insert(account.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<bool> {
        Ok(self.0.lock().expect("poisoned").remove(account).is_some())
    }
}

fn keychain_err(e: keyring::Error) -> Error {
    // keyring errors describe the failure, never the stored value.
    Error::Keychain(e.to_string())
}

#[derive(Serialize, Deserialize)]
struct Stored {
    team_id: String,
    user_id: String,
    token: String,
    cookie: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<SessionOrigin>,
}

/// save / load / delete / list of [`Credentials`].
pub struct CredentialStore {
    backend: Box<dyn SecretBackend>,
}

impl CredentialStore {
    /// The real keychain, service `flek`.
    pub fn keychain() -> Self {
        Self::with_backend(KeyringBackend::new(SERVICE))
    }

    pub fn with_backend(backend: impl SecretBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
        }
    }

    pub fn save(&self, creds: &Credentials) -> Result<()> {
        let mut stored = Stored {
            team_id: creds.team_id.clone(),
            user_id: creds.user_id.clone(),
            token: creds.token.expose().to_owned(),
            cookie: creds.cookie.expose().to_owned(),
            origin: creds.origin.clone(),
        };
        let json = Secret::new(serde_json::to_string(&stored)?);
        stored.token.zeroize();
        stored.cookie.zeroize();
        self.backend.set(&creds.team_id, json.expose())?;

        let mut ids = self.list()?;
        if !ids.contains(&creds.team_id) {
            ids.push(creds.team_id.clone());
            self.write_index(&ids)?;
        }
        Ok(())
    }

    pub fn load(&self, team_id: &str) -> Result<Option<Credentials>> {
        let Some(json) = self.backend.get(team_id)?.map(Secret::new) else {
            return Ok(None);
        };
        let stored: Stored = serde_json::from_str(json.expose()).map_err(|_| {
            Error::Keychain(format!("stored credentials for {team_id} are malformed"))
        })?;
        Ok(Some(Credentials {
            team_id: stored.team_id,
            user_id: stored.user_id,
            token: Secret::new(stored.token),
            cookie: Secret::new(stored.cookie),
            origin: stored.origin,
        }))
    }

    pub fn delete(&self, team_id: &str) -> Result<bool> {
        let deleted = self.backend.delete(team_id)?;
        let mut ids = self.list()?;
        let before = ids.len();
        ids.retain(|id| id != team_id);
        if ids.len() != before {
            self.write_index(&ids)?;
        }
        Ok(deleted)
    }

    /// Team ids with stored credentials, in the order they were added.
    pub fn list(&self) -> Result<Vec<String>> {
        match self.backend.get(INDEX_ACCOUNT)? {
            Some(json) => serde_json::from_str(&json)
                .map_err(|_| Error::Keychain("credential index is malformed".into())),
            None => Ok(Vec::new()),
        }
    }

    fn write_index(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            self.backend.delete(INDEX_ACCOUNT)?;
            return Ok(());
        }
        self.backend
            .set(INDEX_ACCOUNT, &serde_json::to_string(ids)?)
    }
}

/// The password Slack desktop uses to encrypt its cookies. macOS shows an
/// "allow access" prompt the first time.
#[cfg(target_os = "macos")]
pub fn slack_safe_storage_password() -> Result<Secret> {
    use security_framework::passwords::get_generic_password;

    const SAFE_STORAGE_SERVICE: &str = "Slack Safe Storage";
    let mut last_err = None;
    for account in ["Slack Key", "Slack"] {
        match get_generic_password(SAFE_STORAGE_SERVICE, account) {
            Ok(bytes) => {
                let password = String::from_utf8(bytes).map_err(|_| {
                    Error::Keychain("Slack Safe Storage password is not UTF-8".into())
                })?;
                return Ok(Secret::new(password));
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(Error::Keychain(format!(
        "cannot read \"{SAFE_STORAGE_SERVICE}\": {}",
        last_err.map(|e| e.to_string()).unwrap_or_default()
    )))
}

#[cfg(not(target_os = "macos"))]
pub fn slack_safe_storage_password() -> Result<Secret> {
    Err(Error::Unsupported(
        "reading Slack desktop cookies outside macOS",
    ))
}
