//! Getting and keeping a Slack session.

pub mod chromium;
pub mod desktop_import;
pub mod desktop_profile;
pub mod keychain;
pub mod origin;
pub mod webview;

use crate::api::{AuthTest, Client, ClientProfile, DEFAULT_BASE_URL};
use crate::{Error, Result};
pub use keychain::{CredentialStore, Credentials};
pub use origin::{OriginKind, SessionOrigin};

/// A session Slack has just accepted.
#[derive(Debug, Clone)]
pub struct Session {
    pub credentials: Credentials,
    pub identity: AuthTest,
}

impl Session {
    /// A client presenting the identity of the session's issuer.
    pub fn client(&self) -> Result<Client> {
        client_for(&self.credentials, &team_api_base(&self.identity.url))
    }
}

/// Outcome of importing one workspace from Slack desktop.
#[derive(Debug)]
pub struct ImportedTeam {
    pub team_id: String,
    pub name: String,
    pub result: Result<Session>,
}

/// Reads the desktop session and validates each workspace with `auth.test`.
/// Nothing is saved; see [`import_and_save`].
pub async fn import_from_desktop() -> Result<Vec<ImportedTeam>> {
    let (desktop, origin) = tokio::task::spawn_blocking(|| {
        let desktop = desktop_import::import()?;
        Ok::<_, Error>((desktop, desktop_origin()))
    })
    .await
    .map_err(|e| Error::DesktopData(format!("import task failed: {e}")))??;

    let mut out = Vec::with_capacity(desktop.teams.len());
    for team in desktop.teams {
        let result = validate(team.token.clone(), desktop.cookie.clone(), origin.clone()).await;
        out.push(ImportedTeam {
            team_id: team.team_id,
            name: team.name,
            result,
        });
    }
    Ok(out)
}

/// Imports from desktop and stores every workspace that validated.
/// `only` limits the import to one team id.
pub async fn import_and_save(
    store: &CredentialStore,
    only: Option<&str>,
) -> Result<Vec<ImportedTeam>> {
    let mut teams = import_from_desktop().await?;
    if let Some(id) = only {
        teams.retain(|t| t.team_id.eq_ignore_ascii_case(id));
        if teams.is_empty() {
            return Err(Error::TeamSelection(format!(
                "Slack desktop is not signed in to team {id}"
            )));
        }
    }
    for team in &teams {
        if let Ok(session) = &team.result {
            store.save(&session.credentials)?;
        }
    }
    Ok(teams)
}

/// Loads the stored session for `team` (or the only/first stored one),
/// importing from desktop when nothing is stored. When Slack rejects the
/// stored session it re-imports once before giving up.
pub async fn resolve_session(store: &CredentialStore, team: Option<&str>) -> Result<Session> {
    resume_session(store, None, team).await
}

/// [`resolve_session`] for the app: when Slack cannot be reached (offline,
/// server error), the session starts from the stored credentials and the
/// identity recorded in `db`, so the app opens from cache and the sync engine
/// connects once the network is back. A revoked session still re-imports.
pub async fn resume_session(
    store: &CredentialStore,
    db: Option<&crate::store::Store>,
    team: Option<&str>,
) -> Result<Session> {
    let creds = match pick_team(store, team)? {
        Some(id) => store.load(&id)?,
        None => None,
    };
    let Some(mut creds) = creds else {
        tracing::info!("no stored credentials, importing from Slack desktop");
        return session_from_fresh_import(store, team).await;
    };

    if refresh_origin(&mut creds).await {
        store.save(&creds)?;
    }

    match validate(
        creds.token.clone(),
        creds.cookie.clone(),
        creds.origin.clone(),
    )
    .await
    {
        Err(e) if e.is_auth_error() => {
            tracing::info!(team_id = %creds.team_id, "stored session rejected, re-importing from Slack desktop");
            session_from_fresh_import(store, Some(&creds.team_id)).await
        }
        Err(e) if e.is_unreachable() => {
            let Some(db) = db else { return Err(e) };
            let team_id = creds.team_id.clone();
            let stored = db
                .read(move |c| crate::store::directory::stored_identity(c, &team_id))
                .await?;
            match stored {
                Some(identity) => {
                    tracing::info!(error = %e, "Slack unreachable, starting offline");
                    Ok(Session {
                        credentials: creds,
                        identity,
                    })
                }
                None => Err(e),
            }
        }
        other => other,
    }
}

