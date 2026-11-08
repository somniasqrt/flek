//! Menu bar icon: sync status, open window, launch at login, quit. Flek keeps
//! syncing after its window is closed; quitting is done from here.

use flek_core::sync::live::{ConnectionState, Progress};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

use crate::state::{AppState, Status};

struct TrayItems {
    status: MenuItem<Wry>,
    autostart: CheckMenuItem<Wry>,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "Starting…", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Flek", true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Open at Login",
        true,
        crate::autostart::is_enabled(app),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit Flek", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .icon_as_template(true)
        .tooltip("Flek")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => crate::show_main(app),
            "autostart" => {
                let on = !crate::autostart::is_enabled(app);
                crate::autostart::set(app, on);
                refresh(app);
            }
            "quit" => {
                app.state::<AppState>().stop_engine();
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    app.manage(TrayItems { status, autostart });
    refresh(app);
    Ok(())
}

/// Re-renders the status line and the launch-at-login check.
pub fn refresh(app: &AppHandle) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let state = app.state::<AppState>();
    let _ = items.status.set_text(status_line(
        &state.status(),
        state.progress().as_ref(),
        state.connection(),
    ));
    let _ = items
        .autostart
        .set_checked(crate::autostart::is_enabled(app));
}

fn status_line(
    status: &Status,
    progress: Option<&Progress>,
    connection: Option<ConnectionState>,
) -> String {
    match status {
        Status::Starting => "Starting…".into(),
        Status::Importing => "Importing the Slack desktop session…".into(),
        Status::SignIn { .. } => "Not signed in".into(),
        Status::SigningIn => "Signing in…".into(),
        Status::Ready { team, .. } => {
            if let Some(ConnectionState::Reconnecting { .. }) = connection {
                return format!("{team}: offline, reconnecting…");
            }
            match progress {
                Some(Progress::Directory) => format!("{team}: syncing channels and people…"),
                Some(Progress::Conversations {
                    done,
                    total,
                    deep: false,
                    ..
                }) => format!("{team}: syncing messages {done}/{total}"),
                Some(Progress::Conversations {
                    done,
                    total,
                    deep: true,
                    ..
                }) => format!("{team}: fetching older history {done}/{total}"),
                Some(Progress::Files { done, total }) => {
                    format!("{team}: downloading files {done}/{total}")
                }
                Some(Progress::Idle) | None => format!("{team}: up to date"),
            }
        }
    }
}
