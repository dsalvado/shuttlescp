//! Application logging to a file (and stderr) with a level that can change at runtime.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use relm4::gtk::glib;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::prelude::*;
use tracing_subscriber::{Registry, fmt, reload};

/// Crate name, used as the tracing target prefix of every event this app emits.
const APP_TARGET: &str = env!("CARGO_PKG_NAME");
/// Target for level-change notices. It is always enabled, so a change is recorded even
/// when the new (or old) level would hide informational messages.
const LEVEL_CHANGE_TARGET: &str = concat!(env!("CARGO_PKG_NAME"), "::log_level");
/// When the log file grows past this size it is moved to `<name>.1` at startup.
const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
/// Also echo log lines to the terminal. Development aid: set to `false` to log only to
/// the file (stderr is still used if the file cannot be opened).
const LOG_TO_STDERR: bool = true;

/// Verbosity of the application log, from least to most detailed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Only failures.
    Error,
    /// Failures and suspicious situations.
    Warn,
    /// High level events such as startup, shutdown and settings changes.
    Info,
    /// Detailed events useful while developing, e.g. every directory listed.
    #[default]
    Debug,
    /// Everything, including very chatty internal steps.
    Trace,
}

impl LogLevel {
    /// All levels in menu order.
    pub const ALL: [Self; 5] = [
        Self::Error,
        Self::Warn,
        Self::Info,
        Self::Debug,
        Self::Trace,
    ];

    /// Stable identifier, used as the menu action target (matches the serde name).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    /// Human readable name for menus.
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::Warn => "Warning",
            Self::Info => "Info",
            Self::Debug => "Debug",
            Self::Trace => "Trace",
        }
    }

    /// Inverse of [`LogLevel::as_str`].
    ///
    /// # Arguments
    ///
    /// * `name` - A string produced by [`LogLevel::as_str`]
    ///
    /// # Returns
    ///
    /// The matching level, or `None` for an unknown name
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.as_str() == name)
    }

    fn filter(self) -> LevelFilter {
        match self {
            Self::Error => LevelFilter::ERROR,
            Self::Warn => LevelFilter::WARN,
            Self::Info => LevelFilter::INFO,
            Self::Debug => LevelFilter::DEBUG,
            Self::Trace => LevelFilter::TRACE,
        }
    }
}

/// Error raised when the log file cannot be prepared.
#[derive(Debug, Error)]
#[error("cannot open log file {}: {source}", path.display())]
pub struct LogFileError {
    path: PathBuf,
    #[source]
    source: io::Error,
}

/// Handle to the installed logger, used to change its level while the app runs.
pub struct Logger {
    /// `reload::Handle` lets us swap the filter of an already installed subscriber.
    handle: reload::Handle<Targets, Registry>,
    level: LogLevel,
    file_path: Option<PathBuf>,
}

impl Logger {
    /// Current log level.
    pub fn level(&self) -> LogLevel {
        self.level
    }

    /// Path of the log file, or `None` if it could not be opened (stderr only).
    pub fn file_path(&self) -> Option<&Path> {
        self.file_path.as_deref()
    }

    /// Change the log level and record the change in the log.
    ///
    /// # Arguments
    ///
    /// * `level` - New level
    pub fn set_level(&mut self, level: LogLevel) {
        if level == self.level {
            return;
        }
        if let Err(error) = self.handle.reload(filter_for(level)) {
            tracing::error!("cannot change log level: {error}");
            return;
        }
        let previous = std::mem::replace(&mut self.level, level);
        tracing::info!(
            target: LEVEL_CHANGE_TARGET,
            "log level changed from {} to {}",
            previous.label(),
            level.label()
        );
    }
}

/// Default log file location: `$XDG_STATE_HOME/guiscp/logs/guiscp.log`
/// (`~/.local/state/guiscp/logs/guiscp.log` when the variable is unset).
pub fn default_log_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| glib::home_dir().join(".local/state"), PathBuf::from)
        .join(APP_TARGET)
        .join("logs")
        .join(concat!(env!("CARGO_PKG_NAME"), ".log"))
}

