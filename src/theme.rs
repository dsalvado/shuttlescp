//! Discovery and application of GTK themes.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use relm4::gtk;
use relm4::gtk::glib;
use serde::{Deserialize, Serialize};

/// Themes compiled into GTK itself, available even when nothing is installed on disk.
const BUILTIN_THEMES: [&str; 3] = ["Adwaita", "Adwaita-dark", "HighContrast"];

/// Cursor theme bundled with GTK/GNOME, used when the configured one is missing.
const FALLBACK_CURSOR_THEME: &str = "Adwaita";

/// Name of the GTK settings property holding the theme.
const THEME_PROPERTY: &str = "gtk-theme-name";

/// Theme selected by the user.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    /// Follow the desktop's theme.
    #[default]
    System,
    /// Use a specific GTK theme by name.
    Named(String),
}

impl ThemeChoice {
    /// Parse the string carried by a theme menu action; the empty string means
    /// [`ThemeChoice::System`] (no theme directory can have an empty name).
    ///
    /// # Arguments
    ///
    /// * `target` - The action's target value
    ///
    /// # Returns
    ///
    /// The choice the target represents
    pub fn from_action_target(target: &str) -> Self {
        if target.is_empty() {
            Self::System
        } else {
            Self::Named(target.to_owned())
        }
    }

    /// Inverse of [`ThemeChoice::from_action_target`].
    ///
    /// # Returns
    ///
    /// The empty string for [`ThemeChoice::System`], otherwise the theme name
    pub fn action_target(&self) -> &str {
        match self {
            Self::System => "",
            Self::Named(name) => name,
        }
    }
}

/// Directories GTK searches for themes, in GTK's own lookup order.
///
/// # Returns
///
/// `$XDG_DATA_HOME/themes`, `~/.themes`, then `themes` in each `$XDG_DATA_DIRS` entry
pub fn search_dirs() -> Vec<PathBuf> {
    data_dirs("themes")
}

/// XDG data directories for one kind of resource, in GTK's lookup order.
///
/// # Arguments
///
/// * `subdir` - Resource folder name, e.g. `themes` or `icons`
///
/// # Returns
///
/// `$XDG_DATA_HOME/<subdir>`, `~/.<subdir>`, then `<subdir>` in each `$XDG_DATA_DIRS` entry
fn data_dirs(subdir: &str) -> Vec<PathBuf> {
    let system_dirs = glib::system_data_dirs();
    let mut dirs = Vec::with_capacity(system_dirs.len() + 2);
    dirs.push(glib::user_data_dir().join(subdir));
    dirs.push(glib::home_dir().join(format!(".{subdir}")));
    dirs.extend(system_dirs.into_iter().map(|dir| dir.join(subdir)));
    dirs
}

/// Switch this app to GTK's Adwaita cursors if the desktop's cursor theme is missing.
///
/// A cursor theme set in the desktop settings but not installed where GTK can see it
/// (for example one shipped only inside a Snap package) makes GTK fall back to a plain
/// arrow for every cursor, hiding resize and text cursors.
pub fn ensure_cursor_theme() {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let Some(name) = settings.gtk_cursor_theme_name() else {
        return;
    };
    if cursor_theme_installed(&name, &data_dirs("icons")) {
        return;
    }
    tracing::warn!("cursor theme {name} is not installed; using {FALLBACK_CURSOR_THEME}");
    settings.set_gtk_cursor_theme_name(Some(FALLBACK_CURSOR_THEME));
}

/// Whether a cursor theme exists in any of `dirs`. A theme counts if it has its own
/// `cursors` folder or an `index.theme`, which may inherit cursors from another theme.
///
/// # Arguments
///
/// * `name` - Cursor theme name
/// * `dirs` - Icon directories to search
fn cursor_theme_installed(name: &str, dirs: &[PathBuf]) -> bool {
    dirs.iter()
        .map(|dir| dir.join(name))
        .any(|theme| theme.join("cursors").is_dir() || theme.join("index.theme").is_file())
}

/// List the themes usable by GTK 4: the built-in ones plus every theme directory that
/// contains a `gtk-4.0` folder.
///
/// # Arguments
///
/// * `search_dirs` - Directories to scan, usually from [`search_dirs`]
///
/// # Returns
///
/// Sorted theme names without duplicates; unreadable directories are skipped
pub fn available_themes(search_dirs: &[PathBuf]) -> Vec<String> {
    // A `BTreeSet` keeps its items sorted and unique, like Python's `sorted(set(...))`.
    let mut names: BTreeSet<String> = BUILTIN_THEMES.iter().map(|&name| name.into()).collect();
    names.extend(
        search_dirs
            .iter()
            .filter_map(|dir| fs::read_dir(dir).ok())
            .flatten()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().join("gtk-4.0").is_dir())
            // Theme names that are not valid UTF-8 cannot be passed to GTK, so drop them.
            .filter_map(|entry| entry.file_name().into_string().ok()),
    );
    names.into_iter().collect()
}

/// Apply a theme to the whole application immediately.
///
/// # Arguments
///
/// * `choice` - Theme to use; [`ThemeChoice::System`] drops any override
pub fn apply(choice: &ThemeChoice) {
    let Some(settings) = gtk::Settings::default() else {
        tracing::error!("cannot change theme: no display connected");
        return;
    };
    match choice {
        // Removes the app-specific value so GTK follows the desktop setting again.
        ThemeChoice::System => settings.reset_property(THEME_PROPERTY),
        ThemeChoice::Named(name) => settings.set_gtk_theme_name(Some(name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_themes_includes_builtins_and_gtk4_themes_only() {
        // Arrange
        let first = tempfile::tempdir().expect("create temp dir");
        let second = tempfile::tempdir().expect("create temp dir");
        fs::create_dir_all(first.path().join("Nord/gtk-4.0")).expect("create theme");
        fs::create_dir_all(first.path().join("OldGtk3/gtk-3.0")).expect("create theme");
        fs::create_dir_all(second.path().join("Nord/gtk-4.0")).expect("create duplicate");
        let missing = first.path().join("missing");
        let dirs = [
            first.path().to_path_buf(),
            second.path().to_path_buf(),
            missing,
        ];

        // Act
        let names = available_themes(&dirs);

        // Assert
        assert_eq!(names, ["Adwaita", "Adwaita-dark", "HighContrast", "Nord"]);
    }

    #[test]
    fn cursor_theme_installed_needs_cursors_or_index() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        fs::create_dir_all(temp.path().join("WithCursors/cursors")).expect("create theme");
        fs::create_dir_all(temp.path().join("Inheriting")).expect("create theme");
        fs::write(temp.path().join("Inheriting/index.theme"), "").expect("write index");
        fs::create_dir_all(temp.path().join("IconsOnly/48x48")).expect("create theme");
        let dirs = [temp.path().join("missing"), temp.path().to_path_buf()];

        // Act + Assert
        assert!(cursor_theme_installed("WithCursors", &dirs));
        assert!(cursor_theme_installed("Inheriting", &dirs));
        assert!(!cursor_theme_installed("IconsOnly", &dirs));
        assert!(!cursor_theme_installed("DMZ-Black", &dirs));
    }

    #[test]
    fn action_target_round_trips() {
        for choice in [
            ThemeChoice::System,
            ThemeChoice::Named(String::from("Zorin")),
        ] {
            assert_eq!(
                ThemeChoice::from_action_target(choice.action_target()),
                choice
            );
        }
        assert_eq!(ThemeChoice::System.action_target(), "");
    }
}
