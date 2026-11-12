//! View menu zoom: scales the main webview like a browser page and is
//! remembered in settings, so the next launch opens at the same size.
use flek_core::settings::ZOOM_STEPS;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoom {
    In,
    Out,
    Reset,
}

fn next(current: u16, zoom: Zoom) -> u16 {
    let first = ZOOM_STEPS[0];
    let last = ZOOM_STEPS[ZOOM_STEPS.len() - 1];
    match zoom {
        Zoom::Reset => 100,
        Zoom::In => ZOOM_STEPS
            .into_iter()
            .find(|s| *s > current)
            .unwrap_or(last),
        Zoom::Out => ZOOM_STEPS
            .into_iter()
            .rev()
            .find(|s| *s < current)
            .unwrap_or(first),
    }
}

fn set(app: &AppHandle, percent: u16) {
    if let Some(window) = app.get_webview_window(crate::MAIN) {
        if let Err(e) = window.set_zoom(f64::from(percent) / 100.0) {
            tracing::warn!("cannot set zoom: {e}");
        }
    }
}

/// Restores the saved zoom before the window is shown.
pub fn apply(app: &AppHandle) {
    set(app, app.state::<AppState>().settings().zoom_percent);
}

pub fn change(app: &AppHandle, zoom: Zoom) {
    let state = app.state::<AppState>();
    let Ok(mut current) = state.settings.lock() else {
        return;
    };
    let percent = next(current.zoom_percent, zoom);
    if percent == current.zoom_percent {
        return;
    }
    let mut settings = current.clone();
    settings.zoom_percent = percent;
    // Zoom still applies for this session if the file cannot be written.
    if let Err(e) = flek_core::settings::default_path().and_then(|path| settings.save(&path)) {
        tracing::warn!("cannot save zoom: {e}");
    }
    *current = settings.clone();
    drop(current);
    let _ = app.emit("flek://settings", settings);
    set(app, percent);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stop_at_the_ends_and_snap_unknown_values() {
        assert_eq!(next(100, Zoom::In), 110);
        assert_eq!(next(100, Zoom::Out), 90);
        assert_eq!(next(200, Zoom::In), 200);
        assert_eq!(next(50, Zoom::Out), 50);
        assert_eq!(next(150, Zoom::Reset), 100);
        assert_eq!(next(105, Zoom::In), 110);
        assert_eq!(next(105, Zoom::Out), 100);
    }
}
