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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
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
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.refresh_days > 30
            || ![1, 3, 12, 24].contains(&self.refresh_hours)
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
        let value = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        value.validate()?;
        Ok(value)
    }

    /// A unique temporary file, fsync, and rename preserve the last valid file
    /// if a write fails. No credentials belong in this file.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let parent = path
            .parent()
            .ok_or_else(|| Error::Store("invalid settings path".into()))?;
        std::fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(&serde_json::to_vec_pretty(self)?)?;
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
