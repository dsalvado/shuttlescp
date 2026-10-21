//! Discovery and application of GTK themes, and the colours that preview each one.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use relm4::gtk;
use relm4::gtk::{gdk, glib};
use shuttlescp_core::config::ThemeChoice;

/// Themes compiled into GTK itself, available even when nothing is installed on disk.
const BUILTIN_THEMES: [&str; 3] = ["Adwaita", "Adwaita-dark", "HighContrast"];

/// Cursor theme bundled with GTK/GNOME, used when the configured one is missing.
const FALLBACK_CURSOR_THEME: &str = "Adwaita";

/// Preview colours (window background, accent) of the themes built into GTK, which
/// have no stylesheet on disk to read them from.
const BUILTIN_SWATCHES: [(&str, &str, &str); 3] = [
    ("Adwaita", "#fafafa", "#3584e4"),
    ("Adwaita-dark", "#242424", "#3584e4"),
    ("HighContrast", "#ffffff", "#1c71d8"),
];
/// Bytes of a theme's stylesheet read for its colours: themes define them at the top,
/// and whole stylesheets run to half a megabyte.
const SWATCH_READ_LIMIT: u64 = 64 * 1024;
/// Names a theme may give its window background, most specific first.
const BACKGROUND_NAMES: [&str; 3] = ["window_bg_color", "theme_bg_color", "bg_color"];
/// Names a theme may give its accent colour, most specific first.
const ACCENT_NAMES: [&str; 4] = [
    "accent_bg_color",
    "theme_selected_bg_color",
    "selected_bg_color",
    "accent_color",
];

/// Name of the GTK settings property holding the theme.
const THEME_PROPERTY: &str = "gtk-theme-name";

/// Directories GTK searches for themes, in GTK's own lookup order.
///
/// # Returns
///
/// `$XDG_DATA_HOME/themes`, `~/.themes`, then `themes` in each `$XDG_DATA_DIRS` entry
pub fn search_dirs() -> Vec<PathBuf> {
    data_dirs("themes")
}

/// The two colours that preview a theme in the theme menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swatch {
    /// The window background: light or dark at a glance.
    pub background: gdk::RGBA,
    /// The accent colour (selections, highlighted buttons).
    pub accent: gdk::RGBA,
}

/// The preview colours of a theme: built in for GTK's own themes, otherwise read from
/// the first `<dir>/<name>/gtk-4.0/gtk.css` found.
///
/// # Arguments
///
/// * `name` - Theme name
/// * `dirs` - Theme directories, as [`search_dirs`] gives them
///
/// # Returns
///
/// The colours, or `None` if the theme does not name them
pub fn swatch(name: &str, dirs: &[PathBuf]) -> Option<Swatch> {
    if let Some(&(_, background, accent)) = BUILTIN_SWATCHES
        .iter()
        .find(|(builtin, _, _)| *builtin == name)
    {
        return Some(Swatch {
            background: gdk::RGBA::parse(background).ok()?,
            accent: gdk::RGBA::parse(accent).ok()?,
        });
    }
    let css = dirs
        .iter()
        .map(|dir| dir.join(name).join("gtk-4.0/gtk.css"))
        .find(|path| path.is_file())
        .and_then(|path| read_start(&path))?;
    swatch_from_css(&css)
}

/// The preview colours named by a stylesheet's `@define-color` lines, following
/// references such as `@define-color theme_bg_color @window_bg_color;`.
///
/// # Arguments
///
/// * `css` - The stylesheet (its start is enough)
fn swatch_from_css(css: &str) -> Option<Swatch> {
    let defined: HashMap<&str, &str> = css
        .lines()
        .filter_map(|line| line.trim().strip_prefix("@define-color "))
        .filter_map(|rest| {
            let (name, value) = rest.split_once(char::is_whitespace)?;
            Some((name, value.trim().trim_end_matches(';').trim()))
        })
        .collect();
    let colour = |names: &[&str]| {
        names.iter().find_map(|&name| {
            // A value may name another colour; follow a few steps, never a loop.
            let mut value = *defined.get(name)?;
            for _ in 0..4 {
                match value.strip_prefix('@') {
                    Some(other) => value = defined.get(other)?,
                    None => break,
                }
            }
            gdk::RGBA::parse(value).ok()
        })
    };
    Some(Swatch {
        background: colour(&BACKGROUND_NAMES)?,
        accent: colour(&ACCENT_NAMES)?,
    })
}

/// The first [`SWATCH_READ_LIMIT`] bytes of a file, as text.
fn read_start(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(SWATCH_READ_LIMIT)
        .read_to_end(&mut bytes)
        .ok()?;
    // The limit can cut a multi-byte character in half; everything before it is kept.
    Some(String::from_utf8_lossy(&bytes).into_owned())
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
    fn swatch_colours_follow_references() {
        let css = "/* Zorin style */\n\
                   @define-color accent_bg_color #15a6f0;\n\
                   @define-color window_bg_color #eaf0f6;\n\
                   @define-color theme_bg_color @window_bg_color;\n\
                   @define-color theme_selected_bg_color @accent_bg_color;\n";

        let swatch = swatch_from_css(css).expect("colours");

        assert_eq!(
            swatch.background,
            gdk::RGBA::parse("#eaf0f6").expect("rgba")
        );
        assert_eq!(swatch.accent, gdk::RGBA::parse("#15a6f0").expect("rgba"));
    }

    #[test]
    fn swatch_uses_older_names_and_colour_words() {
        let css = "@define-color theme_bg_color white;\n@define-color theme_selected_bg_color @blue;\n@define-color blue #3465a4;";

        let swatch = swatch_from_css(css).expect("colours");

        assert_eq!(swatch.background, gdk::RGBA::WHITE);
        assert_eq!(swatch.accent, gdk::RGBA::parse("#3465a4").expect("rgba"));
    }

    #[test]
    fn swatch_needs_both_colours_and_survives_loops() {
        assert_eq!(
            swatch_from_css("@define-color window_bg_color #ffffff;"),
            None
        );
        assert_eq!(
            swatch_from_css(
                "@define-color window_bg_color @a;\n@define-color a @window_bg_color;\n\
                 @define-color accent_bg_color #000000;"
            ),
            None
        );
    }

    #[test]
    fn swatch_reads_a_theme_folder_and_knows_the_builtins() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::create_dir_all(dir.path().join("Nord/gtk-4.0")).expect("theme");
        fs::write(
            dir.path().join("Nord/gtk-4.0/gtk.css"),
            "@define-color window_bg_color #2e3440;\n@define-color accent_bg_color #88c0d0;\n",
        )
        .expect("css");
        let dirs = [dir.path().to_path_buf()];

        assert_eq!(
            swatch("Nord", &dirs).map(|s| s.background),
            gdk::RGBA::parse("#2e3440").ok()
        );
        assert!(swatch("Adwaita-dark", &dirs).is_some());
        assert_eq!(swatch("Missing", &dirs), None);
    }

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
}
