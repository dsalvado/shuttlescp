//! Listing of directories on the local filesystem.
//!
//! Free of any GUI code, so it can be unit tested and called from a background thread.

#[cfg(unix)]
use std::collections::HashMap;
use std::fs::{self, DirEntry, Metadata};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use nix::unistd::{Gid, Group, Uid, User};
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
fn entry_from_dir_entry(entry: &DirEntry, names: &mut Names) -> FileEntry {
    // `fs::metadata` follows symlinks; if the target is missing (broken link) fall back
    // to the metadata of the link itself. `.ok()` turns a `Result` into an `Option`.
    let metadata: Option<Metadata> = fs::metadata(entry.path())
        .or_else(|_| entry.metadata())
        .ok();
    let kind = match metadata.as_ref().map(Metadata::is_dir) {
        Some(true) => EntryKind::Directory,
        Some(false) | None => EntryKind::File,
    };
    let plain = FileEntry::new(
        entry.file_name(),
        kind,
        metadata.as_ref().map_or(0, Metadata::len),
        metadata.as_ref().and_then(|meta| meta.modified().ok()),
    );
    names.add_unix_details(plain, metadata.as_ref())
}

/// User and group names by number, each looked up once per listing: a lookup asks the
/// system's user database, which can be slow (or on the network, with LDAP).
#[cfg(unix)]
#[derive(Default)]
struct Names {
    users: HashMap<u32, String>,
    groups: HashMap<u32, String>,
}

#[cfg(unix)]
impl Names {
    /// `entry` with its permission bits, owner and group from `metadata`.
    fn add_unix_details(&mut self, entry: FileEntry, metadata: Option<&Metadata>) -> FileEntry {
        use std::os::unix::fs::MetadataExt;
        let Some(meta) = metadata else {
            return entry;
        };
        let owner = self.user(meta.uid());
        let group = self.group(meta.gid());
        entry.with_unix(Some(meta.mode()), Some(owner), Some(group))
    }

    /// The user's name, or the number as text for a user the system doesn't know
    /// (files from another computer, or a deleted account).
    fn user(&mut self, uid: u32) -> String {
        self.users
            .entry(uid)
            .or_insert_with(|| {
                User::from_uid(Uid::from_raw(uid))
                    .ok()
                    .flatten()
                    .map_or_else(|| uid.to_string(), |user| user.name)
            })
            .clone()
    }

    /// The group's name, or the number as text.
    fn group(&mut self, gid: u32) -> String {
        self.groups
            .entry(gid)
            .or_insert_with(|| {
                Group::from_gid(Gid::from_raw(gid))
                    .ok()
                    .flatten()
                    .map_or_else(|| gid.to_string(), |group| group.name)
            })
            .clone()
    }
}

/// Windows has no Unix permissions: entries stay without them.
#[cfg(not(unix))]
#[derive(Default)]
struct Names;

#[cfg(not(unix))]
impl Names {
    fn add_unix_details(&mut self, entry: FileEntry, _metadata: Option<&Metadata>) -> FileEntry {
        entry
    }
}

/// Error returned when a directory cannot be listed.
#[derive(Debug, Error)]
pub struct ListError {
    path: PathBuf,
    #[source]
    source: io::Error,
}

// Written by hand: the message is in the interface language.
impl std::fmt::Display for ListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&crate::i18n::cannot(
            "open",
            self.path.display(),
            &self.source,
        ))
    }
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
    let mut names = Names::default();
    let mut entries: Vec<FileEntry> = reader
        .filter_map(|result| match result {
            Ok(entry) => Some(entry_from_dir_entry(&entry, &mut names)),
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

/// Change a local file's or folder's permission bits (Linux and macOS). A link is
/// followed: the file it points to changes.
///
/// # Arguments
///
/// * `path` - File or folder
/// * `mode` - New permission bits; only the lowest 12 are used
///
/// # Errors
///
/// Returns a readable message if the system refuses (usually: not the owner)
#[cfg(unix)]
pub fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    tracing::info!(
        "setting permissions of {} to {:04o}",
        path.display(),
        mode & 0o7777
    );
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o7777))
        .map_err(|error| crate::i18n::cannot("set-permissions", path.display(), error))
}

/// Change a local file's or folder's owner and/or group, by number; `None` keeps that
/// one (Linux and macOS). Giving a file to someone else needs root.
///
/// # Arguments
///
/// * `path` - File or folder
/// * `uid` - New owner, or `None` to keep it
/// * `gid` - New group, or `None` to keep it
///
/// # Errors
///
/// Returns a readable message if the system refuses
#[cfg(unix)]
pub fn set_owner(path: &Path, uid: Option<u32>, gid: Option<u32>) -> Result<(), String> {
    tracing::info!(
        "setting owner of {} to uid {uid:?}, gid {gid:?}",
        path.display()
    );
    std::os::unix::fs::chown(path, uid, gid)
        .map_err(|error| crate::i18n::cannot("set-owner", path.display(), error))
}

/// The number of the user named `name` on this computer, or `name` itself if it is a
/// number (Linux and macOS).
#[cfg(unix)]
pub fn user_id(name: &str) -> Option<u32> {
    name.parse().ok().or_else(|| {
        User::from_name(name)
            .ok()
            .flatten()
            .map(|user| user.uid.as_raw())
    })
}

/// The number of the group named `name` on this computer, or `name` itself if it is a
/// number (Linux and macOS).
#[cfg(unix)]
pub fn group_id(name: &str) -> Option<u32> {
    name.parse().ok().or_else(|| {
        Group::from_name(name)
            .ok()
            .flatten()
            .map(|group| group.gid.as_raw())
    })
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

    #[cfg(unix)]
    #[test]
    fn entries_carry_permissions_and_owner_names() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = temp.path().join("secret.txt");
        fs::write(&file, b"").expect("write file");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).expect("chmod");

        let entries = list_dir(temp.path()).expect("list temp dir");

        assert_eq!(entries[0].mode(), Some(0o640));
        // The file is ours, so the owner is our own user's name.
        let me = User::from_uid(Uid::current())
            .expect("lookup")
            .expect("known user");
        assert_eq!(entries[0].owner(), Some(me.name.as_str()));
        assert!(entries[0].group().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn changes_mode_and_keeps_the_owner_when_asked_to() {
        use std::os::unix::fs::MetadataExt;
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = temp.path().join("script.sh");
        fs::write(&file, b"").expect("write file");

        set_mode(&file, 0o750).expect("chmod");
        let meta = fs::metadata(&file).expect("metadata");
        assert_eq!(meta.mode() & 0o7777, 0o750);

        // "Change" the group to the one it already has: allowed without root, and the
        // owner (None) must stay as it was.
        set_owner(&file, None, Some(meta.gid())).expect("chgrp to own group");
        let after = fs::metadata(&file).expect("metadata");
        assert_eq!((after.uid(), after.gid()), (meta.uid(), meta.gid()));
    }

    #[cfg(unix)]
    #[test]
    fn refusals_name_the_file() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let gone = temp.path().join("gone.txt");
        let error = set_mode(&gone, 0o644).expect_err("no such file");
        assert!(error.contains("gone.txt"), "{error}");
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
