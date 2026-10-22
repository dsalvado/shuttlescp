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
    /// The site it transfers to or from (site to site, the right pane's site).
    pub site_id: String,
    /// Site to site, the left pane's site: the job needs both connected again, in the
    /// same panes. Older files have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_site_id: Option<String>,
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

    /// The unfinished jobs of a site, oldest first (not those it shared with another
    /// site, site to site).
    pub fn for_site<'a>(&'a self, site_id: &'a str) -> impl Iterator<Item = &'a PendingJob> {
        self.for_sites(None, site_id)
    }

    /// The unfinished jobs of a site, or (with `left_site_id`) of a site-to-site pair in
    /// those panes, oldest first.
    pub fn for_sites<'a>(
        &'a self,
        left_site_id: Option<&'a str>,
        site_id: &'a str,
    ) -> impl Iterator<Item = &'a PendingJob> {
        self.jobs.iter().filter(move |pending| {
            pending.site_id == site_id && pending.left_site_id.as_deref() == left_site_id
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::{RemotePath, SidePath};
    use crate::transfer::{JobKind, Side};

    fn pending_job(key: &str, site: &str) -> PendingJob {
        PendingJob {
            key: key.to_owned(),
            site_id: site.to_owned(),
            left_site_id: None,
            job: Job {
                kind: JobKind::Copy,
                side: Side::Local,
                sources: vec![SidePath::Local(PathBuf::from("/data/a"))],
                dest_dir: SidePath::Remote(RemotePath::new("/srv")),
                same_side: false,
                between_servers: false,
                left_server: false,
                resume: false,
            },
        }
    }

    #[test]
    fn add_replaces_by_key_and_for_site_filters() {
        let mut list = PendingTransfers::default();
        list.add(pending_job("k1", "site-a"));
        list.add(pending_job("k2", "site-b"));
        list.add(pending_job("k1", "site-a"));

        assert_eq!(list.for_site("site-a").count(), 1);
        assert!(list.remove("k1"));
        assert!(!list.remove("k1"));
        assert_eq!(list.for_site("site-a").count(), 0);
    }

    #[test]
    fn site_to_site_records_belong_to_their_pair_only() {
        let mut pending = PendingTransfers::default();
        pending.add(pending_job("one", "right"));
        pending.add(PendingJob {
            left_site_id: Some(String::from("left")),
            ..pending_job("pair", "right")
        });

        let keys =
            |jobs: Vec<&PendingJob>| jobs.iter().map(|job| job.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(pending.for_site("right").collect()), ["one"]);
        assert_eq!(
            keys(pending.for_sites(Some("left"), "right").collect()),
            ["pair"]
        );
        assert!(
            pending.for_sites(Some("right"), "left").next().is_none(),
            "panes swapped"
        );

        // Saved with the left site, read back the same; older records have none.
        let json = serde_json::to_string(&pending).expect("save");
        assert_eq!(
            serde_json::from_str::<PendingTransfers>(&json).expect("load"),
            pending
        );
        assert!(!json.contains(r#""left_site_id":null"#));
    }

    #[test]
    fn save_then_load_round_trips_and_garbage_loads_empty() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("state/pending.json");
        let mut list = PendingTransfers::default();
        list.add(pending_job("k1", "site-a"));

        list.save(&path).expect("save");
        assert_eq!(PendingTransfers::load(&path), list);

        std::fs::write(&path, "not json").expect("write");
        assert_eq!(PendingTransfers::load(&path), PendingTransfers::default());
    }
}
