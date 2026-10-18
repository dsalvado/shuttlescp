//! GuiSCP: a graphical SFTP/SCP client for Linux, inspired by WinSCP.

mod app;
mod config;
mod disk_picker;
mod disks;
mod file_pane;
mod local_fs;
mod logging;
mod resize_grips;
mod theme;

use relm4::RelmApp;

use crate::app::{App, AppInit};
use crate::config::Config;

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

    let app = RelmApp::new(APP_ID);
    // `include_str!` embeds the file's contents into the binary at compile time.
    relm4::set_global_css(include_str!("style.css"));
    app.run::<App>(AppInit {
        config,
        config_path,
        logger,
    });
}
