//! Getting and keeping a Slack session.

pub mod chromium;
pub mod desktop_import;
pub mod keychain;

use crate::api::{AuthTest, Client};
use crate::{Error, Result};
pub use keychain::{CredentialStore, Credentials};

/// A session Slack has just accepted.
#[derive(Debug, Clone)]
pub struct Session {
    pub credentials: Credentials,
    pub identity: AuthTest,
}

impl Session {
    pub fn client(&self) -> Result<Client> {
        Client::new(
            self.credentials.token.clone(),
            self.credentials.cookie.clone(),
        )
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
    let desktop = tokio::task::spawn_blocking(desktop_import::import)
        .await
        .map_err(|e| Error::DesktopData(format!("import task failed: {e}")))??;

    let mut out = Vec::with_capacity(desktop.teams.len());
    for team in desktop.teams {
        let result = validate(team.token.clone(), desktop.cookie.clone()).await;
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
    let creds = match pick_team(store, team)? {
        Some(id) => store.load(&id)?,
        None => None,
    };
    let Some(creds) = creds else {
        tracing::info!("no stored credentials, importing from Slack desktop");
        return session_from_fresh_import(store, team).await;
    };

    match validate(creds.token.clone(), creds.cookie.clone()).await {
        Err(e) if e.is_auth_error() => {
            tracing::info!(team_id = %creds.team_id, "stored session rejected, re-importing from Slack desktop");
            session_from_fresh_import(store, Some(&creds.team_id)).await
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

async fn validate(token: crate::Secret, cookie: crate::Secret) -> Result<Session> {
    let client = Client::new(token.clone(), cookie.clone())?;
    let identity = client.auth_test().await?;
    Ok(Session {
        credentials: Credentials {
            team_id: identity.team_id.clone(),
            user_id: identity.user_id.clone(),
            token,
            cookie,
        },
        identity,
    })
}
