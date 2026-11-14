//! What the app holds: the database, the credential store, the change bus,
//! and the background sync engine. Everything real lives in `flek-core`.

use std::sync::{Arc, Mutex};

use flek_core::api::Client;

use flek_core::auth::{self, CredentialStore, Session};
use flek_core::store::Store;
use flek_core::sync::engine::{Engine, EngineOptions, Priority};
use flek_core::sync::live::{bus, Bus, Change, ConnectionState, Progress};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::watch;

/// Event carrying every [`Change`] to the UI.
pub const CHANGE_EVENT: &str = "flek://change";
/// Event carrying the new [`Status`].
pub const STATUS_EVENT: &str = "flek://status";

/// Where the app is in its life: the UI renders one screen per phase.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Status {
    Starting,
    /// Reading the Slack desktop session (macOS may ask for Keychain access).
    Importing,
    /// No usable session: the sign-in window is the way forward.
    SignIn {
        desktop: bool,
        reason: Option<String>,
    },
    /// Signing in through the sign-in window.
    SigningIn,
    Ready {
        team_id: String,
        team: String,
        user_id: String,
        user: String,
    },
}

#[derive(Default)]
pub struct SavedDownloads {
    pub next: u64,
    pub paths: std::collections::VecDeque<(String, std::path::PathBuf)>,
}

pub struct AppState {
    pub saved_downloads: Mutex<SavedDownloads>,
    pub maintenance: tokio::sync::Mutex<()>,
    generation: std::sync::atomic::AtomicU64,
    pub settings: Arc<Mutex<flek_core::settings::Settings>>,
    pub session: Mutex<Option<Session>>,
    engine_done: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    pub store: Store,
    pub creds: CredentialStore,
    pub bus: Bus,
    pub priority: Priority,
    pub presence: watch::Sender<Vec<String>>,
    status: Mutex<Status>,
    /// Latest progress, for a UI that mounts after it was sent.
    progress: Mutex<Option<Progress>>,
    connection: Mutex<Option<ConnectionState>>,
    /// User agent of the session's client, for public image fetches.
    user_agent: Mutex<Option<String>>,
    engine_stop: Mutex<Option<watch::Sender<bool>>>,
    /// Client of the ready session, for writes (sending messages).
    client: Mutex<Option<Arc<Client>>>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        let settings = startup_settings(flek_core::settings::default_path());
        Self {
            saved_downloads: Mutex::new(SavedDownloads::default()),
            maintenance: tokio::sync::Mutex::new(()),
            generation: std::sync::atomic::AtomicU64::new(0),
            settings: Arc::new(Mutex::new(settings)),
            session: Mutex::new(None),
            engine_done: Mutex::new(None),
            store,
            creds: CredentialStore::keychain(),
            bus: bus(),
            priority: Priority::default(),
            presence: watch::channel(Vec::new()).0,
            status: Mutex::new(Status::Starting),
            progress: Mutex::new(None),
            connection: Mutex::new(None),
            user_agent: Mutex::new(None),
            engine_stop: Mutex::new(None),
            client: Mutex::new(None),
        }
    }

    pub fn settings(&self) -> flek_core::settings::Settings {
        self.settings.lock().expect("settings lock").clone()
    }

    pub async fn stop_and_wait(&self) {
        self.stop_engine();
        let done = self.engine_done.lock().ok().and_then(|mut d| d.take());
        if let Some(done) = done {
            let _ = done.await;
        }
    }

    pub fn forget_session(&self) {
        self.saved_downloads
            .lock()
            .expect("downloads lock")
            .paths
            .clear();
        *self.session.lock().expect("session lock") = None;
        *self.client.lock().expect("client lock") = None;
        *self.progress.lock().expect("progress lock") = None;
        *self.connection.lock().expect("connection lock") = None;
        self.presence.send_replace(Vec::new());
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or(Status::Starting)
    }

    pub fn progress(&self) -> Option<Progress> {
        self.progress.lock().ok().and_then(|p| p.clone())
    }

    pub fn connection(&self) -> Option<ConnectionState> {
        self.connection.lock().ok().and_then(|c| *c)
    }

    /// Team of the ready session.
    pub fn team_id(&self) -> Option<String> {
        match self.status() {
            Status::Ready { team_id, .. } => Some(team_id),
            _ => None,
        }
    }

    /// The session client's user agent (a current desktop Chrome before a
    /// session exists), so on-demand image fetches look like the sync's.
    pub fn user_agent(&self) -> String {
        self.user_agent
            .lock()
            .ok()
            .and_then(|u| u.clone())
            .unwrap_or_else(|| flek_core::api::ClientProfile::detect().user_agent)
    }

    /// Client of the ready session.
    pub fn client(&self) -> Option<Arc<Client>> {
        self.client.lock().ok().and_then(|c| c.clone())
    }

    /// Stops the sync engine (quit, or before a new session starts).
    pub fn stop_engine(&self) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(stop) = self.engine_stop.lock().ok().and_then(|mut s| s.take()) {
            let _ = stop.send(true);
        }
    }
}

