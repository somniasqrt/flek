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

/// Imports from desktop and stores only the first validated workspace.
/// `only` limits the import to one team id.
pub async fn import_and_save(
    store: &CredentialStore,
    only: Option<&str>,
) -> Result<Vec<ImportedTeam>> {
    let mut teams = import_from_desktop().await?;
    save_imported(store, only, &mut teams)?;
    Ok(teams)
}

/// User-initiated import: save the session and opt back into desktop recovery.
/// Broken preferences remain untouched and do not discard a valid imported session.
pub async fn import_explicit_and_save(
    store: &CredentialStore,
    only: Option<&str>,
    settings_path: &std::path::Path,
) -> Result<Vec<ImportedTeam>> {
    let mut teams = import_from_desktop().await?;
    save_explicit_import(store, only, &mut teams, settings_path)?;
    Ok(teams)
}

fn save_explicit_import(
    store: &CredentialStore,
    only: Option<&str>,
    teams: &mut Vec<ImportedTeam>,
    settings_path: &std::path::Path,
) -> Result<()> {
    save_imported(store, only, teams)?;
    if teams.iter().any(|t| t.result.is_ok()) {
        if let Err(error) = crate::settings::Settings::enable_desktop_import(settings_path) {
            tracing::warn!(%error, "session imported, but desktop auto-import could not be enabled; repair settings.json");
        }
    }
    Ok(())
}

fn save_imported(
    store: &CredentialStore,
    only: Option<&str>,
    teams: &mut Vec<ImportedTeam>,
) -> Result<()> {
    if let Some(id) = only {
        teams.retain(|t| t.team_id.eq_ignore_ascii_case(id));
        if teams.is_empty() {
            return Err(Error::TeamSelection(format!(
                "Slack desktop is not signed in to team {id}"
            )));
        }
    }
    // The first validated workspace is the one callers start; never retain others.
    if let Some(session) = teams.iter().find_map(|t| t.result.as_ref().ok()) {
        store.save(&session.credentials)?;
        let selected = session.credentials.team_id.clone();
        teams.retain(|t| {
            t.result
                .as_ref()
                .is_ok_and(|s| s.credentials.team_id == selected)
        });
        teams.truncate(1);
    }
    Ok(())
}

/// Validate page-captured tokens and retain only the first accepted workspace.
pub async fn validate_and_save_first(
    store: &CredentialStore,
    tokens: impl IntoIterator<Item = crate::Secret>,
    cookie: crate::Secret,
    origin: Option<SessionOrigin>,
) -> Result<Session> {
    save_first_with(store, tokens, |token| {
        validate(token, cookie.clone(), origin.clone())
    })
    .await
}

