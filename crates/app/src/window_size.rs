//! Main-window size only: native, DPI-independent, restored before first show.
//! No position, account data or preferences UI lives in this file.
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, LogicalSize, Manager, Window};

const FILE: &str = "window-size.json";
// Keep in step with the main window's minimum in tauri.conf.json.
const MIN_WIDTH: f64 = 720.0;
const MIN_HEIGHT: f64 = 480.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct SavedSize {
    width: f64,
    height: f64,
}

impl SavedSize {
    fn valid(self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width > 0.0 && self.height > 0.0
    }

    fn fit(self, screen: LogicalSize<f64>) -> Self {
        Self {
            width: self.width.clamp(MIN_WIDTH, screen.width.max(MIN_WIDTH)),
            height: self.height.clamp(MIN_HEIGHT, screen.height.max(MIN_HEIGHT)),
        }
    }
}

struct Pending {
    size: Option<SavedSize>,
    written: Option<SavedSize>,
    revision: u64,
}

struct WindowSize {
    path: PathBuf,
    pending: Mutex<Pending>,
}

impl WindowSize {
    fn load(path: PathBuf) -> Self {
        let size = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SavedSize>(&bytes).ok())
            .filter(|size| size.valid());
        Self {
            path,
            pending: Mutex::new(Pending {
                size,
                written: size,
                revision: 0,
            }),
        }
    }

    fn remember(&self, size: SavedSize) -> Option<u64> {
        if !size.valid() {
            return None;
        }
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending.size == Some(size) {
            return None;
        }
        pending.size = Some(size);
        pending.revision += 1;
        Some(pending.revision)
    }

    /// A revision avoids writing intermediate sizes from an active resize.
    fn flush(&self, revision: Option<u64>) -> std::io::Result<()> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if revision.is_some_and(|r| r != pending.revision) || pending.size == pending.written {
            return Ok(());
        }
        let Some(size) = pending.size else {
            return Ok(());
        };
        let bytes = serde_json::to_vec(&size)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // A partial write must not destroy the previously saved dimensions.
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(temporary, &self.path)?;
        pending.written = Some(size);
        Ok(())
    }
}

pub fn restore(app: &AppHandle) {
    let Ok(directory) = app.path().app_config_dir() else {
        return;
    };
    let state = WindowSize::load(directory.join(FILE));
    let saved = state.pending.lock().unwrap_or_else(|e| e.into_inner()).size;
    app.manage(state);
    let (Some(saved), Some(window)) = (saved, app.get_webview_window(crate::MAIN)) else {
        return;
    };
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let size = monitor.map_or(saved, |monitor| {
        let available = monitor
            .work_area()
            .size
            .to_logical::<f64>(monitor.scale_factor());
        // Leave room for native decorations outside the saved client area.
        let decorations = window
            .outer_size()
            .ok()
            .zip(window.inner_size().ok())
            .map(|(outer, inner)| {
                LogicalSize::new(
                    f64::from(outer.width.saturating_sub(inner.width)) / monitor.scale_factor(),
                    f64::from(outer.height.saturating_sub(inner.height)) / monitor.scale_factor(),
                )
            })
            .unwrap_or(LogicalSize::new(0.0, 0.0));
        saved.fit(LogicalSize::new(
            available.width - decorations.width,
            available.height - decorations.height,
        ))
    });
    if let Err(e) = window.set_size(LogicalSize::new(size.width, size.height)) {
        tracing::warn!("cannot restore window size: {e}");
    }
}

/// Ignore minimized/fullscreen/maximized sizes so they cannot replace the normal size.
pub fn capture(window: &Window) {
    if window.label() != crate::MAIN
        || window.is_minimized().unwrap_or(true)
        || window.is_maximized().unwrap_or(true)
        || window.is_fullscreen().unwrap_or(true)
    {
        return;
    }
    let (Ok(physical), Ok(scale)) = (window.inner_size(), window.scale_factor()) else {
        return;
    };
    if !scale.is_finite() || scale <= 0.0 {
        return;
    }
    let logical = physical.to_logical::<f64>(scale);
    let app = window.app_handle();
    let Some(state) = app.try_state::<WindowSize>() else {
        return;
    };
    let Some(revision) = state.remember(SavedSize {
        width: logical.width,
        height: logical.height,
    }) else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        if let Some(state) = app.try_state::<WindowSize>() {
            if let Err(e) = state.flush(Some(revision)) {
                tracing::warn!("cannot save window size: {e}");
            }
        }
    });
}

/// Flush on hide/quit, even if the debounce has not elapsed yet.
pub fn save(app: &AppHandle) {
    if let Some(state) = app.try_state::<WindowSize>() {
        if let Err(e) = state.flush(None) {
            tracing::warn!("cannot save window size: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_size_survives_restart_and_stale_resize_cannot_overwrite_it() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(FILE);
        let state = WindowSize::load(path.clone());
        let old = state
            .remember(SavedSize {
                width: 900.0,
                height: 600.0,
            })
            .unwrap();
        let last = SavedSize {
            width: 1250.0,
            height: 820.0,
        };
        state.remember(last);
        state.flush(Some(old)).unwrap();
        assert!(!path.exists());
        state.flush(None).unwrap();
        let restarted = WindowSize::load(path);
        assert_eq!(restarted.pending.lock().unwrap().size, Some(last));
        assert!(state
            .remember(SavedSize {
                width: 0.0,
                height: 0.0
            })
            .is_none());
    }

    #[test]
    fn corrupt_state_falls_back_and_saved_size_fits_a_different_display() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(FILE);
        std::fs::write(&path, b"broken").unwrap();
        assert!(WindowSize::load(path)
            .pending
            .lock()
            .unwrap()
            .size
            .is_none());
        let logical = tauri::PhysicalSize::new(2400, 1600).to_logical::<f64>(2.0);
        assert_eq!(
            SavedSize {
                width: logical.width,
                height: logical.height
            },
            SavedSize {
                width: 1200.0,
                height: 800.0
            }
        );
        assert_eq!(
            SavedSize {
                width: 1800.0,
                height: 1000.0
            }
            .fit(LogicalSize::new(1280.0, 720.0)),
            SavedSize {
                width: 1280.0,
                height: 720.0
            }
        );
        assert_eq!(
            SavedSize {
                width: 100.0,
                height: 100.0
            }
            .fit(LogicalSize::new(1280.0, 720.0)),
            SavedSize {
                width: MIN_WIDTH,
                height: MIN_HEIGHT
            }
        );
    }
}
