//! Windows: the icons File Explorer shows, for the file lists.
//!
//! They come from the system through the `file_icon_provider` crate, which keeps the
//! (unsafe) Windows calls out of this app, so a video shows the icon of the app that
//! opens it, as in Explorer. Windows only describes things that exist, so a file's icon
//! is looked up on an empty sample file with the same extension, kept in the app's
//! state folder: that works for files on a server too, and is kept per extension.
//! Programs, shortcuts and icon files on this computer carry their own icons, so they
//! are looked up themselves, as are special folders (Downloads, ...).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use relm4::gtk::{gdk, glib};
use shuttlescp_core::config;

/// Pixels wide of each icon: drawn at 16px, so it stays sharp on screens scaled to 200%.
const PIXELS: u16 = 32;
/// Folder of the sample files, in the app's state folder.
const SAMPLES_DIR: &str = "icon-samples";
/// Name of the sample files (`sample.mkv`, ...) and of the sample folder.
const SAMPLE_NAME: &str = "sample";
/// Longest extension looked up; longer ones are not file types.
const MAX_EXTENSION: usize = 16;
/// Extensions whose files each carry their own icon.
const OWN_ICON_EXTENSIONS: [&str; 3] = ["exe", "lnk", "ico"];

thread_local! {
    /// Icons already looked up (`None`: the system had none), by extension or path.
    /// GTK runs on one thread, so the cache lives there.
    static CACHE: RefCell<HashMap<String, Option<gdk::Texture>>> = RefCell::default();
}

/// Explorer's icon for a file of this name.
///
/// # Arguments
///
/// * `name` - The file's name
/// * `local_path` - The file itself, if it is on this computer (programs, shortcuts and
///   icon files show their own icons)
pub fn file(name: &OsStr, local_path: Option<&Path>) -> Option<gdk::Texture> {
    let extension = extension(name);
    if let Some(path) = local_path.filter(|_| OWN_ICON_EXTENSIONS.contains(&extension.as_str())) {
        return cached(format!("path:{}", path.display()), || {
            Some(path.to_path_buf())
        });
    }
    cached(format!("extension:{extension}"), || sample_file(&extension))
}

/// Explorer's icon for an ordinary folder.
pub fn folder() -> Option<gdk::Texture> {
    cached(String::from("folder"), || {
        let dir = samples_dir()?.join(SAMPLE_NAME);
        fs::create_dir_all(&dir)
            .inspect_err(|error| tracing::debug!("cannot create {}: {error}", dir.display()))
            .ok()?;
        Some(dir)
    })
}

/// Explorer's icon for a folder on this computer, such as Downloads with its own icon.
///
/// # Arguments
///
/// * `path` - The folder
pub fn local_folder(path: &Path) -> Option<gdk::Texture> {
    cached(format!("path:{}", path.display()), || {
        Some(path.to_path_buf())
    })
}

/// The lower-case extension of `name` if it can be a file type (letters, digits, `-`,
/// `_` and `+`, not too long), otherwise empty: such a file gets the plain file icon.
fn extension(name: &OsStr) -> String {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .filter(|extension| {
            extension.len() <= MAX_EXTENSION
                && extension
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+'))
        })
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// The sample file for an extension (empty: a file without one), created if needed.
fn sample_file(extension: &str) -> Option<PathBuf> {
    let name = if extension.is_empty() {
        String::from(SAMPLE_NAME)
    } else {
        format!("{SAMPLE_NAME}.{extension}")
    };
    let path = samples_dir()?.join(name);
    if !path.is_file() {
        fs::write(&path, b"")
            .inspect_err(|error| tracing::debug!("cannot create {}: {error}", path.display()))
            .ok()?;
    }
    Some(path)
}

/// The folder of the sample files, created if needed.
fn samples_dir() -> Option<PathBuf> {
    let dir = config::state_dir().join(SAMPLES_DIR);
    fs::create_dir_all(&dir)
        .inspect_err(|error| tracing::debug!("cannot create {}: {error}", dir.display()))
        .ok()?;
    Some(dir)
}

/// An icon from the cache, looked up first (on the file `look_up` names) if it is not
/// there yet.
fn cached(key: String, look_up: impl FnOnce() -> Option<PathBuf>) -> Option<gdk::Texture> {
    if let Some(texture) = CACHE.with_borrow(|cache| cache.get(&key).cloned()) {
        return texture;
    }
    let texture = look_up().and_then(|path| icon_of(&path));
    CACHE.with_borrow_mut(|cache| cache.insert(key, texture.clone()));
    texture
}

/// The system's icon of an existing file or folder, as a GTK texture.
fn icon_of(path: &Path) -> Option<gdk::Texture> {
    let icon = file_icon_provider::get_file_icon(path, PIXELS)
        .inspect_err(|error| tracing::debug!("no icon for {}: {error}", path.display()))
        .ok()?;
    let stride = usize::try_from(icon.width).ok()? * 4;
    let width = i32::try_from(icon.width).ok()?;
    let height = i32::try_from(icon.height).ok()?;
    // Windows hands over icons with their colours already multiplied by the opacity.
    let texture = gdk::MemoryTexture::new(
        width,
        height,
        gdk::MemoryFormat::R8g8b8a8Premultiplied,
        &glib::Bytes::from_owned(icon.pixels),
        stride,
    );
    Some(texture.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_file_types_only() {
        assert_eq!(extension(OsStr::new("Movie.MKV")), "mkv");
        assert_eq!(extension(OsStr::new("archive.tar.gz")), "gz");
        assert_eq!(extension(OsStr::new("README")), "");
        assert_eq!(
            extension(OsStr::new("odd.na:me")),
            "",
            "not a valid name here"
        );
        assert_eq!(extension(OsStr::new("notes.this-is-not-a-type-at-all")), "");
    }
}
