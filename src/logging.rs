//! Application logging to a file (and stderr) with a level that can change at runtime.

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use relm4::gtk::glib;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{Event, Subscriber};
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Registry, fmt, reload};

/// Crate name, used as the tracing target prefix of every event this app emits.
const APP_TARGET: &str = env!("CARGO_PKG_NAME");
/// Target for level-change notices. It is always enabled, so a change is recorded even
/// when the new (or old) level would hide informational messages.
const LEVEL_CHANGE_TARGET: &str = concat!(env!("CARGO_PKG_NAME"), "::log_level");
/// Target for the start and end of each app session; always enabled for the same reason.
const SESSION_TARGET: &str = concat!(env!("CARGO_PKG_NAME"), "::session");
/// When the log file grows past this size it is moved to `<name>.1` at startup.
const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
/// Also echo log lines to the terminal. Off now that the Log tab in the bottom panel
/// shows the log; set to `true` to see it in the terminal again while developing.
/// (stderr is still used if the log file cannot be opened.)
const LOG_TO_STDERR: bool = false;

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
    /// Distinct name of this app session, shown in its start and end banners.
    session_label: String,
    /// When this app session started, for the duration logged at the end.
    started: Instant,
}

impl Logger {
    /// Record the end of the app session, with how long it lasted. Call once, on exit.
    ///
    /// # Arguments
    ///
    /// * `reason` - How the app was closed, e.g. "window closed" or "Ctrl+C (SIGINT)"
    pub fn end_session(&self, reason: &str) {
        tracing::info!(
            target: SESSION_TARGET,
            "==== Session {} Ended after {}, reason: {reason} ====",
            self.session_label,
            format_duration(self.started.elapsed())
        );
    }

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
    let tag = session_tag(glib::random_int());
    let session_label = session_label(glib::DateTime::now_local().ok(), &tag);
    let line_format = LineFormat {
        session: Arc::from(tag),
    };
    let (filter, handle) = reload::Layer::new(filter_for(level));
    let (file_layer, file_path) = match open_log_file(log_path) {
        // `Arc<File>` is a lock-free writer: each event is written with a single
        // append-mode `write` call.
        Ok(file) => (
            Some(
                fmt::layer()
                    .event_format(line_format.clone())
                    .with_writer(Arc::new(file)),
            ),
            Ok(log_path.to_path_buf()),
        ),
        Err(error) => (None, Err(error)),
    };
    // An `Option<Layer>` is itself a layer that does nothing when `None`.
    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        // `bool::then` gives `Some(layer)` only when the condition holds.
        .with((LOG_TO_STDERR || file_path.is_err()).then(|| {
            fmt::layer()
                .event_format(line_format)
                .with_writer(io::stderr)
        }))
        .init();

    let started = Instant::now();
    log_crashes(session_label.clone(), started);
    tracing::info!(
        target: SESSION_TARGET,
        "==== Session {session_label} Started (GuiSCP {}, pid {}) ====",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );
    // The error can only be logged once the subscriber above is installed.
    let file_path = file_path
        .inspect_err(|error| tracing::error!("{error}; logging to stderr only"))
        .ok();
    tracing::info!(target: LEVEL_CHANGE_TARGET, "log level is {}", level.label());
    Logger {
        handle,
        level,
        file_path,
        session_label,
        started,
    }
}

/// Make a crash (Rust panic) end the session with a banner in the log before the
/// default panic report is printed.
///
/// # Arguments
///
/// * `session_label` - Label of this session, for the banner
/// * `started` - When the session started, for the duration in the banner
fn log_crashes(session_label: String, started: Instant) {
    // `take_hook` returns the current handler so ours can still call it afterwards.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(
            target: SESSION_TARGET,
            "==== Session {session_label} Ended abnormally after {}: {info} ====",
            format_duration(started.elapsed())
        );
        default_hook(info);
    }));
}

