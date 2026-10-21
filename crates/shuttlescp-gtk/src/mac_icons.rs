//! macOS: the icons Finder shows, for the file lists.
//!
//! They come from the system (NSWorkspace), so a video shows the icon of the app that
//! opens it, as in Finder. A file's icon depends only on its type, so it is looked up
//! by the name's extension, which works for files on a server too, and kept per
//! extension. Special folders (Downloads, ...) are looked up by path.
//!
//! The system hands over each icon in every size up to 1024px (about 37 MB as an
//! image), so only the one size the list needs is kept: 32px, drawn at 16pt (sharp on
//! Retina screens).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;

use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{NSImage, NSWorkspace};
use objc2_foundation::{NSSize, NSString};
use objc2_uniform_type_identifiers::UTType;
use relm4::gtk::{gdk, glib};

/// Pixels wide of the size kept from each icon.
const PIXELS: isize = 32;
/// Size the icon is drawn at, in points.
const POINTS: f64 = 16.0;
/// Type of files whose extension the system does not know, or that have none.
const UNKNOWN_FILE_TYPE: &str = "public.data";
/// Type of ordinary folders.
const FOLDER_TYPE: &str = "public.folder";

thread_local! {
    /// Icons already looked up (`None`: the system had none), by extension or path.
    /// GTK runs on one thread, so the cache lives there.
    static CACHE: RefCell<HashMap<String, Option<gdk::Texture>>> = RefCell::default();
}

/// Finder's icon for a file of this name.
///
/// # Arguments
///
/// * `name` - The file's name
pub fn file(name: &OsStr) -> Option<gdk::Texture> {
    let extension = Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_lowercase)
        .unwrap_or_default();
    cached(format!("extension:{extension}"), || {
        let known = (!extension.is_empty())
            .then(|| UTType::typeWithFilenameExtension(&NSString::from_str(&extension)))
            .flatten();
        let file_type =
            known.or_else(|| UTType::typeWithIdentifier(&NSString::from_str(UNKNOWN_FILE_TYPE)))?;
        Some(NSWorkspace::sharedWorkspace().iconForContentType(&file_type))
    })
}

/// Finder's icon for an ordinary folder.
pub fn folder() -> Option<gdk::Texture> {
    cached(String::from("folder"), || {
        let folder_type = UTType::typeWithIdentifier(&NSString::from_str(FOLDER_TYPE))?;
        Some(NSWorkspace::sharedWorkspace().iconForContentType(&folder_type))
    })
}

/// Finder's icon for a folder on this Mac, such as Downloads with its own icon.
///
/// # Arguments
///
/// * `path` - The folder
pub fn local_folder(path: &Path) -> Option<gdk::Texture> {
    let path = path.to_str()?;
    cached(format!("path:{path}"), || {
        Some(NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path)))
    })
}

/// An icon from the cache, looked up first if it is not there yet.
fn cached(
    key: String,
    look_up: impl FnOnce() -> Option<Retained<NSImage>>,
) -> Option<gdk::Texture> {
    if let Some(texture) = CACHE.with_borrow(|cache| cache.get(&key).cloned()) {
        return texture;
    }
    let texture = look_up().and_then(|icon| texture(&icon));
    CACHE.with_borrow_mut(|cache| cache.insert(key, texture.clone()));
    texture
}

/// The icon at the list's size, as a GTK texture.
fn texture(icon: &NSImage) -> Option<gdk::Texture> {
    // The smallest size at least PIXELS wide, else the largest there is.
    let sizes = icon.representations();
    let size = sizes
        .iter()
        .filter(|size| size.pixelsWide() >= PIXELS)
        .min_by_key(|size| size.pixelsWide())
        .or_else(|| sizes.iter().max_by_key(|size| size.pixelsWide()))?;
    let single = NSImage::initWithSize(NSImage::alloc(), NSSize::new(POINTS, POINTS));
    single.addRepresentation(&size);
    let tiff = single.TIFFRepresentation()?.to_vec();
    gdk::Texture::from_bytes(&glib::Bytes::from_owned(tiff))
        .inspect_err(|error| tracing::warn!("cannot read a macOS icon: {error}"))
        .ok()
}
