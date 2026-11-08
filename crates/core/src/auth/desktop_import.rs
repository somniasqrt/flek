//! Import the session from the installed Slack desktop app (macOS).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::chromium::{self, StoredCookie};
use crate::{Error, Result, Secret};

const SLACK_ORIGIN: &str = "https://app.slack.com";
const LOCAL_CONFIG_KEY: &str = "localConfig_v2";
const COOKIE_HOSTS: &[&str] = &[".slack.com", "slack.com"];
const COOKIE_NAME: &str = "d";

/// Where Slack desktop keeps its profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPaths {
    pub root: PathBuf,
    pub leveldb: PathBuf,
    pub cookies: PathBuf,
}

/// One workspace the desktop app is signed in to.
#[derive(Debug, Clone)]
pub struct TeamSession {
    pub team_id: String,
    pub name: String,
    pub url: String,
    pub token: Secret,
}

/// Everything needed to talk to Slack as the user.
#[derive(Debug, Clone)]
pub struct DesktopSession {
    pub teams: Vec<TeamSession>,
    pub cookie: Secret,
}

/// Profile directories to check, regular build first, then App Store.
pub fn candidate_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("Library/Application Support/Slack"),
        home.join(
            "Library/Containers/com.tinyspeck.slackmacgap/Data/Library/Application Support/Slack",
        ),
    ]
}

/// Finds the first candidate root with both Local Storage and a cookie store.
pub fn locate(home: &Path) -> Result<DesktopPaths> {
    let roots = candidate_roots(home);
    let mut any_root = false;
    for root in &roots {
        if !root.is_dir() {
            continue;
        }
        any_root = true;
        let leveldb = root.join("Local Storage/leveldb");
        let cookies = [root.join("Network/Cookies"), root.join("Cookies")]
            .into_iter()
            .find(|p| p.is_file());
        if let (true, Some(cookies)) = (leveldb.is_dir(), cookies) {
            return Ok(DesktopPaths {
                root: root.clone(),
                leveldb,
                cookies,
            });
        }
        tracing::debug!(root = %root.display(), "Slack profile is incomplete");
    }
    if any_root {
        return Err(Error::DesktopNotLoggedIn);
    }
    Err(Error::DesktopNotFound {
        searched: roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
    })
}

/// Reads `localConfig_v2` and returns the workspaces with their tokens.
pub fn read_teams(leveldb_dir: &Path) -> Result<Vec<TeamSession>> {
    let copy = chromium::copy_leveldb(leveldb_dir)?;
    let raw = chromium::find_local_storage_value(copy.path(), SLACK_ORIGIN, LOCAL_CONFIG_KEY)?
        .ok_or(Error::DesktopNotLoggedIn)?;
    let json = chromium::decode_storage_string(&raw)?;
    parse_teams(&json)
}

/// Parses the `localConfig_v2` JSON. Teams without an `xoxc` token are skipped.
pub fn parse_teams(json: &str) -> Result<Vec<TeamSession>> {
    #[derive(Deserialize)]
    struct LocalConfig {
        #[serde(default)]
        teams: std::collections::BTreeMap<String, Team>,
    }
    #[derive(Deserialize)]
    struct Team {
        id: Option<String>,
        #[serde(default)]
        name: String,
        #[serde(default)]
        url: String,
        token: Option<String>,
    }

    let config: LocalConfig = serde_json::from_str(json)
        .map_err(|e| Error::DesktopData(format!("localConfig_v2 is not valid: {e}")))?;
    let teams: Vec<TeamSession> = config
        .teams
        .into_iter()
        .filter_map(|(key, t)| {
            let token = t.token.filter(|t| t.starts_with("xoxc-"))?;
            Some(TeamSession {
                team_id: t.id.unwrap_or(key),
                name: t.name,
                url: t.url,
                token: Secret::new(token),
            })
        })
        .collect();
    if teams.is_empty() {
        return Err(Error::DesktopNotLoggedIn);
    }
    Ok(teams)
}

/// Reads and decrypts the `d` cookie. `password` is the "Slack Safe Storage"
/// keychain password; it is only fetched (and the OS prompt only shown) when
/// the cookie is actually encrypted.
pub fn read_cookie(cookies_db: &Path, password: impl FnOnce() -> Result<Secret>) -> Result<Secret> {
    match chromium::read_cookie(cookies_db, COOKIE_HOSTS, COOKIE_NAME)? {
        Some(StoredCookie::Plain(v)) => Ok(Secret::new(v)),
        Some(StoredCookie::Encrypted(enc)) => {
            let key = chromium::derive_key(&password()?);
            chromium::decrypt_cookie(&enc, &key)
        }
        None => Err(Error::DesktopNotLoggedIn),
    }
}

/// Full import from the Slack desktop profile of the current user. Blocking:
/// reads files and may show the macOS keychain prompt.
pub fn import() -> Result<DesktopSession> {
    let home =
        dirs::home_dir().ok_or_else(|| Error::DesktopData("home directory is unknown".into()))?;
    let paths = locate(&home)?;
    tracing::info!(root = %paths.root.display(), "reading Slack desktop profile");
    let teams = read_teams(&paths.leveldb)?;
    let cookie = read_cookie(&paths.cookies, super::keychain::slack_safe_storage_password)?;
    Ok(DesktopSession { teams, cookie })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_teams_and_skips_tokenless() {
        let json = r#"{
            "teams": {
                "T1": {"id": "T1", "name": "One", "url": "https://one.slack.com/", "token": "xoxc-1"},
                "T2": {"name": "Two", "url": "https://two.slack.com/", "token": "xoxc-2"},
                "T3": {"id": "T3", "name": "No token"}
            },
            "lastActiveTeamId": "T1"
        }"#;
        let teams = parse_teams(json).unwrap();
        assert_eq!(teams.len(), 2);
        assert_eq!(teams[0].team_id, "T1");
        assert_eq!(teams[0].token.expose(), "xoxc-1");
        assert_eq!(teams[1].team_id, "T2");
        assert_eq!(teams[1].url, "https://two.slack.com/");
    }

    #[test]
    fn no_teams_means_not_logged_in() {
        assert!(matches!(
            parse_teams(r#"{"teams":{}}"#),
            Err(Error::DesktopNotLoggedIn)
        ));
        assert!(matches!(parse_teams("{}"), Err(Error::DesktopNotLoggedIn)));
    }

    #[test]
    fn locate_reports_missing_app() {
        let home = tempfile::tempdir().unwrap();
        assert!(matches!(
            locate(home.path()),
            Err(Error::DesktopNotFound { .. })
        ));
    }

    #[test]
    fn locate_finds_app_store_build_and_network_cookies() {
        let home = tempfile::tempdir().unwrap();
        let root = &candidate_roots(home.path())[1];
        std::fs::create_dir_all(root.join("Local Storage/leveldb")).unwrap();
        std::fs::create_dir_all(root.join("Network")).unwrap();
        std::fs::write(root.join("Network/Cookies"), b"").unwrap();
        let paths = locate(home.path()).unwrap();
        assert_eq!(&paths.root, root);
        assert_eq!(paths.cookies, root.join("Network/Cookies"));
    }

    #[test]
    fn locate_reports_incomplete_profile_as_not_logged_in() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(&candidate_roots(home.path())[0]).unwrap();
        assert!(matches!(
            locate(home.path()),
            Err(Error::DesktopNotLoggedIn)
        ));
    }
}
