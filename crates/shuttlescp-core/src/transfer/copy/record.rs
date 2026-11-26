//! What is done of each copy in parts under way, so a paused or interrupted one is
//! continued rather than started over: the partial copy is full-sized from the start,
//! so its length says nothing (unlike a whole file's).
//!
//! Kept on this computer (`split-copies.json` in the app's state folder), so nothing
//! more is left on servers. Without a record, a copy in parts starts over: a copy with
//! gaps is never put in place.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::parts::Part;
use crate::config;
use crate::transfer::PlanItem;

/// Records older than this are dropped when loading: their jobs were abandoned.
const KEEP_FOR: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// One file's copy in parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SplitRecord {
    /// The source's size and date (seconds since 1970) when copying began: if either
    /// changed, the copy starts over.
    pub(super) size: u64,
    pub(super) modified: Option<u64>,
    /// When the record was last written, in seconds since 1970.
    pub(super) saved_at: u64,
    /// Each part's range and what is done of it.
    pub(super) parts: Vec<Part>,
}

impl SplitRecord {
    /// A record for `item` in `parts`.
    pub(super) fn new(item: &PlanItem, parts: Vec<Part>) -> Self {
        Self {
            size: item.size,
            modified: seconds(item.modified),
            saved_at: seconds(Some(SystemTime::now())).unwrap_or(0),
            parts,
        }
    }

    /// Whether this record is for `item` as it is now (same size and date).
    pub(super) fn fits(&self, item: &PlanItem) -> bool {
        self.size == item.size && self.modified == seconds(item.modified)
    }
}

/// `time` in whole seconds since 1970.
fn seconds(time: Option<SystemTime>) -> Option<u64> {
    time?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|since| since.as_secs())
}

/// The records of the copies in parts under way, by file (see [`key`]), written to
/// their file on every change. One is shared by every job of the app.
#[derive(Debug, Default)]
pub struct SplitRecords {
    /// Where they are saved; `None` keeps them in memory only (for one run).
    path: Option<PathBuf>,
    records: Mutex<BTreeMap<String, SplitRecord>>,
}

impl SplitRecords {
    /// Default location: `split-copies.json` in the app's state folder.
    pub fn default_path() -> PathBuf {
        config::state_dir().join("split-copies.json")
    }

    /// The records saved at `path` (kept there from now on). A missing or unreadable
    /// file gives none (logged): copies in parts then start over.
    pub fn load(path: &Path) -> Self {
        let mut records: BTreeMap<String, SplitRecord> =
            config::load_json(path).unwrap_or_else(|error| {
                tracing::warn!("{error}; copies in parts under way start over");
                BTreeMap::new()
            });
        let now = seconds(Some(SystemTime::now())).unwrap_or(0);
        records.retain(|_, record| now.saturating_sub(record.saved_at) < KEEP_FOR.as_secs());
        Self {
            path: Some(path.to_path_buf()),
            records: Mutex::new(records),
        }
    }

    /// The app's records, at [`Self::default_path`]: one for every job, loaded the first
    /// time they are asked for.
    pub fn shared() -> Arc<Self> {
        static SHARED: OnceLock<Arc<SplitRecords>> = OnceLock::new();
        Arc::clone(SHARED.get_or_init(|| Arc::new(Self::load(&Self::default_path()))))
    }

    /// Records kept in memory only: a paused copy in parts continues within this run.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// The record for `key`, if there is one.
    pub(super) fn get(&self, key: &str) -> Option<SplitRecord> {
        self.records.lock().ok()?.get(key).cloned()
    }

    /// Record `record` for `key`, replacing what was there, and save.
    pub(super) fn set(&self, key: &str, mut record: SplitRecord) {
        record.saved_at = seconds(Some(SystemTime::now())).unwrap_or(0);
        self.change(|records| {
            records.insert(key.to_owned(), record);
        });
    }

    /// Forget the record for `key` (the copy is done, or gone), and save.
    pub(super) fn forget(&self, key: &str) {
        self.change(|records| {
            records.remove(key);
        });
    }

    /// Apply `change`, then write the records to their file (if they have one). The
    /// lock is held while writing, so writes never overtake each other.
    fn change(&self, change: impl FnOnce(&mut BTreeMap<String, SplitRecord>)) {
        let Ok(mut records) = self.records.lock() else {
            return;
        };
        change(&mut records);
        if let Some(path) = &self.path
            && let Err(error) = config::save_json(&*records, path)
        {
            // Only resuming is lost: the copy itself goes on.
            tracing::warn!("{error}");
        }
    }
}

/// What a file's record is filed under: where it is copied from and to.
pub(super) fn key(item: &PlanItem) -> String {
    format!("{} -> {}", item.source, item.dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::Checksum;
    use crate::transfer::copy::parts::Piece;

    fn record(saved_at: u64) -> SplitRecord {
        let sha256 = Checksum::from_hex(&"ab".repeat(32)).expect("hex");
        SplitRecord {
            size: 100,
            modified: Some(1_700_000_000),
            saved_at,
            parts: vec![
                Part {
                    start: 0,
                    end: 50,
                    pieces: vec![Piece {
                        start: 0,
                        end: 20,
                        sha256,
                    }],
                },
                Part {
                    start: 50,
                    end: 100,
                    pieces: Vec::new(),
                },
            ],
        }
    }

    #[test]
    fn records_are_saved_and_read_back_and_forgotten() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("split-copies.json");
        let records = SplitRecords::load(&path);

        records.set("a -> b", record(0));
        let loaded = SplitRecords::load(&path).get("a -> b");
        records.forget("a -> b");

        let mut expected = record(0);
        expected.saved_at = loaded.as_ref().map_or(0, |loaded| loaded.saved_at);
        assert_eq!(loaded, Some(expected));
        assert_eq!(SplitRecords::load(&path).get("a -> b"), None);
    }

    #[test]
    fn old_records_are_dropped_and_a_damaged_file_gives_none() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("split-copies.json");
        let now = seconds(Some(SystemTime::now())).expect("now");
        let old: BTreeMap<_, _> = [
            ("old".to_owned(), record(now - KEEP_FOR.as_secs() - 1)),
            ("new".to_owned(), record(now)),
        ]
        .into();
        config::save_json(&old, &path).expect("save");

        let records = SplitRecords::load(&path);

        assert!(records.get("old").is_none());
        assert!(records.get("new").is_some());
        std::fs::write(&path, "not json").expect("write");
        assert!(SplitRecords::load(&path).get("new").is_none());
    }

    #[test]
    fn records_in_memory_write_nothing() {
        let records = SplitRecords::in_memory();
        records.set("a -> b", record(0));
        assert!(records.get("a -> b").is_some());
        assert!(records.path.is_none());
    }
}
