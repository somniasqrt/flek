//! Versioned, non-secret preferences shared by desktop and sync.
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    System,
    En,
    Uk,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSize {
    Small,
    #[default]
    Default,
    Large,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DmSort {
    #[default]
    Recent,
    Name,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// Runtime diagnostic for IPC; never accepted from or persisted to disk.
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub load_error: Option<String>,
    /// App-owned migration marker; never supplied by page input.
    pub webview_cleanup_version: u32,
    pub language: Language,
    pub theme: Theme,
    pub text_size: TextSize,
    /// None follows the resolved language's clock convention.
    pub clock_24: Option<bool>,
    pub launch_at_login: bool,
    pub desktop_import_on_start: bool,
    pub keep_in_menu_bar: bool,
    pub dm_sort: DmSort,
    pub show_muted: bool,
    pub presence_dots: bool,
    pub link_previews: bool,
    pub image_previews: bool,
    /// None = unlimited, zero = disabled, otherwise bytes.
    pub max_file_bytes: Option<i64>,
    pub refresh_days: u32,
    pub refresh_hours: u32,
    /// App-owned page zoom from the View menu, one of `ZOOM_STEPS`.
    pub zoom_percent: u16,
}

/// Zoom levels in percent, as in browsers and the Slack desktop app.
pub const ZOOM_STEPS: [u16; 11] = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200];

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            load_error: None,
            webview_cleanup_version: 0,
            language: Language::System,
            theme: Theme::System,
            text_size: TextSize::Default,
            clock_24: None,
            launch_at_login: false,
            desktop_import_on_start: true,
            keep_in_menu_bar: true,
            dm_sort: DmSort::Recent,
            show_muted: true,
            presence_dots: true,
            link_previews: true,
            image_previews: true,
            max_file_bytes: Some(100 * 1024 * 1024),
            refresh_days: 7,
            refresh_hours: 3,
            zoom_percent: 100,
        }
    }
}

impl Settings {
    /// Used when preferences cannot be read; never silently import a session.
    pub fn safe_defaults() -> Self {
        Self {
            desktop_import_on_start: false,
            ..Self::default()
        }
    }

