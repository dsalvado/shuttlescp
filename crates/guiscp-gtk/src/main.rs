//! GuiSCP: a graphical SFTP/SCP client for Linux, inspired by WinSCP.

mod app;
mod build_info;
mod confirm_dialog;
mod disk_picker;
mod file_pane;
mod log_tail;
mod login_dialog;
mod passphrase_dialog;
mod resize_grips;
mod theme;
mod theme_list;
mod transfer_panel;

use relm4::RelmApp;

use guiscp_core::config::Config;
use guiscp_core::logging;

use crate::app::{App, AppInit};

/// GTK application identifier (reverse-DNS form, used for D-Bus and desktop integration).
const APP_ID: &str = "io.github.guiscp.GuiScp";

fn main() {
    // Settings are read before logging starts because they hold the log level; a load
    // error is reported once the logger is running.
    let config_path = Config::default_path();
    let loaded = Config::load(&config_path);
    let level = loaded.as_ref().map(Config::log_level).unwrap_or_default();
    let logger = logging::init(level, &logging::default_log_path());
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

    let app = RelmApp::new(APP_ID);
    // `include_str!` embeds the file's contents into the binary at compile time.
    relm4::set_global_css(include_str!("style.css"));
    app.run::<App>(AppInit {
        config,
        config_path,
        logger,
    });
}