fn startup_settings(path: flek_core::Result<std::path::PathBuf>) -> flek_core::settings::Settings {
    let mut settings = flek_core::settings::Settings::safe_defaults();
    match path {
        Ok(path) => settings.reload(&path),
        Err(e) => {
            tracing::warn!(error = %e, "cannot locate settings; desktop auto-import disabled");
            settings.load_error = Some("settings_unavailable".into());
        }
    }
    settings
}

pub fn set_status(app: &AppHandle, status: Status) {
    let state = app.state::<AppState>();
    if let Ok(mut s) = state.status.lock() {
        *s = status.clone();
    }
    let _ = app.emit(STATUS_EVENT, status);
    crate::tray::refresh(app);
}

/// Forwards the change bus to the UI and remembers the latest progress.
pub fn forward_changes(app: &AppHandle) {
    let mut rx = app.state::<AppState>().bus.subscribe();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let change = match rx.recv().await {
                Ok(c) => c,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!(n, "UI forwarder lagged");
                    continue;
                }
                Err(_) => break,
            };
            match &change {
                Change::Progress(p) => {
                    if let Ok(mut slot) = app.state::<AppState>().progress.lock() {
                        *slot = Some(p.clone());
                    }
                    crate::tray::refresh(&app);
                }
                Change::Connection(c) => {
                    if let Ok(mut slot) = app.state::<AppState>().connection.lock() {
                        *slot = Some(*c);
                    }
                    crate::tray::refresh(&app);
                }
                Change::SettingsError { error } => {
                    let state = app.state::<AppState>();
                    if let Ok(mut settings) = state.settings.lock() {
                        settings.load_error = error.clone();
                        if error.is_some() {
                            settings.desktop_import_on_start = false;
                        }
                        let _ = app.emit("flek://settings", settings.clone());
                    };
                }
                _ => {}
            }
            let _ = app.emit(CHANGE_EVENT, &change);
        }
    });
}

/// First thing after launch: a stored session (offline-tolerant), else the
/// Slack desktop import, else the sign-in screen.
pub async fn boot(app: AppHandle) {
    let state = app.state::<AppState>();
    if let Err(e) = flek_core::sync::presence::stale(&state.store, &state.bus).await {
        tracing::warn!(error = %e, "cannot invalidate cached presence");
    }
    let stored = state
        .creds
        .list()
        .map(|ids| !ids.is_empty())
        .unwrap_or(false);
    if stored {
        match auth::resume_session(
            &state.creds,
            Some(&state.store),
            None,
            state.settings().desktop_import_on_start,
        )
        .await
        {
            Ok(session) => return start(&app, session),
            Err(e) => {
                tracing::warn!("stored session unusable: {e}");
                set_status(
                    &app,
                    Status::SignIn {
                        desktop: desktop_installed(),
                        reason: Some(e.to_string()),
                    },
                );
                return;
            }
        }
    }
    if !state.settings().desktop_import_on_start {
        set_status(
            &app,
            Status::SignIn {
                desktop: desktop_installed(),
                reason: None,
            },
        );
        return;
    }
    import_desktop_with_policy(app, false).await;
}

/// Imports the Slack desktop session, or falls back to the sign-in screen.
pub async fn import_desktop(app: AppHandle) {
    let state = app.state::<AppState>();
    let _maintenance = state.maintenance.lock().await;
    import_desktop_with_policy(app.clone(), true).await;
}

async fn import_desktop_with_policy(app: AppHandle, explicit: bool) {
    if !desktop_installed() {
        set_status(
            &app,
            Status::SignIn {
                desktop: false,
                reason: None,
            },
        );
        return;
    }
    set_status(&app, Status::Importing);
    let state = app.state::<AppState>();
    let imported = if explicit {
        match flek_core::settings::default_path() {
            Ok(path) => {
                let imported = auth::import_explicit_and_save(&state.creds, None, &path).await;
                let settings = {
                    let mut settings = state.settings.lock().expect("settings lock");
                    settings.reload(&path);
                    settings.clone()
                };
                let _ = app.emit("flek://settings", settings);
                imported
            }
            Err(error) => Err(error),
        }
    } else {
        auth::import_and_save(&state.creds, None).await
    };
    let reason = match imported {
        Ok(teams) => {
            let mut last_err = None;
            for t in teams {
                match t.result {
                    Ok(session) => return start(&app, session),
                    Err(e) => last_err = Some(e.to_string()),
                }
            }
            last_err
        }
        Err(e) => Some(e.to_string()),
    };
    set_status(
        &app,
        Status::SignIn {
            desktop: true,
            reason,
        },
    );
}

pub(crate) fn desktop_installed() -> bool {
    // Where import is not implemented, sign-in is the only path.
    auth::desktop_import::IMPORT_SUPPORTED
        && dirs::home_dir().is_some_and(|home| {
            auth::desktop_import::candidate_roots(&home)
                .iter()
                .any(|p| p.is_dir())
        })
}

