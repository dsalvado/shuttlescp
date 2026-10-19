//! User preferences persisted as JSON in the XDG config directory.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::logging::LogLevel;

/// Error raised while loading or saving the configuration file.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The file or its directory could not be read or written.
    #[error("cannot access {}: {source}", path.display())]
    Io {
        /// File or directory involved.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: io::Error,
    },
    /// The file exists but does not contain valid settings.
    #[error("invalid settings in {}: {source}", path.display())]
    Json {
        /// File involved.
        path: PathBuf,
        /// Underlying parse error.
        #[source]
        source: serde_json::Error,
    },
}

/// Appearance the user picked. Which themes exist, and how one is applied, is up to the
/// front end; this only records the choice.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    /// Follow the desktop's theme.
    #[default]
    System,
    /// Use a specific theme by name.
    Named(String),
}

impl ThemeChoice {
    /// Parse a key made by [`ThemeChoice::key`]; the empty string means
    /// [`ThemeChoice::System`] (no theme can have an empty name).
    ///
    /// # Arguments
    ///
    /// * `key` - The key
    ///
    /// # Returns
    ///
    /// The choice the key represents
    pub fn from_key(key: &str) -> Self {
        if key.is_empty() {
            Self::System
        } else {
            Self::Named(key.to_owned())
        }
    }

    /// A single string identifying the choice, e.g. for a menu item's value.
    ///
    /// # Returns
    ///
    /// The empty string for [`ThemeChoice::System`], otherwise the theme name
    pub fn key(&self) -> &str {
        match self {
            Self::System => "",
            Self::Named(name) => name,
        }
    }
}

/// Persisted user preferences.
// `#[serde(default)]` fills fields missing from older files from `Config::default()`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    theme: ThemeChoice,
    /// Directory the local pane showed when the app was last closed.
    last_local_dir: Option<PathBuf>,
    /// Last directory shown on each disk, keyed by mount point. A `BTreeMap` keeps the
    /// keys sorted, so the saved file lists disks in a stable order.
    last_dir_on_disk: BTreeMap<PathBuf, PathBuf>,
    log_level: LogLevel,
    /// Open the Login dialog when the app starts.
    show_login_at_startup: bool,
}

// Written out by hand (not derived) because the Login dialog should show at startup
// unless the user turns it off, and a derived default would make that `false`.
impl Default for Config {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            last_local_dir: None,
            last_dir_on_disk: BTreeMap::new(),
            log_level: LogLevel::default(),
            show_login_at_startup: true,
        }
    }
}

/// The app's folder for user files: `$XDG_CONFIG_HOME/guiscp` on Linux (usually
/// `~/.config/guiscp`), and the platform's equivalent elsewhere (e.g. `%APPDATA%\guiscp`
/// on Windows, `~/Library/Application Support/guiscp` on macOS).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        .unwrap_or_default()
        .join("guiscp")
}