/// Build the short session tag printed on every log line: four hex digits in brackets,
/// e.g. `[a3f1]`.
///
/// # Arguments
///
/// * `random` - Random number; its low 16 bits are used
fn session_tag(random: u32) -> String {
    format!("[{:04x}]", random & 0xffff)
}

/// Build the full session label used in the start and end banners: the local start time
/// plus the session tag, e.g. `20261018-070512-[a3f1]`. Labels sort in start order and
/// two sessions started in the same second still differ.
///
/// # Arguments
///
/// * `start` - Local start time, or `None` if the clock could not be read
/// * `tag` - The session tag from [`session_tag`]
fn session_label(start: Option<glib::DateTime>, tag: &str) -> String {
    let time = start
        .and_then(|time| time.format("%Y%m%d-%H%M%S").ok())
        .map_or_else(|| String::from("unknown-time"), String::from);
    format!("{time}-{tag}")
}

/// Log line layout, in local time:
/// `[YYYY-MM-DD HH:MM:SS.mmm] [tag] LEVEL target: message`.
/// Session banners leave out the target so they read as plain headings.
#[derive(Clone)]
struct LineFormat {
    /// Session tag printed on every line. `Arc<str>` is a shared, immutable string, so
    /// the file and terminal formatters share one copy.
    session: Arc<str>,
}

// `where` clauses spell out what tracing needs from the subscriber (`S`) and the field
// formatter (`N`); this implementation works with any that qualify.
impl<S, N> FormatEvent<S, N> for LineFormat
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        context: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let metadata = event.metadata();
        write!(
            writer,
            "[{}] {} {:<5} ",
            local_timestamp(),
            self.session,
            metadata.level()
        )?;
        if metadata.target() != SESSION_TARGET {
            write!(writer, "{}: ", metadata.target())?;
        }
        context
            .field_format()
            .format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// Current local time as `YYYY-MM-DD HH:MM:SS.mmm`.
fn local_timestamp() -> String {
    let Ok(now) = glib::DateTime::now_local() else {
        return String::from("unknown time");
    };
    let mut text = now
        .format("%Y-%m-%d %H:%M:%S")
        .map_or_else(|_| String::from("unknown time"), String::from);
    // Writing into a `String` cannot fail, so the result is safely ignored.
    let _ = write!(text, ".{:03}", now.microsecond() / 1000);
    text
}

/// Format a duration as `H:MM:SS`, e.g. 3725 seconds becomes `"1:02:05"`.
///
/// # Arguments
///
/// * `duration` - Elapsed time; fractions of a second are dropped
fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// Build the filter for a level: this app logs at `level`, the level-change and session
/// targets are always on, and third-party crates are capped at warnings to keep the log
/// readable.
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
        .with_target(SESSION_TARGET, LevelFilter::TRACE)
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
        assert!(filter.would_enable(SESSION_TARGET, &Level::INFO));
    }

    #[test]
    fn session_tag_is_four_bracketed_hex_digits() {
        assert_eq!(session_tag(0xa3f1), "[a3f1]");
        assert_eq!(session_tag(0x1234_0007), "[0007]");
    }

    #[test]
    fn session_label_combines_start_time_and_tag() {
        // Arrange
        let start = glib::DateTime::from_local(2026, 10, 18, 7, 5, 12.0).expect("valid date");

        // Act + Assert
        assert_eq!(
            session_label(Some(start), "[a3f1]"),
            "20261018-070512-[a3f1]"
        );
        assert_eq!(session_label(None, "[beef]"), "unknown-time-[beef]");
    }

    #[test]
    fn format_duration_shows_hours_minutes_seconds() {
        assert_eq!(format_duration(Duration::from_millis(999)), "0:00:00");
        assert_eq!(format_duration(Duration::from_secs(3725)), "1:02:05");
        assert_eq!(
            format_duration(Duration::from_secs(100 * 3600)),
            "100:00:00"
        );
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