async fn session_from_fresh_import(store: &CredentialStore, team: Option<&str>) -> Result<Session> {
    let teams = import_and_save(store, team).await?;
    let mut last_err = None;
    for t in teams {
        match t.result {
            Ok(session) => return Ok(session),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or(Error::DesktopNotLoggedIn))
}

fn pick_team(store: &CredentialStore, team: Option<&str>) -> Result<Option<String>> {
    let ids = store.list()?;
    Ok(match team {
        Some(t) => ids.into_iter().find(|id| id.eq_ignore_ascii_case(t)),
        None => ids.into_iter().next(),
    })
}

/// Checks a session with `auth.test`, presenting its issuer's identity.
pub async fn validate(
    token: crate::Secret,
    cookie: crate::Secret,
    origin: Option<SessionOrigin>,
) -> Result<Session> {
    let mut credentials = Credentials {
        team_id: String::new(),
        user_id: String::new(),
        token,
        cookie,
        origin,
    };
    let identity = client_for(&credentials, DEFAULT_BASE_URL)?
        .auth_test()
        .await?;
    credentials.team_id = identity.team_id.clone();
    credentials.user_id = identity.user_id.clone();
    Ok(Session {
        credentials,
        identity,
    })
}

/// The team's own API root (`https://<team>.slack.com/api`), where Slack
/// desktop sends its calls. Anything that is not a Slack host falls back to
/// `slack.com`, so the session never goes elsewhere.
pub fn team_api_base(team_url: &str) -> String {
    reqwest::Url::parse(team_url)
        .ok()
        .filter(|u| u.scheme() == "https")
        .and_then(|u| u.host_str().map(str::to_owned))
        .filter(|h| h.ends_with(".slack.com"))
        .map_or_else(
            || DEFAULT_BASE_URL.to_owned(),
            |h| format!("https://{h}/api"),
        )
}

fn client_for(creds: &Credentials, base_url: &str) -> Result<Client> {
    let profile = creds
        .origin
        .as_ref()
        .map_or_else(ClientProfile::detect, SessionOrigin::profile);
    Client::with_profile(creds.token.clone(), creds.cookie.clone(), base_url, profile)
}

/// The installed Slack desktop's identity as a session origin. Blocking
/// (reads logs, maybe the Electron binary); `None` when it cannot be told.
pub fn desktop_origin() -> Option<SessionOrigin> {
    let home = dirs::home_dir()?;
    let root = desktop_import::locate(&home).ok()?.root;
    let app = desktop_profile::find_app(&home);
    match desktop_profile::read(app.as_deref(), &root) {
        Ok(p) => {
            tracing::info!(app = %p.app, from_logs = p.from_logs, "Slack desktop profile");
            Some(SessionOrigin::new(
                OriginKind::Desktop {
                    app: p.app,
                    electron: p.electron,
                    chrome: p.chrome,
                },
                p.user_agent,
            ))
        }
        Err(e) => {
            tracing::warn!("cannot read the Slack desktop profile: {e}");
            None
        }
    }
}

/// Brings a stored session's origin up to date: re-reads the desktop profile
/// when Slack desktop was updated since, and records one for sessions stored
/// before origins existed (all of those came from desktop import). Returns
/// whether the origin changed.
pub async fn refresh_origin(creds: &mut Credentials) -> bool {
    let recorded = match &creds.origin {
        Some(SessionOrigin {
            kind: OriginKind::Desktop { app, .. },
            ..
        }) => Some(app.clone()),
        Some(_) => return false,
        None => None,
    };
    let fresh = tokio::task::spawn_blocking(move || {
        let installed = desktop_profile::installed_version(&dirs::home_dir()?);
        if recorded.is_some() && (installed.is_none() || installed == recorded) {
            return None;
        }
        desktop_origin()
    })
    .await
    .ok()
    .flatten();
    match fresh {
        Some(origin) => {
            tracing::info!(to = %origin.describe(), "session origin updated");
            creds.origin = Some(origin);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::team_api_base;

    #[test]
    fn team_api_base_stays_on_slack() {
        assert_eq!(
            team_api_base("https://w1-rx2.slack.com/"),
            "https://w1-rx2.slack.com/api"
        );
        for bad in [
            "",
            "http://t.slack.com/",
            "https://evil.com/",
            "https://slack.com.evil.com/",
        ] {
            assert_eq!(team_api_base(bad), crate::api::DEFAULT_BASE_URL, "{bad}");
        }
    }
}

/// Remove only Flek's session and suppress automatic desktop re-import on the
/// next launch. The local archive and the Slack desktop session are untouched.
pub fn sign_out(
    credentials: &CredentialStore,
    team_id: &str,
    settings_path: &std::path::Path,
) -> crate::Result<crate::settings::Settings> {
    let previous = crate::settings::Settings::load(settings_path)?;
    let mut settings = previous.clone();
    settings.desktop_import_on_start = false;
    settings.save(settings_path)?;
    if let Err(error) = credentials.delete(team_id) {
        previous.save(settings_path)?;
        return Err(error);
    }
    Ok(settings)
}

#[cfg(test)]
mod sign_out_tests {
    use super::*;

    #[test]
    fn removes_indexed_session_keeps_archive_and_language() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let archive = dir.path().join("archive.db");
        std::fs::write(&archive, b"archive placeholder").unwrap();
        let settings = crate::settings::Settings {
            language: crate::settings::Language::Uk,
            ..Default::default()
        };
        settings.save(&path).unwrap();
        let credentials = CredentialStore::with_backend(keychain::MemoryBackend::default());
        credentials
            .save(&Credentials {
                team_id: "TFAKE".into(),
                user_id: "UFAKE".into(),
                token: crate::Secret::new("xoxc-fake"),
                cookie: crate::Secret::new("xoxd-fake"),
                origin: None,
            })
            .unwrap();
        let result = sign_out(&credentials, "TFAKE", &path).unwrap();
        assert!(!result.desktop_import_on_start);
        assert_eq!(result.language, crate::settings::Language::Uk);
        assert!(credentials.list().unwrap().is_empty());
        assert!(credentials.load("TFAKE").unwrap().is_none());
        assert_eq!(std::fs::read(archive).unwrap(), b"archive placeholder");
    }
}
