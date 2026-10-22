//! Transfers that have not finished, kept on disk so they can be resumed after the app
//! was closed (or crashed), or the connection dropped.
//!
//! A job is recorded when it starts and removed once it completes or the user cancels
//! it. Whatever is left belongs to interrupted or failed jobs, which front ends offer
//! to resume the next time the same site is connected.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{self, ConfigError};
use crate::transfer::Job;

/// An unfinished transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingJob {
    /// Identifies this entry (it stays the same when the job is resumed).
    pub key: String,
    /// The site it transfers to or from.
    pub site_id: String,
    /// What it does.
    pub job: Job,
}

/// All unfinished transfers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTransfers {
    jobs: Vec<PendingJob>,
}

impl PendingTransfers {
    /// Default location: `pending-transfers.json` in the app's state folder.
    pub fn default_path() -> PathBuf {
        config::state_dir().join("pending-transfers.json")
    }

    /// Read the list from `path`; a missing or unreadable file gives an empty list
    /// (logged), since losing it only loses the offer to resume.
    pub fn load(path: &Path) -> Self {
        config::load_json(path).unwrap_or_else(|error| {
            tracing::warn!("{error}; forgetting interrupted transfers");
            Self::default()
        })
    }

    /// Write the list to `path`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Io`] if the file cannot be written
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        config::save_json(self, path)
    }

    /// Record a job, replacing any entry with the same key.
    pub fn add(&mut self, pending: PendingJob) {
        self.remove(&pending.key);
        self.jobs.push(pending);
    }

    /// Forget a job.
    ///
    /// # Returns
    ///
    /// Whether it was recorded
    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.jobs.len();
        self.jobs.retain(|pending| pending.key != key);
        self.jobs.len() != before
    }

    /// The unfinished jobs of a site, oldest first.
    pub fn for_site<'a>(&'a self, site_id: &'a str) -> impl Iterator<Item = &'a PendingJob> {
        self.jobs
            .iter()
            .filter(move |pending| pending.site_id == site_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::{RemotePath, SidePath};
    use crate::transfer::{JobKind, Side};

    fn pending(key: &str, site: &str) -> PendingJob {
        PendingJob {
            key: key.to_owned(),
            site_id: site.to_owned(),
            job: Job {
                kind: JobKind::Copy,
                side: Side::Local,
                sources: vec![SidePath::Local(PathBuf::from("/data/a"))],
                dest_dir: SidePath::Remote(RemotePath::new("/srv")),
                same_side: false,
                between_servers: false,
                resume: false,
            },
        }
    }

    #[test]
    fn add_replaces_by_key_and_for_site_filters() {
        let mut list = PendingTransfers::default();
        list.add(pending("k1", "site-a"));
        list.add(pending("k2", "site-b"));
        list.add(pending("k1", "site-a"));

        assert_eq!(list.for_site("site-a").count(), 1);
        assert!(list.remove("k1"));
        assert!(!list.remove("k1"));
        assert_eq!(list.for_site("site-a").count(), 0);
    }

    #[test]
    fn save_then_load_round_trips_and_garbage_loads_empty() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("state/pending.json");
        let mut list = PendingTransfers::default();
        list.add(pending("k1", "site-a"));

        list.save(&path).expect("save");
        assert_eq!(PendingTransfers::load(&path), list);

        std::fs::write(&path, "not json").expect("write");
        assert_eq!(PendingTransfers::load(&path), PendingTransfers::default());
    }
}
