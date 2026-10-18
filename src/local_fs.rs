//! Listing of directories on the local filesystem.
//!
//! This module is deliberately free of any GUI code so it can be unit tested and
//! called from a background thread.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs::{self, DirEntry, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use thiserror::Error;

/// Whether an entry can be navigated into or not.
///
/// The variant order matters: deriving `Ord` compares variants in declaration
/// order, so directories sort before files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    /// A directory, or a symbolic link pointing to one.
    Directory,
    /// Anything else (regular files, devices, broken links, ...).
    File,
}

/// A single item inside a listed directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Raw file name. Unix file names are arbitrary bytes, not guaranteed UTF-8,
    /// so we keep the original `OsString` to be able to rebuild the exact path.
    name: OsString,
    /// Lower-cased display name, computed once so sorting never allocates.
    sort_key: String,
    kind: EntryKind,
    size: u64,
    modified: Option<SystemTime>,
}

impl FileEntry {
    /// Build an entry from a `read_dir` result, following symbolic links so a link to a
    /// directory is reported as a directory.
    ///
    /// # Arguments
    ///
    /// * `entry` - The directory entry returned by [`fs::read_dir`]
    ///
    /// # Returns
    ///
    /// The entry; metadata fields fall back to empty values when they cannot be read
    fn from_dir_entry(entry: &DirEntry) -> Self {
        // `fs::metadata` follows symlinks; if the target is missing (broken link) fall back
        // to the metadata of the link itself. `.ok()` turns a `Result` into an `Option`.
        let metadata: Option<Metadata> = fs::metadata(entry.path())
            .or_else(|_| entry.metadata())
            .ok();
        let kind = match metadata.as_ref().map(Metadata::is_dir) {
            Some(true) => EntryKind::Directory,
            Some(false) | None => EntryKind::File,
        };
        let name = entry.file_name();
        Self {
            sort_key: name.to_string_lossy().to_lowercase(),
            name,
            kind,
            size: metadata.as_ref().map_or(0, Metadata::len),
            modified: metadata.and_then(|meta| meta.modified().ok()),
        }
    }

    /// Raw file name, suitable for joining onto the parent directory path.
    pub fn name(&self) -> &OsString {
        &self.name
    }

    /// File name for display; invalid UTF-8 sequences are replaced with U+FFFD.
    ///
    /// # Returns
    ///
    /// A borrowed `&str` when the name is valid UTF-8 (the common case, no allocation),
    /// otherwise an owned, repaired `String`
    pub fn display_name(&self) -> std::borrow::Cow<'_, str> {
        self.name.to_string_lossy()
    }

    /// Whether the entry is a directory or a file.
    pub fn kind(&self) -> EntryKind {
        self.kind
    }

    /// Size in bytes (0 when unknown).
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Last modification time, if the filesystem reports one.
    pub fn modified(&self) -> Option<SystemTime> {
        self.modified
    }

    /// Ordering used by file explorers: directories first, then case-insensitive name,
    /// with the raw name as a tie-breaker so the order is total and stable.
    ///
    /// # Arguments
    ///
    /// * `other` - The entry to compare against
    ///
    /// # Returns
    ///
    /// The relative order of `self` and `other`
    pub fn listing_order(&self, other: &Self) -> Ordering {
        self.kind
            .cmp(&other.kind)
            .then_with(|| self.sort_key.cmp(&other.sort_key))
            .then_with(|| self.name.cmp(&other.name))
    }
}

/// Error returned when a directory cannot be listed.
#[derive(Debug, Error)]
#[error("cannot open {}: {source}", path.display())]
pub struct ListError {
    path: PathBuf,
    #[source]
    source: io::Error,
}

