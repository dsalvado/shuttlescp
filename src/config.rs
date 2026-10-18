//! User preferences persisted as JSON in the XDG config directory.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use relm4::gtk::glib;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::logging::LogLevel;
use crate::theme::ThemeChoice;

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

/// Persisted user preferences.
// `#[serde(default)]` fills fields missing from older files with their default values.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    theme: ThemeChoice,
    /// Directory the local pane showed when the app was last closed.
    last_local_dir: Option<PathBuf>,
    /// Last directory shown on each disk, keyed by mount point. A `BTreeMap` keeps the
    /// keys sorted, so the saved file lists disks in a stable order.
    last_dir_on_disk: BTreeMap<PathBuf, PathBuf>,
    log_level: LogLevel,
}

impl Config {
    /// Default location: `$XDG_CONFIG_HOME/guiscp/settings.json`.
    pub fn default_path() -> PathBuf {
        glib::user_config_dir().join("guiscp").join("settings.json")
    }

    /// Read the configuration from `path`.
    ///
    /// # Arguments
    ///
    /// * `path` - JSON file to read
    ///
    /// # Returns
    ///
    /// The stored configuration, or the defaults if the file does not exist yet
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Io`] if the file exists but cannot be read, or
    /// [`ConfigError::Json`] if it is not valid JSON
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
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

    /// Write the configuration to `path`, creating parent directories as needed.
    ///
    /// # Arguments
    ///
    /// * `path` - JSON file to write
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Io`] if the directory or file cannot be written
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let io_error = |path: &Path| {
            let path = path.to_path_buf();
            move |source| ConfigError::Io { path, source }
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(io_error(dir))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|source| ConfigError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        fs::write(path, json).map_err(io_error(path))
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
        assert_eq!(Config::load(&path).expect("load"), Config::default());

        fs::write(&path, "not json").expect("write file");
        assert!(matches!(Config::load(&path), Err(ConfigError::Json { .. })));
    }
}
