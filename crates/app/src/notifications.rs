//! OS delivery and native focus. Notification policy and DB collection live in core.
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use flek_core::notifications::{self, Context, Target};
use flek_core::sync::live::{Change, ConnectionState};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

use crate::state::{AppState, Status};

#[derive(Default)]
pub struct DesktopNotifications {
    conversation: Option<String>,
    pending: Option<Target>,
    next_id: i32,
    // A bounded registry: action payloads are resolved to targets we delivered.
    delivered: std::collections::VecDeque<(i32, Target)>,
}

#[tauri::command]
pub fn notification_context(app: AppHandle, conversation_id: Option<String>) {
    app.state::<Mutex<DesktopNotifications>>()
        .lock()
        .expect("notification lock")
        .conversation = conversation_id;
}

#[tauri::command]
pub fn notification_target(app: AppHandle, state: State<'_, AppState>) -> Option<Target> {
    let target = app
        .state::<Mutex<DesktopNotifications>>()
        .lock()
        .ok()?
        .pending
        .take()?;
    (state.team_id().as_deref() == Some(&target.team_id)).then_some(target)
}

fn focused_conversation(app: &AppHandle) -> Option<String> {
    let window = app.get_webview_window(crate::MAIN)?;
    if !window.is_focused().unwrap_or(false)
        || !window.is_visible().unwrap_or(false)
        || window.is_minimized().unwrap_or(true)
    {
        return None;
    }
    app.state::<Mutex<DesktopNotifications>>()
        .lock()
        .ok()?
        .conversation
        .clone()
}

fn badge(app: &AppHandle, count: i64) {
    if let Some(window) = app.get_webview_window(crate::MAIN) {
        let _ = window.set_badge_count((count > 0).then_some(count));
    }
    #[cfg(target_os = "macos")]
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_title(Some(if count > 0 {
            count.to_string()
        } else {
            String::new()
        }));
    }
}

pub fn clear(app: &AppHandle) {
    badge(app, 0);
    if let Ok(mut state) = app.state::<Mutex<DesktopNotifications>>().lock() {
        state.pending = None;
        state.delivered.clear();
        state.conversation = None;
    }
}

pub fn start(app: &AppHandle) -> anyhow::Result<()> {
    app.manage(Mutex::new(DesktopNotifications::default()));
    let handle = app.clone();
    app.notification().on_action(move |action| {
        let Some(notification) = action.notification() else {
            return;
        };
        let state = handle.state::<Mutex<DesktopNotifications>>();
        let target = state.lock().ok().and_then(|s| {
            s.delivered
                .iter()
                .find(|(id, _)| *id == notification.id())
                .map(|(_, target)| target.clone())
        });
        if let Some(target) = target {
            if handle.state::<AppState>().team_id().as_deref() != Some(&target.team_id) {
                return;
            }
            if let Ok(mut state) = state.lock() {
                state.pending = Some(target);
            }
            crate::show_main(&handle);
            let _ = handle.emit("flek://notification-open", ());
        }
    })?;
    let state = app.state::<AppState>();
    let mut cursor = state.store.with_read(notifications::watermark)?;
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let since = format!("{}.{:06}", started.as_secs(), started.subsec_micros());
    let mut receiver = state.bus.subscribe();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut catching_up = true;
        let mut dirty = false;
        let mut badge_dirty = true;
        let mut timer = tokio::time::interval(Duration::from_millis(600));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                event = receiver.recv() => {
                    if matches!(&event, Ok(Change::History { .. } | Change::Message { .. } | Change::MessageDeleted { .. } | Change::ReadMarker { .. } | Change::Directory)) {
                        badge_dirty = true;
                    }
                    match event {
                    Ok(Change::Connection(ConnectionState::Connected | ConnectionState::Reconnecting { .. } | ConnectionState::SyncIncomplete { .. })) => {
                        catching_up = true;
                        dirty = true;
                    }
                    Ok(Change::CaughtUp) => { catching_up = false; dirty = true; }
                    Ok(Change::History { .. } | Change::Message { .. } | Change::Presence | Change::Directory) => dirty = true,
                    Ok(_) => {},
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // IDs in SQLite recover messages missed on the invalidation bus.
                        dirty = true;
                        badge_dirty = true;
                    }
                    Err(_) => break,
                    }
                },
                _ = timer.tick(), if badge_dirty || dirty && !catching_up => {
                    let state = app.state::<AppState>();
                    let status = state.status();
                    let Status::Ready { team_id, user_id, .. } = status.clone() else { continue };
                    if badge_dirty {
                        let team = team_id.clone();
                        if let Ok(count) = state.store.read(move |conn| notifications::badge_count(conn, &team)).await {
                            if state.team_id().as_deref() == Some(&team_id) { badge(&app, count); }
                            badge_dirty = false;
                        }
                    }
                    if catching_up || !dirty { continue; }
                    let settings = state.settings();
                    let focused = focused_conversation(&app);
                    let (team, user, preferences, focus, cutoff) =
                        (team_id.clone(), user_id, settings.clone(), focused.clone(), since.clone());
                    let result = state.store.read(move |conn| {
                        let presence = flek_core::store::presence::get(conn, &user)?;
                        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
                        notifications::collect(conn, cursor, &cutoff, &team, &preferences, &Context {
                            user_id: &user, focused_conversation: focus.as_deref(), presence: presence.as_ref(), now,
                        })
                    }).await;
                    let Ok((until, batch)) = result else {
                        tracing::warn!("cannot read notification candidates");
                        continue;
                    };
                    // A focus/preferences/account change during the read must be re-evaluated.
                    if state.status() != status
                        || settings != state.settings() || focused != focused_conversation(&app) { continue; }
                    cursor = until;
                    dirty = false;
                    if let Some(message) = batch.latest {
                        let title = if settings.notification_preview {
                            if message.author.is_empty() { message.conversation.clone() }
                            else { format!("{} — {}", message.author, message.conversation) }
                        } else { "Flek".into() };
                        let body = if batch.count > 1 {
                            format!("{}: {}", crate::i18n::t(settings.language, crate::i18n::Key::NewMessages), batch.count)
                        } else if settings.notification_preview && !message.text.is_empty() {
                            message.text
                        } else {
                            crate::i18n::t(settings.language, crate::i18n::Key::NewMessage).into()
                        };
                        let id = {
                            let registry = app.state::<Mutex<DesktopNotifications>>();
                            let mut registry = registry.lock().expect("notification lock");
                            registry.next_id = registry.next_id.checked_add(1).unwrap_or(1);
                            let id = registry.next_id;
                            registry.delivered.push_back((id, message.target));
                            if registry.delivered.len() > 100 { registry.delivered.pop_front(); }
                            id
                        };
                        let mut notification = app.notification().builder().id(id).title(title).body(body);
                        if settings.notification_sound { notification = notification.sound("Glass"); }
                        if notification.show().is_err() { tracing::warn!("cannot deliver notification"); }
                    }
                }
            }
        }
    });
    Ok(())
}