/// List the contents of a directory, sorted with [`FileEntry::listing_order`].
///
/// Individual entries that vanish or fail while iterating are skipped and logged.
///
/// # Arguments
///
/// * `dir` - Directory to list
///
/// # Returns
///
/// The sorted entries of `dir`
///
/// # Errors
///
/// Returns [`ListError`] if `dir` does not exist, is not a directory, or is not readable
pub fn list_dir(dir: &Path) -> Result<Vec<FileEntry>, ListError> {
    let reader = fs::read_dir(dir).map_err(|source| ListError {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut entries: Vec<FileEntry> = reader
        .filter_map(|result| match result {
            Ok(entry) => Some(FileEntry::from_dir_entry(&entry)),
            Err(error) => {
                tracing::warn!("skipping entry in {}: {error}", dir.display());
                None
            }
        })
        .collect();
    entries.sort_unstable_by(FileEntry::listing_order);
    tracing::trace!("read {} entries from {}", entries.len(), dir.display());
    Ok(entries)
}

/// Find the closest directory at or above `path` that can be opened for listing.
///
/// # Arguments
///
/// * `path` - Starting point, e.g. a directory saved in a previous session
///
/// # Returns
///
/// `path` itself or its nearest listable ancestor, or `None` if none of them can be listed
///
/// # Examples
///
/// ```ignore
/// // If /home/me/gone was deleted, this yields /home/me.
/// let dir = nearest_listable_dir(Path::new("/home/me/gone/sub"));
/// ```
pub fn nearest_listable_dir(path: &Path) -> Option<&Path> {
    // `ancestors()` yields the path, then its parent, grandparent, ... up to the root.
    path.ancestors().find(|dir| fs::read_dir(dir).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[FileEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| e.display_name().into_owned())
            .collect()
    }

    #[test]
    fn lists_directories_first_then_case_insensitive_names() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        fs::write(temp.path().join("b.txt"), b"12345").expect("write file");
        fs::write(temp.path().join("A.txt"), b"").expect("write file");
        fs::create_dir(temp.path().join("zdir")).expect("create dir");

        // Act
        let entries = list_dir(temp.path()).expect("list temp dir");

        // Assert
        assert_eq!(names(&entries), ["zdir", "A.txt", "b.txt"]);
        assert_eq!(entries[0].kind(), EntryKind::Directory);
        assert_eq!(entries[2].kind(), EntryKind::File);
        assert_eq!(entries[2].size(), 5);
        assert!(entries[2].modified().is_some());
    }

    #[test]
    fn symlink_to_directory_is_a_directory_and_broken_link_is_a_file() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        fs::create_dir(temp.path().join("real")).expect("create dir");
        std::os::unix::fs::symlink(temp.path().join("real"), temp.path().join("link"))
            .expect("create link");
        std::os::unix::fs::symlink(temp.path().join("missing"), temp.path().join("broken"))
            .expect("create broken link");

        // Act
        let entries = list_dir(temp.path()).expect("list temp dir");

        // Assert
        assert_eq!(names(&entries), ["link", "real", "broken"]);
        assert_eq!(entries[0].kind(), EntryKind::Directory);
        assert_eq!(entries[2].kind(), EntryKind::File);
    }

    #[test]
    fn nearest_listable_dir_walks_up_to_an_existing_directory() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        let existing = temp.path().join("kept");
        fs::create_dir(&existing).expect("create dir");
        let file = existing.join("file.txt");
        fs::write(&file, b"").expect("write file");

        // Act + Assert
        assert_eq!(nearest_listable_dir(&existing), Some(existing.as_path()));
        assert_eq!(
            nearest_listable_dir(&existing.join("gone/deeper")),
            Some(existing.as_path())
        );
        assert_eq!(nearest_listable_dir(&file), Some(existing.as_path()));
    }

    #[test]
    fn nearest_listable_dir_is_none_when_nothing_exists() {
        assert_eq!(nearest_listable_dir(Path::new("gone/deeper")), None);
    }

    #[test]
    fn missing_directory_reports_its_path() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        let missing = temp.path().join("nope");

        // Act
        let error = list_dir(&missing).expect_err("listing must fail");

        // Assert
        assert!(error.to_string().contains("nope"));
    }
}