async fn save_first_with<T, F, Fut>(
    store: &CredentialStore,
    candidates: impl IntoIterator<Item = T>,
    mut validate: F,
) -> Result<Session>
where
    F: FnMut(T) -> Fut,
    Fut: std::future::Future<Output = Result<Session>>,
{
    let mut last_error = Error::DesktopNotLoggedIn;
    for candidate in candidates {
        match validate(candidate).await {
            Ok(session) => {
                store.save(&session.credentials)?;
                return Ok(session);
            }
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

/// Loads the stored session for `team` (or the only/first stored one),
/// importing from desktop when nothing is stored. When Slack rejects the
/// stored session it re-imports once before giving up.
pub async fn resolve_session(store: &CredentialStore, team: Option<&str>) -> Result<Session> {
    let path = crate::settings::default_path()?;
    let mut settings = crate::settings::Settings::safe_defaults();
    settings.reload(&path);
    let enabled = settings.desktop_import_on_start;
    resume_session(store, None, team, enabled).await
}

/// [`resolve_session`] with cache fallback: any non-auth validation failure
/// starts the session from the stored credentials and the
/// identity recorded in `db`, so the app opens from cache and the sync engine
/// connects once the network is back. Revocation recovery requires an enabled
/// desktop import and a recorded desktop origin, and accepts only the same team.
pub async fn resume_session(
    store: &CredentialStore,
    db: Option<&crate::store::Store>,
    team: Option<&str>,
    desktop_import_on_start: bool,
) -> Result<Session> {
    let creds = match pick_team(store, team)? {
        Some(id) => store.load(&id)?,
        None => None,
    };
    let Some(mut creds) = creds else {
        if !desktop_import_on_start {
            return Err(Error::DesktopImportDisabled);
        }
        tracing::info!("no stored credentials, importing from Slack desktop");
        return session_from_fresh_import(store, team).await;
    };

    if refresh_origin(&mut creds).await {
        store.save(&creds)?;
    }

    let validated = validate(
        creds.token.clone(),
        creds.cookie.clone(),
        creds.origin.clone(),
    )
    .await;
    resume_validated(store, db, creds, desktop_import_on_start, validated).await
}

async fn resume_validated(
    store: &CredentialStore,
    db: Option<&crate::store::Store>,
    creds: Credentials,
    desktop_import_on_start: bool,
    validated: Result<Session>,
) -> Result<Session> {
    match validated {
        Err(e) if e.is_auth_error() => {
            recover_rejected(store, &creds, desktop_import_on_start).await
        }
        Err(e) => {
            let Some(db) = db else { return Err(e) };
            let team_id = creds.team_id.clone();
            let stored = db
                .read(move |c| crate::store::directory::stored_identity(c, &team_id))
                .await?;
            match stored {
                Some(identity) => {
                    tracing::info!(error = %e, "session validation unavailable, starting from cache");
                    Ok(Session {
                        credentials: creds,
                        identity,
                    })
                }
                None => Err(e),
            }
        }
        Ok(session) => Ok(session),
    }
}

/// Recover a rejected session only under the owner's explicit desktop-import policy.
/// Delete rejected credentials before attempting I/O, so a failed import cannot revive them.
pub async fn recover_rejected(
    store: &CredentialStore,
    creds: &Credentials,
    enabled: bool,
) -> Result<Session> {
    recover_with(store, creds, enabled, import_from_desktop).await
}

async fn recover_with<F, Fut>(
    store: &CredentialStore,
    creds: &Credentials,
    enabled: bool,
    import: F,
) -> Result<Session>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Vec<ImportedTeam>>>,
{
    store.delete(&creds.team_id)?;
    if !enabled {
        return Err(Error::DesktopImportDisabled);
    }
    if !creds
        .origin
        .as_ref()
        .is_some_and(|o| matches!(o.kind, OriginKind::Desktop { .. }))
    {
        return Err(Error::DesktopNotLoggedIn);
    }
    let teams = import().await?;
    let session = teams
        .into_iter()
        .find_map(|t| {
            if t.team_id != creds.team_id {
                return None;
            }
            t.result.ok().filter(|s| {
                s.credentials.team_id == creds.team_id && s.identity.team_id == creds.team_id
            })
        })
        .ok_or(Error::DesktopNotLoggedIn)?;
    store.save(&session.credentials)?;
    Ok(session)
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
/// when Slack desktop was updated since. Unknown origins stay unknown, so they
/// cannot become eligible for automatic credential recovery. Returns
/// whether the origin changed.
pub async fn refresh_origin(creds: &mut Credentials) -> bool {
    let recorded = match &creds.origin {
        Some(SessionOrigin {
            kind: OriginKind::Desktop { app, .. },
            ..
        }) => Some(app.clone()),
        Some(_) => return false,
        None => return false,
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

/// Remove all indexed Flek sessions and suppress automatic desktop re-import on the
/// next launch. The local archive and the Slack desktop session are untouched.
pub fn sign_out(
    credentials: &CredentialStore,
    known_team_ids: &[String],
    settings_path: &std::path::Path,
) -> crate::Result<crate::settings::Settings> {
    let saved = (|| {
        let mut settings = crate::settings::Settings::load(settings_path)?;
        settings.desktop_import_on_start = false;
        settings.save(settings_path)?;
        Ok(settings)
    })();
    // Both operations must run; settings errors take precedence over purge errors.
    let cleared = credentials.clear_known(known_team_ids);
    saved.and_then(|settings| cleared.map(|()| settings))
}

#[cfg(test)]
mod sign_out_tests {
    use super::*;

    #[test]
    fn settings_failure_still_purges_credentials() {
        for unreadable in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(if unreadable {
                "settings.json"
            } else {
                "missing-settings.json/"
            });
            if unreadable {
                std::fs::write(&path, "malformed settings").unwrap();
            } else {
                // Missing file loads defaults, but rename to a trailing slash fails.
                assert!(crate::settings::Settings::load(&path).is_ok());
            }
            let credentials = CredentialStore::with_backend(keychain::MemoryBackend::default());
            credentials
                .save(&Credentials {
                    team_id: "T1".into(),
                    user_id: "U1".into(),
                    token: crate::Secret::new("xoxc-fake"),
                    cookie: crate::Secret::new("xoxd-fake"),
                    origin: None,
                })
                .unwrap();
            assert!(sign_out(&credentials, &["T1".into()], &path).is_err());
            assert!(credentials.load("T1").unwrap().is_none());
            assert!(credentials.list().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn removes_indexed_session_keeps_archive_and_language() {
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
        let mut other = credentials.load("TFAKE").unwrap().unwrap();
        other.team_id = "TOTHER".into();
        credentials.save(&other).unwrap();
        let result = sign_out(&credentials, &["TFAKE".into()], &path).unwrap();
        assert!(credentials.load("TOTHER").unwrap().is_none());
        assert!(!result.desktop_import_on_start);
        assert_eq!(result.language, crate::settings::Language::Uk);
        assert!(credentials.list().unwrap().is_empty());
        assert!(credentials.load("TFAKE").unwrap().is_none());
        assert_eq!(std::fs::read(archive).unwrap(), b"archive placeholder");
        // Boot's stored-session branch is false; resolution with the persisted policy
        // returns signed out without touching the desktop importer.
        assert!(pick_team(&credentials, None).unwrap().is_none());
        let saved = crate::settings::Settings::load(&path).unwrap();
        assert!(
            resume_session(&credentials, None, None, saved.desktop_import_on_start)
                .await
                .is_err()
        );
    }
}

#[cfg(test)]
mod security_tests;
