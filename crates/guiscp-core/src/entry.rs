//! One file or folder in a listing, local or remote, and the orders lists are sorted in.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::time::SystemTime;

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
    /// Build an entry from its parts.
    ///
    /// # Arguments
    ///
    /// * `name` - File name
    /// * `kind` - Directory or file
    /// * `size` - Size in bytes
    /// * `modified` - Last modification time, if known
    pub fn new(name: OsString, kind: EntryKind, size: u64, modified: Option<SystemTime>) -> Self {
        Self {
            sort_key: name.to_string_lossy().to_lowercase(),
            name,
            kind,
            size,
            modified,
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

    /// Name order used by file explorers: directories first, then case-insensitive name,
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

    /// Size order: directories first, then smallest to largest, then by name.
    ///
    /// # Arguments
    ///
    /// * `other` - The entry to compare against
    pub fn size_order(&self, other: &Self) -> Ordering {
        self.kind
            .cmp(&other.kind)
            .then_with(|| self.size.cmp(&other.size))
            .then_with(|| self.listing_order(other))
    }

    /// Date order: oldest to newest (unknown dates first), then by name.
    ///
    /// # Arguments
    ///
    /// * `other` - The entry to compare against
    pub fn modified_order(&self, other: &Self) -> Ordering {
        self.modified
            .cmp(&other.modified)
            .then_with(|| self.listing_order(other))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    fn entry(name: &str, kind: EntryKind, size: u64, seconds: u64) -> FileEntry {
        FileEntry::new(
            OsString::from(name),
            kind,
            size,
            Some(UNIX_EPOCH + Duration::from_secs(seconds)),
        )
    }

    #[test]
    fn listing_order_puts_folders_first_and_ignores_case() {
        let (dir, upper, lower) = (
            entry("zdir", EntryKind::Directory, 0, 0),
            entry("A.txt", EntryKind::File, 0, 0),
            entry("b.txt", EntryKind::File, 0, 0),
        );
        assert_eq!(dir.listing_order(&upper), Ordering::Less);
        assert_eq!(upper.listing_order(&lower), Ordering::Less);
    }

    #[test]
    fn size_and_modified_orders() {
        let (small, big) = (
            entry("b", EntryKind::File, 1, 200),
            entry("a", EntryKind::File, 9, 100),
        );
        assert_eq!(small.size_order(&big), Ordering::Less);
        assert_eq!(big.modified_order(&small), Ordering::Less);
        let dir = entry("z", EntryKind::Directory, 999, 0);
        assert_eq!(dir.size_order(&small), Ordering::Less);
    }
}
