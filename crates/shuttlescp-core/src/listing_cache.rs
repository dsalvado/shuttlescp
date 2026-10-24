//! Server folder listings saved between sessions, so a folder on a slow or distant
//! server can be shown at once while it is read again.
//!
//! Each site has its own folder under the app's cache folder, with one small JSON file
//! per server folder (named after a hash of its path). Writing a listing marks it as
//! recently used; past [`MAX_FOLDERS`] per site, the least recently written are deleted.
//! The files hold file names from the server, so on Unix only the user can read them.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config;
use crate::entry::{EntryKind, FileEntry};
use crate::path::RemotePath;

/// Most folders kept per site.
pub const MAX_FOLDERS: usize = 200;

/// A listing read back: when it was saved, and its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedListing {
    /// When the listing was read from the server.
    pub saved_at: SystemTime,
    /// The folder's entries, in listing order.
    pub entries: Vec<FileEntry>,
}

/// The saved listings of one site.
#[derive(Debug, Clone)]
pub struct ListingCache {
    dir: PathBuf,
    limit: usize,
}

/// A listing as stored on disk.
#[derive(Serialize, Deserialize)]
struct Stored {
    /// The folder, to tell apart two paths whose hashes would clash.
    path: String,
    /// Seconds since 1970.
    saved_at: u64,
    entries: Vec<StoredEntry>,
}

/// One entry as stored on disk.
#[derive(Serialize, Deserialize)]
struct StoredEntry {
    name: String,
    folder: bool,
    size: u64,
    /// Seconds since 1970, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modified: Option<u64>,
}

impl ListingCache {
    /// The saved listings of the site with this id.
    pub fn for_site(site_id: &str) -> Self {
        Self::in_dir(site_dir(site_id), MAX_FOLDERS)
    }

    /// Listings kept in `dir`, at most `limit` of them (for tests).
    pub fn in_dir(dir: PathBuf, limit: usize) -> Self {
        Self { dir, limit }
    }

    /// The saved listing of `path`, if there is one (an unreadable one counts as none).
    pub fn load(&self, path: &RemotePath) -> Option<SavedListing> {
        let text = fs::read_to_string(self.file(path)).ok()?;
        let stored: Stored = serde_json::from_str(&text)
            .inspect_err(|error| tracing::debug!("ignoring saved listing of {path}: {error}"))
            .ok()?;
        (stored.path == path.as_str()).then(|| SavedListing {
            saved_at: from_seconds(stored.saved_at),
            entries: stored
                .entries
                .into_iter()
                .map(|entry| {
                    FileEntry::new(
                        entry.name.into(),
                        if entry.folder {
                            EntryKind::Directory
                        } else {
                            EntryKind::File
                        },
                        entry.size,
                        entry.modified.map(from_seconds),
                    )
                })
                .collect(),
        })
    }

    /// Save the listing of `path` just read from the server, then delete the least
    /// recently saved listings past the limit.
    ///
    /// # Errors
    ///
    /// Returns the error if the folder or file cannot be written
    pub fn store(&self, path: &RemotePath, entries: &[FileEntry]) -> io::Result<()> {
        let stored = Stored {
            path: path.as_str().to_owned(),
            saved_at: seconds(SystemTime::now()).unwrap_or(0),
            entries: entries
                .iter()
                .map(|entry| StoredEntry {
                    name: entry.display_name().into_owned(),
                    folder: entry.kind() == EntryKind::Directory,
                    size: entry.size(),
                    modified: entry.modified().and_then(seconds),
                })
                .collect(),
        };
        create_private_dir(&self.dir)?;
        let file = self.file(path);
        // Written beside it and renamed, so a reader never sees half a file.
        let partial = file.with_extension("partial");
        write_private(
            &partial,
            &serde_json::to_vec(&stored).map_err(io::Error::other)?,
        )?;
        fs::rename(&partial, &file)?;
        self.prune();
        Ok(())
    }

    /// Delete the least recently saved listings past the limit.
    fn prune(&self) {
        let Ok(files) = fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(SystemTime, PathBuf)> = files
            .filter_map(Result::ok)
            .filter(|file| file.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|file| Some((file.metadata().ok()?.modified().ok()?, file.path())))
            .collect();
        if files.len() <= self.limit {
            return;
        }
        files.sort_unstable();
        for (_, file) in &files[..files.len() - self.limit] {
            if let Err(error) = fs::remove_file(file) {
                tracing::debug!("cannot delete saved listing {}: {error}", file.display());
            }
        }
    }

