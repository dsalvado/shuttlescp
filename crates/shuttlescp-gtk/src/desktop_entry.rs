//! Desktop integration: the app's icon and its launcher entry.
//!
//! On Wayland, GNOME and other desktops pick a running app's icon from the desktop
//! entry whose name matches the app id, not from the window. So at startup the app
//! writes (or updates) `~/.local/share/applications/<app id>.desktop`, which also makes
//! ShuttleSCP appear in the desktop's app launcher.

use std::path::Path;

use relm4::gtk;
use relm4::gtk::glib;

use shuttlescp_core::about;

/// Desktop entries earlier versions installed under other app ids (while the app was
/// called GuiSCP, and before its id named the GitHub account); removed so the launcher
/// does not list the app twice.
const LEGACY_ENTRIES: [&str; 2] = [
    "io.github.guiscp.GuiScp.desktop",
    "io.github.shuttlescp.ShuttleSCP.desktop",
];

/// Theme icon used for the app (also on the About dialog).
pub const APP_ICON: &str = "folder-remote";

/// Set the app's icon and make sure its desktop entry is installed and up to date.
/// Failures are logged, never fatal: they only cost the icon.
///
/// # Arguments
///
/// * `app_id` - The application id (reverse-DNS), which names the desktop entry
pub fn install(app_id: &str) {
    // Used for windows on X11, and by desktops that read the window's icon.
    gtk::Window::set_default_icon_name(APP_ICON);

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            tracing::warn!("cannot find the program's path for its desktop entry: {error}");
            return;
        }
    };
    let applications = glib::user_data_dir().join("applications");
    for legacy in LEGACY_ENTRIES.map(|name| applications.join(name)) {
        if legacy.exists() {
            match std::fs::remove_file(&legacy) {
                Ok(()) => tracing::info!("removed the old desktop entry {}", legacy.display()),
                Err(error) => tracing::warn!("cannot remove {}: {error}", legacy.display()),
            }
        }
    }
    let path = applications.join(format!("{app_id}.desktop"));
    let entry = entry_text(app_id, &exe);
    if std::fs::read_to_string(&path).is_ok_and(|current| current == entry) {
        return;
    }
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, &entry));
    match written {
        Ok(()) => tracing::info!("installed desktop entry {}", path.display()),
        Err(error) => tracing::warn!("cannot write {}: {error}", path.display()),
    }
}

/// The desktop entry's contents (see the freedesktop Desktop Entry Specification).
fn entry_text(app_id: &str, exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Comment={comment}\n\
         Exec={exec}\n\
         Icon={APP_ICON}\n\
         Terminal=false\n\
         Categories=Network;FileTransfer;\n\
         StartupNotify=true\n\
         StartupWMClass={app_id}\n",
        name = about::APP_NAME,
        comment = about::DESCRIPTION,
        exec = quote_exec(&exe.to_string_lossy()),
    )
}

/// Quote a program path for an `Exec=` line: wrapped in double quotes, with `"`, `` ` ``,
/// `$` and `\` escaped by a backslash, and `%` doubled (it introduces field codes).
fn quote_exec(path: &str) -> String {
    let mut quoted = String::from("\"");
    for character in path.chars() {
        match character {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    // The entry is itself escaped once more: a backslash in a value is written twice.
    quoted.replace('\\', "\\\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_paths_are_quoted_and_escaped() {
        assert_eq!(
            quote_exec("/opt/gui scp/shuttlescp"),
            "\"/opt/gui scp/shuttlescp\""
        );
        assert_eq!(quote_exec("/a$b"), "\"/a\\\\$b\"");
        assert_eq!(quote_exec("/100%"), "\"/100%%\"");
    }

    #[test]
    fn entry_names_the_icon_and_window_class() {
        let text = entry_text("io.example.App", Path::new("/usr/bin/shuttlescp"));
        assert!(text.starts_with("[Desktop Entry]\n"));
        assert!(text.contains("\nIcon=folder-remote\n"));
        assert!(text.contains("\nStartupWMClass=io.example.App\n"));
        assert!(text.contains("\nExec=\"/usr/bin/shuttlescp\"\n"));
    }
}