/// Read a JSON file into `T`.
///
/// # Arguments
///
/// * `path` - JSON file to read
///
/// # Returns
///
/// The stored value, or `T::default()` if the file does not exist yet
///
/// # Errors
///
/// Returns [`ConfigError::Io`] if the file exists but cannot be read, or
/// [`ConfigError::Json`] if it is not valid JSON for `T`
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T, ConfigError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(T::default()),
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    serde_json::from_str(&text).map_err(|source| ConfigError::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// Write `value` to `path` as pretty-printed JSON, creating parent directories as needed.
///
/// # Arguments
///
/// * `value` - Data to write
/// * `path` - JSON file to write
///
/// # Errors
///
/// Returns [`ConfigError::Io`] if the directory or file cannot be written
pub fn save_json<T: Serialize>(value: &T, path: &Path) -> Result<(), ConfigError> {
    let io_error = |path: &Path| {
        let path = path.to_path_buf();
        move |source| ConfigError::Io { path, source }
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io_error(dir))?;
    }
    let json = serde_json::to_string_pretty(value).map_err(|source| ConfigError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    fs::write(path, json).map_err(io_error(path))
}

impl Config {
    /// Default location: `settings.json` in [`config_dir`].
    pub fn default_path() -> PathBuf {
        config_dir().join("settings.json")
    }

    /// Read the configuration from `path` (see [`load_json`]).
    ///
    /// # Arguments
    ///
    /// * `path` - JSON file to read
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if the file exists but cannot be read or parsed
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        load_json(path)
    }

    /// Write the configuration to `path` (see [`save_json`]).
    ///
    /// # Arguments
    ///
    /// * `path` - JSON file to write
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Io`] if the directory or file cannot be written
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        save_json(self, path)
    }

    /// Theme the user picked.
    pub fn theme(&self) -> &ThemeChoice {
        &self.theme
    }

    /// Change the theme preference (call [`Config::save`] to persist it).
    ///
    /// # Arguments
    ///
    /// * `theme` - New theme preference
    pub fn set_theme(&mut self, theme: ThemeChoice) {
        self.theme = theme;
    }

    /// Directory the local pane showed when the app was last closed, if any.
    pub fn last_local_dir(&self) -> Option<&Path> {
        self.last_local_dir.as_deref()
    }

    /// Remember the local pane's directory (call [`Config::save`] to persist it).
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory currently shown in the local pane
    pub fn set_last_local_dir(&mut self, dir: PathBuf) {
        self.last_local_dir = Some(dir);
    }

    /// Last directory shown on each disk, keyed by the disk's mount point.
    pub fn last_dir_on_disk(&self) -> &BTreeMap<PathBuf, PathBuf> {
        &self.last_dir_on_disk
    }

    /// Remember the directory last shown on a disk (call [`Config::save`] to persist it).
    ///
    /// # Arguments
    ///
    /// * `disk_root` - Mount point of the disk
    /// * `dir` - Directory shown on that disk
    pub fn set_last_dir_on_disk(&mut self, disk_root: PathBuf, dir: PathBuf) {
        self.last_dir_on_disk.insert(disk_root, dir);
    }

    /// Whether the Login dialog opens when the app starts.
    pub fn show_login_at_startup(&self) -> bool {
        self.show_login_at_startup
    }

    /// Change whether the Login dialog opens at startup (call [`Config::save`] to persist).
    ///
    /// # Arguments
    ///
    /// * `show` - `true` to open it at startup
    pub fn set_show_login_at_startup(&mut self, show: bool) {
        self.show_login_at_startup = show;
    }

    /// Log level the user picked.
    pub fn log_level(&self) -> LogLevel {
        self.log_level
    }

    /// Change the log level preference (call [`Config::save`] to persist it).
    ///
    /// # Arguments
    ///
    /// * `level` - New log level
    pub fn set_log_level(&mut self, level: LogLevel) {
        self.log_level = level;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_choice_key_round_trips() {
        for choice in [
            ThemeChoice::System,
            ThemeChoice::Named(String::from("Zorin")),
        ] {
            assert_eq!(ThemeChoice::from_key(choice.key()), choice);
        }
        assert_eq!(ThemeChoice::System.key(), "");
    }

    #[test]
    fn missing_file_loads_defaults() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let config = Config::load(&temp.path().join("absent.json")).expect("load defaults");
        assert_eq!(config, Config::default());
    }

    #[test]
    fn save_then_load_round_trips_into_new_directory() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("nested/settings.json");
        let mut config = Config::default();
        config.set_theme(ThemeChoice::Named(String::from("Adwaita-dark")));
        config.set_last_local_dir(PathBuf::from("/srv/data"));
        config.set_log_level(LogLevel::Warn);
        config.set_last_dir_on_disk(PathBuf::from("/media/usb"), PathBuf::from("/media/usb/a"));

        // Act
        config.save(&path).expect("save config");
        let loaded = Config::load(&path).expect("load config");

        // Assert
        assert_eq!(loaded, config);
    }

    #[test]
    fn empty_object_uses_defaults_and_garbage_is_an_error() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("settings.json");

        fs::write(&path, "{}").expect("write file");
        let loaded = Config::load(&path).expect("load");
        assert_eq!(loaded, Config::default());
        // Settings files from before this option existed keep showing the dialog.
        assert!(loaded.show_login_at_startup());

        fs::write(&path, "not json").expect("write file");
        assert!(matches!(Config::load(&path), Err(ConfigError::Json { .. })));
    }
}
