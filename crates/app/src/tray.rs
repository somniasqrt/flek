//! Menu bar icon: sync status, open window, launch at login, quit. Flek keeps
//! syncing after its window is closed; quitting is done from here.

use crate::i18n::{t, Key};
use flek_core::sync::live::{ConnectionState, Progress};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::Emitter;
use tauri::{AppHandle, Manager, Wry};

use crate::state::{AppState, Status};

struct TrayItems {
    status: MenuItem<Wry>,
    autostart: CheckMenuItem<Wry>,
    open: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    preferences: MenuItem<Wry>,
    language: std::sync::Mutex<flek_core::settings::Language>,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let lang = app.state::<AppState>().settings().language;
    let preferences = MenuItem::with_id(
        app,
        "preferences",
        t(lang, Key::Preferences),
        true,
        None::<&str>,
    )?;
    rebuild_native(app)?;
    app.on_menu_event(|app, event| {
        if event.id().as_ref() == "preferences" {
            crate::show_main(app);
            let _ = app.emit("flek://preferences", ());
        } else if event.id().as_ref() == "quit" {
            app.exit(0);
        }
    });
    let status = MenuItem::with_id(app, "status", t(lang, Key::Starting), false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", t(lang, Key::Open), true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        t(lang, Key::Login),
        true,
        crate::autostart::is_enabled(app),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", t(lang, Key::Quit), true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &preferences,
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
                let state = app.state::<AppState>();
                let mut settings = state.settings();
                settings.launch_at_login = on;
                let _ = crate::commands::set_settings(app.clone(), state, settings);
            }
            "preferences" => {
                crate::show_main(app);
                let _ = app.emit("flek://preferences", ());
            }
            "quit" => {
                app.state::<AppState>().stop_engine();
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    app.manage(TrayItems {
        status,
        autostart,
        open,
        quit,
        preferences,
        language: std::sync::Mutex::new(lang),
    });
    refresh(app);
    Ok(())
}

/// Re-renders the status line and the launch-at-login check.
pub fn refresh(app: &AppHandle) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let state = app.state::<AppState>();
    let lang = state.settings().language;
    let _ = items.open.set_text(t(lang, Key::Open));
    let _ = items.quit.set_text(t(lang, Key::Quit));
    let _ = items.autostart.set_text(t(lang, Key::Login));
    let _ = items.preferences.set_text(t(lang, Key::Preferences));
    if let Ok(mut previous) = items.language.lock() {
        if *previous != lang && rebuild_native(app).is_ok() {
            *previous = lang;
        }
    }
    let _ = items.status.set_text(status_line(
        lang,
        &state.status(),
        state.progress().as_ref(),
        state.connection(),
    ));
    let _ = items
        .autostart
        .set_checked(crate::autostart::is_enabled(app));
}

fn status_line(
    lang: flek_core::settings::Language,
    status: &Status,
    progress: Option<&Progress>,
    connection: Option<ConnectionState>,
) -> String {
    match status {
        Status::Starting => t(lang, Key::Starting).into(),
        Status::Importing => t(lang, Key::Importing).into(),
        Status::SignIn { .. } => t(lang, Key::SignedOut).into(),
        Status::SigningIn => t(lang, Key::SigningIn).into(),
        Status::Ready { team, .. } => {
            if let Some(ConnectionState::Reconnecting { .. }) = connection {
                return format!("{team}: {}", t(lang, Key::Offline));
            }
            match progress {
                Some(Progress::Directory) => format!("{team}: {}", t(lang, Key::Directory)),
                Some(Progress::Conversations {
                    done,
                    total,
                    deep: false,
                    ..
                }) => format!("{team}: {} {done}/{total}", t(lang, Key::Messages)),
                Some(Progress::Conversations {
                    done,
                    total,
                    deep: true,
                    ..
                }) => format!("{team}: {} {done}/{total}", t(lang, Key::History)),
                Some(Progress::Files { done, total }) => {
                    format!("{team}: {} {done}/{total}", t(lang, Key::Files))
                }
                Some(Progress::Idle) | None => format!("{team}: {}", t(lang, Key::Current)),
            }
        }
    }
}

/// Rebuild standard native items too: their labels follow the app language,
/// while predefined edit actions retain macOS responder-chain behavior.
fn rebuild_native(app: &AppHandle) -> tauri::Result<()> {
    let lang = app.state::<AppState>().settings().language;
    let preferences = MenuItem::with_id(
        app,
        "preferences",
        t(lang, Key::Preferences),
        true,
        Some("CmdOrCtrl+,"),
    )?;
    let app_menu = Submenu::with_items(
        app,
        "Flek",
        true,
        &[
            &PredefinedMenuItem::about(app, Some(t(lang, Key::About)), None)?,
            &PredefinedMenuItem::separator(app)?,
            &preferences,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some(t(lang, Key::Hide)))?,
            &PredefinedMenuItem::hide_others(app, Some(t(lang, Key::HideOthers)))?,
            &PredefinedMenuItem::show_all(app, Some(t(lang, Key::ShowAll)))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", t(lang, Key::Quit), true, Some("CmdOrCtrl+Q"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        t(lang, Key::Edit),
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(t(lang, Key::Undo)))?,
            &PredefinedMenuItem::redo(app, Some(t(lang, Key::Redo)))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some(t(lang, Key::Cut)))?,
            &PredefinedMenuItem::copy(app, Some(t(lang, Key::Copy)))?,
            &PredefinedMenuItem::paste(app, Some(t(lang, Key::Paste)))?,
            &PredefinedMenuItem::select_all(app, Some(t(lang, Key::SelectAll)))?,
        ],
    )?;
    app.set_menu(Menu::with_items(app, &[&app_menu, &edit])?)?;
    Ok(())
}