/// Install the global logger, writing to `log_path` (and stderr while `LOG_TO_STDERR`).
///
/// If the file cannot be opened, logging continues on stderr only and the failure is
/// logged. Must be called once, before anything else logs.
///
/// # Arguments
///
/// * `level` - Initial log level
/// * `log_path` - File to append log lines to
///
/// # Returns
///
/// A [`Logger`] used to change the level later
pub fn init(level: LogLevel, log_path: &Path) -> Logger {
    let (filter, handle) = reload::Layer::new(filter_for(level));
    let (file_layer, file_path) = match open_log_file(log_path) {
        // `Arc<File>` is a lock-free writer: each event is written with a single
        // append-mode `write` call.
        Ok(file) => (
            Some(fmt::layer().with_ansi(false).with_writer(Arc::new(file))),
            Ok(log_path.to_path_buf()),
        ),
        Err(error) => (None, Err(error)),
    };
    // An `Option<Layer>` is itself a layer that does nothing when `None`.
    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        // `bool::then` gives `Some(layer)` only when the condition holds.
        .with((LOG_TO_STDERR || file_path.is_err()).then(|| fmt::layer().with_writer(io::stderr)))
        .init();

    // The error can only be logged once the subscriber above is installed.
    let file_path = file_path
        .inspect_err(|error| tracing::error!("{error}; logging to stderr only"))
        .ok();
    tracing::info!(target: LEVEL_CHANGE_TARGET, "log level is {}", level.label());
    Logger {
        handle,
        level,
        file_path,
    }
}

/// Build the filter for a level: this app logs at `level`, the level-change target is
/// always on, and third-party crates are capped at warnings to keep the log readable.
///
/// # Arguments
///
/// * `level` - Level selected by the user
fn filter_for(level: LogLevel) -> Targets {
    let app_filter = level.filter();
    Targets::new()
        // More verbose filters compare greater, so `min` picks the quieter of the two.
        .with_default(app_filter.min(LevelFilter::WARN))
        .with_target(APP_TARGET, app_filter)
        .with_target(LEVEL_CHANGE_TARGET, LevelFilter::TRACE)
}

/// Open `path` for appending, creating its directory and rotating it when too large.
///
/// # Arguments
///
/// * `path` - Log file path
///
/// # Errors
///
/// Returns [`LogFileError`] if the directory or file cannot be created or opened
fn open_log_file(path: &Path) -> Result<File, LogFileError> {
    let wrap = |source| LogFileError {
        path: path.to_path_buf(),
        source,
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(wrap)?;
    }
    if fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES) {
        let mut rotated = path.as_os_str().to_owned();
        rotated.push(".1");
        fs::rename(path, rotated).map_err(wrap)?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(wrap)
}

#[cfg(test)]
mod tests {
    use tracing::Level;

    use super::*;

    #[test]
    fn level_names_round_trip() {
        for level in LogLevel::ALL {
            assert_eq!(LogLevel::from_name(level.as_str()), Some(level));
        }
        assert_eq!(LogLevel::from_name("loud"), None);
        assert_eq!(LogLevel::default(), LogLevel::Debug);
    }

    #[test]
    fn filter_limits_app_but_always_records_level_changes() {
        // Arrange
        let filter = filter_for(LogLevel::Error);

        // Act + Assert
        assert!(filter.would_enable(APP_TARGET, &Level::ERROR));
        assert!(!filter.would_enable(APP_TARGET, &Level::INFO));
        assert!(filter.would_enable(LEVEL_CHANGE_TARGET, &Level::INFO));
    }

    #[test]
    fn filter_caps_other_crates_at_warn() {
        let filter = filter_for(LogLevel::Trace);
        assert!(filter.would_enable(concat!(env!("CARGO_PKG_NAME"), "::app"), &Level::TRACE));
        assert!(filter.would_enable("relm4", &Level::WARN));
        assert!(!filter.would_enable("relm4", &Level::DEBUG));
    }

    #[test]
    fn open_log_file_creates_directory_and_rotates_large_files() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("nested/app.log");
        fs::create_dir_all(path.parent().expect("has parent")).expect("create dir");
        let big = File::create(&path).expect("create log");
        big.set_len(MAX_LOG_BYTES + 1).expect("grow log");

        // Act
        let file = open_log_file(&path).expect("open log");

        // Assert
        assert_eq!(file.metadata().expect("metadata").len(), 0);
        assert!(temp.path().join("nested/app.log.1").exists());
    }
}
