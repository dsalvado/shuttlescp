//! Listing of directories on the local filesystem.
//!
//! Free of any GUI code, so it can be unit tested and called from a background thread.

use std::fs::{self, DirEntry, Metadata};
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::entry::{EntryKind, FileEntry};

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
fn entry_from_dir_entry(entry: &DirEntry) -> FileEntry {
    // `fs::metadata` follows symlinks; if the target is missing (broken link) fall back
    // to the metadata of the link itself. `.ok()` turns a `Result` into an `Option`.
    let metadata: Option<Metadata> = fs::metadata(entry.path())
        .or_else(|_| entry.metadata())
        .ok();
    let kind = match metadata.as_ref().map(Metadata::is_dir) {
        Some(true) => EntryKind::Directory,
        Some(false) | None => EntryKind::File,
    };
    FileEntry::new(
        entry.file_name(),
        kind,
        metadata.as_ref().map_or(0, Metadata::len),
        metadata.and_then(|meta| meta.modified().ok()),
    )
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
            Ok(entry) => Some(entry_from_dir_entry(&entry)),
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

    #[cfg(unix)]
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
