//! ShuttleSCP: a graphical SFTP/SCP client for Linux, macOS and Windows, inspired by
//! WinSCP.

// Windows: a release build is a GUI program, so starting it opens no console window
// (its messages go to the log). Debug builds keep the console, for `cargo run`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod build_info;
mod confirm_dialog;
mod conflict_dialog;
mod desktop_entry;
mod dialog;
mod disk_picker;
mod file_pane;
mod key_passphrase_dialog;
mod log_tail;
mod login_dialog;
#[cfg(target_os = "macos")]
mod mac_icons;
mod name_dialog;
mod passphrase_dialog;
mod quit_dialog;
mod resize_grips;
mod session;
mod theme;
mod theme_list;
mod transfer_panel;
#[cfg(debug_assertions)]
mod ui_gallery;
mod update_dialog;
#[cfg(windows)]
mod win_icons;

use relm4::RelmApp;

use shuttlescp_core::config::{self, Config};
use shuttlescp_core::logging;

use crate::app::{App, AppInit};

/// GTK application identifier (reverse-DNS form, used for D-Bus and desktop integration).
const APP_ID: &str = "io.github.dsalvado.ShuttleSCP";

fn main() {
    // Settings are read before logging starts because they hold the log level; a load
    // error is reported once the logger is running.
    // Carry settings over from the app's earlier name (GuiSCP) before reading them.
    let migrated = config::migrate_from_legacy_name();
    let config_path = Config::default_path();
    let loaded = Config::load(&config_path);
    let level = loaded.as_ref().map(Config::log_level).unwrap_or_default();
    let logger = logging::init(level, &logging::default_log_path());
    forward_toolkit_messages();
    for note in migrated {
        tracing::info!("moving over from GuiSCP: {note}");
    }
    let config = loaded.unwrap_or_else(|error| {
        tracing::warn!("{error}; using default settings");
        Config::default()
    });

    // Background work (listings, transfers, the SSH connection) runs on relm4's async
    // runtime, which uses a single worker thread unless told otherwise. One thread per
    // CPU core (2 to 8) lets several transfers run truly in parallel. It must be set
    // before the runtime starts.
    let threads = std::thread::available_parallelism().map_or(2, |count| count.get().clamp(2, 8));
    let _ = relm4::RELM_THREADS.set(threads);
    tracing::debug!("background worker threads: {threads}");

    // The UI gallery registers under its own ID, so it runs even while ShuttleSCP is
    // open (a second launch would otherwise hand over to the running app and exit).
    #[cfg(debug_assertions)]
    let app_id = if std::env::var_os(ui_gallery::ENV_VAR).is_some() {
        ui_gallery::APP_ID
    } else {
        APP_ID
    };
    #[cfg(not(debug_assertions))]
    let app_id = APP_ID;
    let app = RelmApp::new(app_id);
    // `include_str!` embeds the file's contents into the binary at compile time. On
    // macOS the Mac's sizes follow, overriding the Linux ones (see style-macos.css).
    #[cfg(target_os = "macos")]
    relm4::set_global_css(concat!(
        include_str!("style.css"),
        include_str!("style-macos.css")
    ));
    #[cfg(not(target_os = "macos"))]
    relm4::set_global_css(include_str!("style.css"));
    desktop_entry::install(APP_ID);
    app.run::<App>(AppInit {
        config,
        config_path,
        logger,
    });
}

/// Send the messages of GTK and the libraries under it (GLib, Pango, ...), such as
/// "Gtk-WARNING: Failed to load icon", to the app's log instead of the terminal. Each
/// line starts with the library's name. GLib hides its info and debug messages unless
/// asked; here they become debug and trace lines.
fn forward_toolkit_messages() {
    use relm4::gtk::glib::{self, LogLevel, LogWriterOutput};

    glib::log_set_writer_func(|level, fields| {
        let field = |key: &str| {
            fields
                .iter()
                .find(|field| field.key() == key)
                .and_then(glib::LogField::value_str)
        };
        let domain = field("GLIB_DOMAIN").unwrap_or("GLib");
        let message = field("MESSAGE").unwrap_or_default();
        match level {
            LogLevel::Error | LogLevel::Critical => tracing::error!("{domain}: {message}"),
            LogLevel::Warning => tracing::warn!("{domain}: {message}"),
            LogLevel::Message => tracing::info!("{domain}: {message}"),
            LogLevel::Info => tracing::debug!("{domain}: {message}"),
            LogLevel::Debug => tracing::trace!("{domain}: {message}"),
        }
        LogWriterOutput::Handled
    });
}