    /// The file holding the listing of `path`.
    fn file(&self, path: &RemotePath) -> PathBuf {
        self.dir.join(format!("{}.json", short_hash(path.as_str())))
    }
}

/// Delete all the saved listings of a site (it was deleted).
pub fn forget_site(site_id: &str) {
    let dir = site_dir(site_id);
    match fs::remove_dir_all(&dir) {
        Ok(()) => tracing::info!("deleted the saved folder listings of a deleted site"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!("cannot delete {}: {error}", dir.display()),
    }
}

/// The folder of a site's listings, named after a hash of its id.
fn site_dir(site_id: &str) -> PathBuf {
    config::cache_dir()
        .join("listings")
        .join(short_hash(site_id))
}

/// The first 128 bits of the SHA-256 of `text`, in hex: a safe file name.
fn short_hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())[..16]
        .iter()
        .fold(String::with_capacity(32), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// Seconds since 1970, if the time is after it.
fn seconds(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|since| since.as_secs())
}

fn from_seconds(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

/// Create `dir` (and its parents), readable only by the user on Unix.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    fs::create_dir_all(dir)
}

/// Write `bytes` to a new file readable only by the user on Unix.
fn write_private(file: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(file)?
            .write_all(bytes)
    }
    #[cfg(not(unix))]
    fs::write(file, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: EntryKind, size: u64, modified: Option<u64>) -> FileEntry {
        FileEntry::new(name.into(), kind, size, modified.map(from_seconds))
    }

    #[test]
    fn a_listing_reads_back_as_saved() {
        let temp = tempfile::tempdir().expect("temp dir");
        let cache = ListingCache::in_dir(temp.path().join("site"), 10);
        let path = RemotePath::new("/media/store01");
        let entries = vec![
            entry("videos", EntryKind::Directory, 0, Some(1_700_000_000)),
            entry("notes.txt", EntryKind::File, 1234, None),
        ];
        assert_eq!(cache.load(&path), None);

        cache.store(&path, &entries).expect("store");
        let saved = cache.load(&path).expect("saved listing");
        assert_eq!(saved.entries, entries);
        assert!(saved.saved_at <= SystemTime::now());
        assert_eq!(cache.load(&RemotePath::new("/media")), None);
    }

    #[cfg(unix)]
    #[test]
    fn only_the_user_can_read_saved_listings() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("temp dir");
        let cache = ListingCache::in_dir(temp.path().join("site"), 10);
        let path = RemotePath::new("/");
        cache.store(&path, &[]).expect("store");
        let mode = |file: &Path| fs::metadata(file).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&temp.path().join("site")), 0o700);
        assert_eq!(mode(&cache.file(&path)), 0o600);
    }

    #[test]
    fn the_least_recently_saved_are_deleted_past_the_limit() {
        let temp = tempfile::tempdir().expect("temp dir");
        let cache = ListingCache::in_dir(temp.path().to_path_buf(), 2);
        let (first, second, third) = (
            RemotePath::new("/a"),
            RemotePath::new("/b"),
            RemotePath::new("/c"),
        );
        for (age, path) in [(30, &first), (20, &second)] {
            cache.store(path, &[]).expect("store");
            // Older modification times, so the order does not depend on the clock's
            // resolution.
            let when = SystemTime::now() - Duration::from_secs(age);
            fs::File::options()
                .write(true)
                .open(cache.file(path))
                .and_then(|file| file.set_modified(when))
                .expect("set time");
        }
        cache.store(&third, &[]).expect("store");

        assert_eq!(cache.load(&first), None, "the oldest is deleted");
        assert!(cache.load(&second).is_some());
        assert!(cache.load(&third).is_some());
    }

    #[test]
    fn a_damaged_file_counts_as_no_listing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let cache = ListingCache::in_dir(temp.path().to_path_buf(), 10);
        let path = RemotePath::new("/x");
        fs::write(cache.file(&path), "not json").expect("write");
        assert_eq!(cache.load(&path), None);
    }
}
