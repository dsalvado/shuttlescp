//! This program's build details, recorded by `build.rs`, and the About dialog that
//! shows them.

use relm4::gtk;
use relm4::gtk::prelude::*;

use shuttlescp_core::about::{self, BuildInfo};

use crate::dialog::{self, ButtonKind, Dialog};

/// Size of the app icon in the About dialog, in pixels.
const LOGO_SIZE: i32 = 64;

/// How this program was built.
pub fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        commit: env!("SHUTTLESCP_GIT_COMMIT"),
        dirty: env!("SHUTTLESCP_GIT_DIRTY") == "true",
        built: env!("SHUTTLESCP_BUILT"),
        profile: env!("SHUTTLESCP_PROFILE"),
        target: env!("SHUTTLESCP_TARGET"),
        rustc: env!("SHUTTLESCP_RUSTC"),
    }
}

/// Full build and system details, as shown in the About dialog.
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
            shuttlescp_core::config::config_dir().display().to_string(),
        ),
    ];
    if let Some(path) = log_path {
        extra.push(("Log file", path.display().to_string()));
    }
    build_info().details(&extra)
}

/// Open the About dialog over `parent`: the app, its version, and the build and system
/// details (folded away, with a button to copy them for a bug report).
///
/// # Arguments
///
/// * `parent` - Window the dialog stays above
/// * `log_path` - The log file, listed with the system details
pub fn show_about(parent: &gtk::Window, log_path: Option<&std::path::Path>) {
    let info = build_info();
    let dialog = Dialog::new(parent, &format!("About {}", about::APP_NAME));

    let logo = gtk::Image::builder()
        .icon_name(crate::desktop_entry::APP_ICON)
        .pixel_size(LOGO_SIZE)
        .valign(gtk::Align::Start)
        .build();
    let name = dialog::text(about::APP_NAME, &["about-name"]);
    let version = dialog::text(
        &format!("Version {} ({})", info.version, info.revision()),
        &["dim-label"],
    );
    let description = dialog::text(about::DESCRIPTION, &[]);
    let authors: Vec<&str> = env!("CARGO_PKG_AUTHORS")
        .split(':')
        .filter(|name| !name.is_empty())
        .collect();
    let heading = gtk::Box::new(gtk::Orientation::Vertical, 2);
    heading.set_hexpand(true);
    heading.append(&name);
    heading.append(&version);
    heading.append(&description);
    if !authors.is_empty() {
        heading.append(&dialog::text(
            &format!("By {}", authors.join(", ")),
            &["dim-label"],
        ));
    }
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    top.append(&logo);
    top.append(&heading);

    let system = details(log_path);
    let details_text = gtk::Label::builder()
        .label(&system)
        .xalign(0.0)
        .selectable(true)
        .css_classes(["about-details"])
        .build();
    let expander = gtk::Expander::builder()
        .label("System details")
        .child(&details_text)
        .margin_top(dialog::SPACING)
        .build();
    dialog.content.append(&top);
    dialog.content.append(&expander);

    dialog.add_button("Copy Details", ButtonKind::Normal, move || {
        if let Some(display) = gtk::gdk::Display::default() {
            display.clipboard().set_text(&system);
        }
    });
    let close = dialog.add_close_button("Close");
    dialog.window.set_default_widget(Some(&close));
    dialog.window.present();
    close.grab_focus();
}
