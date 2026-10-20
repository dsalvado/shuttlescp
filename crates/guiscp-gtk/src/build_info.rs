//! This program's build details, recorded by `build.rs`, and the About dialog that
//! shows them.

use relm4::gtk;
use relm4::gtk::prelude::*;

use guiscp_core::about::{self, BuildInfo};

/// About dialog size (width, height) in pixels.
const ABOUT_SIZE: (i32, i32) = (620, 470);

/// How this program was built.
pub fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        commit: env!("GUISCP_GIT_COMMIT"),
        dirty: env!("GUISCP_GIT_DIRTY") == "true",
        built: env!("GUISCP_BUILT"),
        profile: env!("GUISCP_PROFILE"),
        target: env!("GUISCP_TARGET"),
        rustc: env!("GUISCP_RUSTC"),
    }
}

/// Full build and system details, as shown on the About dialog's System page.
///
/// # Arguments
///
/// * `log_path` - The log file, if logging to a file works
pub fn details(log_path: Option<&std::path::Path>) -> String {
    let mut extra = vec![
        (
            "GTK",
            format!(
                "{}.{}.{}",
                gtk::major_version(),
                gtk::minor_version(),
                gtk::micro_version()
            ),
        ),
        (
            "Settings",
            guiscp_core::config::config_dir().display().to_string(),
        ),
    ];
    if let Some(path) = log_path {
        extra.push(("Log file", path.display().to_string()));
    }
    build_info().details(&extra)
}

/// Open the About dialog over `parent`.
///
/// # Arguments
///
/// * `parent` - Window the dialog stays above
/// * `log_path` - The log file, listed with the system details
pub fn show_about(parent: &gtk::Window, log_path: Option<&std::path::Path>) {
    let info = build_info();
    // Cargo lists authors separated by ':'.
    let authors: Vec<&str> = env!("CARGO_PKG_AUTHORS")
        .split(':')
        .filter(|name| !name.is_empty())
        .collect();
    let dialog = gtk::AboutDialog::builder()
        .transient_for(parent)
        .modal(true)
        .program_name(about::APP_NAME)
        .version(format!("{} ({})", info.version, info.revision()))
        .comments(about::DESCRIPTION)
        .authors(authors)
        .logo_icon_name(crate::desktop_entry::APP_ICON)
        .system_information(details(log_path))
        // Tall enough to show every System line without scrolling.
        .default_width(ABOUT_SIZE.0)
        .default_height(ABOUT_SIZE.1)
        .build();
    dialog.present();
}