    /// Retains the last good sync preferences and reports an error once per
    /// failure episode. A successful reload clears the diagnostic.
    pub fn reload(&mut self, path: &Path) {
        match Self::load(path) {
            Ok(settings) => *self = settings,
            Err(e) => {
                if self.load_error.is_none() {
                    tracing::warn!(error = %e, "cannot read settings; retaining safe preferences");
                }
                self.desktop_import_on_start = false;
                self.load_error = Some("settings_unavailable".into());
            }
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.refresh_days > 30
            || ![1, 3, 12, 24].contains(&self.refresh_hours)
            || !ZOOM_STEPS.contains(&self.zoom_percent)
            || ![
                None,
                Some(0),
                Some(10 << 20),
                Some(25 << 20),
                Some(100 << 20),
            ]
            .contains(&self.max_file_bytes)
        {
            return Err(Error::Store("invalid_settings".into()));
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            if let Some(parent) = path.parent() {
                crate::private_fs::dir(parent)?;
            }
            crate::private_fs::mode(path, 0o600)?;
        }
        let value = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        value.validate()?;
        Ok(value)
    }

    /// An explicit desktop import opts back into automatic session recovery.
    /// Loading first preserves invalid/unreadable preferences without replacing them.
    pub fn enable_desktop_import(path: &Path) -> Result<Self> {
        let mut settings = Self::load(path)?;
        if !settings.desktop_import_on_start {
            settings.desktop_import_on_start = true;
            settings.save(path)?;
        }
        Ok(settings)
    }

    /// A unique temporary file, fsync, and rename preserve the last valid file
    /// if a write fails. No credentials belong in this file.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        // Refuse to replace unreadable/invalid preferences, including writes
        // from startup migrations and stale in-memory UI snapshots.
        if path.try_exists()? {
            Self::load(path)?;
        }
        let parent = path
            .parent()
            .ok_or_else(|| Error::Store("invalid settings path".into()))?;
        crate::private_fs::dir(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        crate::private_fs::mode(temp.path(), 0o600)?;
        let mut persisted = self.clone();
        persisted.load_error = None;
        temp.write_all(&serde_json::to_vec_pretty(&persisted)?)?;
        temp.as_file().sync_all()?;
        temp.persist(path).map_err(|e| Error::Io(e.error))?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    }

    pub fn apply_sync(&self, options: &mut crate::sync::engine::EngineOptions, cache: PathBuf) {
        options.files = Some((cache, self.max_file_bytes));
        options.refresh_days = self.refresh_days;
        options.refresh_every =
            std::time::Duration::from_secs(u64::from(self.refresh_hours) * 3600);
    }
}

pub fn default_path() -> Result<PathBuf> {
    Ok(crate::store::default_path()?.with_file_name("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_warning_is_once_per_failure_episode() {
        #[derive(Clone)]
        struct Output(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Output {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let output = Output(std::sync::Arc::default());
        let writer = output.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        // With a single dispatcher, tracing asks the thread that reaches the
        // callsite first; a parallel test without a subscriber would disable it.
        let _second = tracing::Dispatch::new(tracing_subscriber::registry());
        tracing::callsite::rebuild_interest_cache();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = Settings::safe_defaults();
        std::fs::write(&path, b"{broken").unwrap();
        settings.reload(&path);
        settings.reload(&path);
        let warnings = || {
            String::from_utf8(output.0.lock().unwrap().clone())
                .unwrap()
                .matches("cannot read settings;")
                .count()
        };
        assert_eq!(warnings(), 1);
        std::fs::remove_file(&path).unwrap();
        Settings::default().save(&path).unwrap();
        settings.reload(&path);
        std::fs::write(&path, b"broken again").unwrap();
        settings.reload(&path);
        assert_eq!(warnings(), 2);
    }

    #[test]
    fn fallback_disables_import_retains_preferences_and_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut cached = Settings::safe_defaults();
        for bytes in [
            b"{broken".as_slice(),
            br#"{"version":9}"#,
            br#"{"max_file_bytes":-1}"#,
        ] {
            std::fs::write(&path, bytes).unwrap();
            cached.reload(&path);
            assert!(!cached.desktop_import_on_start);
            assert!(cached.load_error.is_some());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        std::fs::remove_file(&path).unwrap();
        let good = Settings {
            refresh_days: 3,
            theme: Theme::Dark,
            ..Default::default()
        };
        good.save(&path).unwrap();
        cached.reload(&path);
        assert_eq!(cached, good);
        std::fs::write(&path, b"invalid").unwrap();
        cached.reload(&path);
        assert_eq!(cached.refresh_days, 3);
        assert_eq!(cached.theme, Theme::Dark);
        assert!(!cached.desktop_import_on_start);
        // A read error, distinct from a JSON/validation error.
        let parent = dir.path().join("not-a-directory");
        std::fs::write(&parent, b"file").unwrap();
        cached.reload(&parent.join("settings.json"));
        assert!(cached.load_error.is_some());
        assert_eq!(cached.refresh_days, 3);
    }

    #[test]
    fn broken_settings_block_automatic_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for bytes in [
            b"{broken".as_slice(),
            br#"{"version":2}"#,
            br#"{"refresh_hours":2}"#,
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(Settings::load(&path).is_err());
            assert!(Settings::default().save(&path).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn defaults_roundtrip_unknown_fields_and_atomic_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut s = Settings::load(&path).unwrap();
        assert_eq!(s.refresh_days, 7);
        assert_eq!(s.max_file_bytes, Some(100 << 20));
        s.save(&path).unwrap();
        s.language = Language::Uk;
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
        let mut json = serde_json::to_value(&s).unwrap();
        json["future_field"] = true.into();
        assert_eq!(serde_json::from_value::<Settings>(json).unwrap(), s);
        s.refresh_days = 31;
        assert!(s.save(&path).is_err());
        assert_eq!(Settings::load(&path).unwrap().refresh_days, 7);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn legacy_preferences_require_cleanup_and_marker_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, br#"{"version":1,"theme":"dark"}"#).unwrap();
        let mut settings = Settings::load(&path).unwrap();
        assert_eq!(settings.webview_cleanup_version, 0);
        settings.webview_cleanup_version = 1;
        settings.save(&path).unwrap();
        let restarted = Settings::load(&path).unwrap();
        assert_eq!(restarted.webview_cleanup_version, 1);
        assert_eq!(restarted.theme, Theme::Dark);
    }

    #[test]
    fn validation_and_sync_options() {
        for cap in [
            None,
            Some(0),
            Some(10 << 20),
            Some(25 << 20),
            Some(100 << 20),
        ] {
            let s = Settings {
                max_file_bytes: cap,
                refresh_days: 0,
                refresh_hours: 12,
                ..Default::default()
            };
            s.validate().unwrap();
            let mut o = crate::sync::engine::EngineOptions::default();
            s.apply_sync(&mut o, PathBuf::from("cache"));
            assert_eq!(o.files.unwrap().1, cap);
            assert_eq!(o.refresh_days, 0);
            assert_eq!(o.refresh_every.as_secs(), 43200);
        }
        for s in [
            Settings {
                version: 2,
                ..Default::default()
            },
            Settings {
                refresh_hours: 2,
                ..Default::default()
            },
            Settings {
                max_file_bytes: Some(-1),
                ..Default::default()
            },
            Settings {
                zoom_percent: 105,
                ..Default::default()
            },
        ] {
            assert!(s.validate().is_err());
        }
    }
    #[test]
    fn engine_reloads_file_cap_and_refresh_at_pass_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let options = crate::sync::engine::EngineOptions {
            settings_path: Some(path.clone()),
            files: Some((dir.path().join("cache"), None)),
            ..Default::default()
        };
        let mut settings = Settings::default();
        settings.save(&path).unwrap();
        assert_eq!(
            options.reload_settings().unwrap().files.unwrap().1,
            Some(100 << 20)
        );
        settings.max_file_bytes = Some(0);
        settings.refresh_days = 0;
        settings.refresh_hours = 24;
        settings.save(&path).unwrap();
        let resolved = options.reload_settings().unwrap();
        assert_eq!(resolved.files.unwrap().1, Some(0));
        assert_eq!(resolved.refresh_days, 0);
        assert_eq!(resolved.refresh_every.as_secs(), 86400);
        // Changing the next pass does not mutate an in-flight pass.
        assert_eq!(options.files.unwrap().1, None);
    }
}