/// Session ready: show it, start syncing in the background.
pub fn start(app: &AppHandle, session: Session) {
    start_with_options(app, session, false, false);
}

pub fn start_with_options(app: &AppHandle, session: Session, full: bool, skip_files: bool) {
    let state = app.state::<AppState>();
    state.stop_engine();
    let (client, write_client) = match session_clients(&session) {
        Ok(clients) => clients,
        Err(e) => {
            tracing::error!("cannot build the Slack client: {e}");
            state.forget_session();
            set_status(
                app,
                Status::SignIn {
                    desktop: desktop_installed(),
                    reason: Some(e.to_string()),
                },
            );
            return;
        }
    };
    *state.session.lock().expect("session lock") = Some(session.clone());
    *state.client.lock().expect("client lock") = Some(write_client);
    let me = session.identity.clone();
    state.presence.send_replace(vec![me.user_id.clone()]);
    set_status(
        app,
        Status::Ready {
            team_id: me.team_id.clone(),
            team: me.team.clone(),
            user_id: me.user_id.clone(),
            user: me.user.clone(),
        },
    );

    if let Ok(mut ua) = state.user_agent.lock() {
        *ua = Some(client.profile().user_agent.clone());
    }
    let files = flek_core::files::default_cache_dir()
        .ok()
        .map(|dir| (dir, state.settings().max_file_bytes));
    let engine = Engine {
        client,
        store: state.store.clone(),
        me,
        bus: state.bus.clone(),
        priority: state.priority.clone(),
        options: EngineOptions {
            files,
            follow: true,
            full,
            skip_files_first_pass: skip_files,
            settings_path: flek_core::settings::default_path().ok(),
            settings_cache: Arc::clone(&state.settings),
            follow_config: flek_core::sync::follow::FollowConfig {
                presence: Some(state.presence.subscribe()),
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let (stop_tx, stop_rx) = watch::channel(false);
    if let Ok(mut slot) = state.engine_stop.lock() {
        *slot = Some(stop_tx);
    }
    let generation = state.generation.load(std::sync::atomic::Ordering::SeqCst);
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    *state.engine_done.lock().expect("engine done lock") = Some(done_rx);
    let app = app.clone();
    // The engine gets its own thread and runtime: its future is not `Send`,
    // and sync work never competes with the UI's IPC this way.
    let recovery_credentials = session.credentials.clone();
    let spawned = std::thread::Builder::new()
        .name("flek-sync".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => return tracing::error!("cannot start the sync runtime: {e}"),
            };
            let mut cancel = stop_rx.clone();
            let result = rt.block_on(async {
                tokio::select! {
                    biased;
                    _ = cancel.changed() => Ok(Default::default()),
                    result = engine.run(stop_rx) => result,
                }
            });
            // Awaiting maintenance must also wait for cancelled blocking I/O.
            drop(rt);
            let _ = done_tx.send(());
            match result {
                Ok(_) => tracing::info!("sync engine stopped"),
                Err(e) if e.is_auth_error() => {
                    tracing::warn!("session revoked: {e}");
                    // Slack desktop may hold a fresh session; else sign in again.
                    if app
                        .state::<AppState>()
                        .generation
                        .load(std::sync::atomic::Ordering::SeqCst)
                        == generation
                    {
                        tauri::async_runtime::spawn(async move {
                            let state = app.state::<AppState>();
                            let _maintenance = state.maintenance.lock().await;
                            if state.generation.load(std::sync::atomic::Ordering::SeqCst)
                                == generation
                            {
                                match auth::recover_rejected(
                                    &state.creds,
                                    &recovery_credentials,
                                    state.settings().desktop_import_on_start,
                                )
                                .await
                                {
                                    Ok(session) => start(&app, session),
                                    Err(error) => {
                                        state.forget_session();
                                        set_status(
                                            &app,
                                            Status::SignIn {
                                                desktop: desktop_installed(),
                                                reason: Some(error.to_string()),
                                            },
                                        );
                                    }
                                }
                            }
                        });
                    }
                }
                Err(e) => tracing::error!("sync engine failed: {e}"),
            }
        });
    if let Err(e) = spawned {
        tracing::error!("cannot start the sync thread: {e}");
    }
}

fn session_clients(session: &Session) -> flek_core::Result<(Client, Arc<Client>)> {
    Ok((session.client()?, Arc::new(session.client()?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_startup_settings_disable_import_and_preserve_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, b"{broken").unwrap();
        let settings = startup_settings(Ok(path.clone()));
        assert!(!settings.desktop_import_on_start);
        assert_eq!(settings.load_error.as_deref(), Some("settings_unavailable"));
        assert!(settings.save(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
        assert!(
            !startup_settings(Err(flek_core::Error::Store("no path".into())))
                .desktop_import_on_start
        );
    }
}
