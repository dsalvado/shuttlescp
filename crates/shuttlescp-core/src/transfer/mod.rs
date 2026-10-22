//! Copying, moving and deleting files and folders between this computer and a server,
//! reporting progress as it goes.
//!
//! A [`Job`] is first *planned*: its sources are walked into a tree of [`PlanItem`]s
//! (folders and the files inside them, with sizes), noting which files already exist at
//! the destination. Front ends show the tree. Before anything is copied, each existing
//! file is settled by asking the front end ([`TransferEvent::Conflict`]): overwrite, skip
//! or cancel. The job then runs: folders are created first, then up to [`FILES_AT_ONCE`]
//! files are copied at the same time, each reporting [`TransferEvent::Progress`].
//!
//! Files are written under a temporary name (ending in [`PARTIAL_SUFFIX`]) and renamed
//! when complete, so a destination file is never half-written. If a copy fails or the
//! app is closed, the partial file stays, and the next attempt continues from where it
//! stopped (after checking that its end still matches the source). A move deletes each
//! source file only after it was copied, and a source folder only once it is empty.
//!
//! Every job runs as its own async task, so several jobs proceed side by side. All jobs to
//! one server share its single SSH connection.

mod copy;
mod endpoint;
mod meter;
mod ops;
mod plan;

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

pub use meter::{RateMeter, fraction, percent};
pub use ops::{
    check_name, check_same_side, create_folder, rename, windows_name_problem, windows_safe_name,
};

use crate::format::format_size;
use crate::path::{RemotePath, SidePath};
use crate::remote::Remote;
use endpoint::Endpoint;

/// Files of one job copied at the same time. More would not add speed (they share one
/// network link and disk) but would make each file slower to finish.
pub const FILES_AT_ONCE: usize = 4;

/// Ending added to a file's name while it is being copied; the file gets its real name
/// once complete. The same convention as WinSCP.
pub const PARTIAL_SUFFIX: &str = ".filepart";

/// Where files are: this computer or the connected server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// This computer.
    Local,
    /// The connected server.
    Remote,
}

impl Side {
    /// The other side: where copies go.
    pub fn other(self) -> Self {
        match self {
            Self::Local => Self::Remote,
            Self::Remote => Self::Local,
        }
    }
}

/// What a job does with its sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Copy to the other side (or, for a [`Job::same_side`] job, to another folder on
    /// the same side), keeping the originals.
    Copy,
    /// Copy like [`JobKind::Copy`], then delete the originals.
    Move,
    /// Delete the sources.
    Delete,
}

/// One user request: copy, move or delete some files and folders.
///
/// Saved (see [`crate::pending`]) as a [`JobRecord`], in the same form as before paths
/// knew their side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "JobRecord", into = "JobRecord")]
pub struct Job {
    /// What to do.
    pub kind: JobKind,
    /// Where the sources are.
    pub side: Side,
    /// Files and folders to act on (absolute paths on `side`).
    pub sources: Vec<SidePath>,
    /// Folder that copies go into, on the other side (or on the same side for a
    /// [`Job::same_side`] job); unused when deleting.
    pub dest_dir: SidePath,
    /// Copies go to another folder on the same side (e.g. a file dragged onto a folder
    /// in its own pane) instead of to the other side.
    pub same_side: bool,
    /// Continue an earlier, interrupted run of this job: files already copied (same
    /// size and date at the destination) are not copied again or asked about, and
    /// sources already moved away are passed over.
    pub resume: bool,
}

impl Job {
    /// Short description, e.g. "Upload report.pdf to /home/me" or "Delete 3 items from
    /// the server".
    pub fn title(&self) -> String {
        let what = match self.sources.as_slice() {
            [single] => single.file_name().map_or_else(
                || single.to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
            many => format!("{} items", many.len()),
        };
        let dest = &self.dest_dir;
        let on_server = match self.side {
            Side::Local => "",
            Side::Remote => " on the server",
        };
        match (self.kind, self.side) {
            (JobKind::Copy, _) if self.same_side => format!("Copy {what} to {dest}{on_server}"),
            (JobKind::Move, _) if self.same_side => format!("Move {what} to {dest}{on_server}"),
            (JobKind::Copy, Side::Local) => format!("Upload {what} to {dest}"),
            (JobKind::Copy, Side::Remote) => format!("Download {what} to {dest}"),
            (JobKind::Move, Side::Local) => format!("Move {what} to the server, {dest}"),
            (JobKind::Move, Side::Remote) => format!("Move {what} to this computer, {dest}"),
            (JobKind::Delete, Side::Local) => format!("Delete {what} from this computer"),
            (JobKind::Delete, Side::Remote) => format!("Delete {what} from the server"),
        }
    }

    /// Whether the job needs the server connection.
    pub fn uses_server(&self) -> bool {
        let stays_local = self.kind == JobKind::Delete || self.same_side;
        !(stays_local && self.side == Side::Local)
    }

    /// The files and folders the job is changing: what copies and moves write at the
    /// destination, or what a delete removes. Everything inside a folder
    /// listed here is in use too, and so is a file's partial copy (its name plus
    /// [`PARTIAL_SUFFIX`]).
    pub fn paths_in_use(&self) -> Vec<SidePath> {
        if self.kind == JobKind::Delete {
            return self.sources.clone();
        }
        self.sources
            .iter()
            .filter_map(SidePath::file_name)
            .map(|name| self.dest_dir.join(ops::dest_name(name, &self.dest_dir)))
            .collect()
    }

    /// Where copies go: the other side, or the same side for a [`Job::same_side`] job.
    pub fn dest_side(&self) -> Side {
        if self.same_side {
            self.side
        } else {
            self.side.other()
        }
    }
}

/// A [`Job`] as saved: its paths as plain text, read back as local or server paths by
/// the side they are on.
#[derive(Serialize, Deserialize)]
struct JobRecord {
    kind: JobKind,
    side: Side,
    sources: Vec<PathBuf>,
    dest_dir: PathBuf,
    #[serde(default)]
    same_side: bool,
    #[serde(default)]
    resume: bool,
}

impl JobRecord {
    /// A saved path as a path on `side`.
    fn path_on(side: Side, path: PathBuf) -> SidePath {
        match side {
            Side::Local => SidePath::Local(path),
            Side::Remote => SidePath::Remote(RemotePath::new(path.to_string_lossy())),
        }
    }

    /// A path as saved.
    fn saved(path: SidePath) -> PathBuf {
        match path {
            SidePath::Local(path) => path,
            SidePath::Remote(path) => PathBuf::from(String::from(path)),
        }
    }
}

impl From<JobRecord> for Job {
    fn from(record: JobRecord) -> Self {
        let mut job = Self {
            kind: record.kind,
            side: record.side,
            sources: Vec::new(),
            dest_dir: SidePath::Local(PathBuf::new()),
            same_side: record.same_side,
            resume: record.resume,
        };
        job.sources = record
            .sources
            .into_iter()
            .map(|source| JobRecord::path_on(job.side, source))
            .collect();
        job.dest_dir = JobRecord::path_on(job.dest_side(), record.dest_dir);
        job
    }
}

impl From<Job> for JobRecord {
    fn from(job: Job) -> Self {
        Self {
            kind: job.kind,
            side: job.side,
            sources: job.sources.into_iter().map(JobRecord::saved).collect(),
            dest_dir: JobRecord::saved(job.dest_dir),
            same_side: job.same_side,
            resume: job.resume,
        }
    }
}

/// Whether a planned item is a folder or a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// A folder; its contents are separate items.
    Folder,
    /// A file (or, when deleting, a symbolic link).
    File,
}

/// Size and date of a file, e.g. to compare two versions of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileInfo {
    /// Bytes.
    pub size: u64,
    /// Last modification, if known.
    pub modified: Option<SystemTime>,
}

impl FileInfo {
    /// Whether two files look identical: same size and the same modification time to
    /// the second (SFTP stores whole seconds).
    pub fn same_as(&self, other: &Self) -> bool {
        let seconds = |time: Option<SystemTime>| {
            time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|since| since.as_secs())
        };
        self.size == other.size
            && seconds(self.modified).is_some()
            && seconds(self.modified) == seconds(other.modified)
    }
}

/// One file or folder of a planned job. Items form a tree through `parent`; a parent
/// always comes before its children.
#[derive(Debug, Clone)]
pub struct PlanItem {
    /// Position in the plan; events refer to items by it.
    pub id: usize,
    /// The folder item this one is inside, `None` for the job's sources.
    pub parent: Option<usize>,
    /// File or folder name.
    pub name: String,
    /// Folder or file.
    pub kind: ItemKind,
    /// Bytes: the file's size, or for a folder the total of the files inside it.
    pub size: u64,
    /// Where it is now.
    pub source: SidePath,
    /// Where it is copied to (unused when deleting).
    pub dest: SidePath,
    /// A file already at `dest`, which copying would replace.
    pub existing: Option<FileInfo>,
    /// The source's modification time, given to the copy.
    modified: Option<SystemTime>,
    /// A folder (not a file) is already at `dest`.
    existing_is_dir: bool,
    /// Bytes of an earlier, interrupted copy found at `dest` + [`PARTIAL_SUFFIX`].
    partial: u64,
}

impl PlanItem {
    /// Size and date of the source, for comparing with an existing copy.
    pub fn info(&self) -> FileInfo {
        FileInfo {
            size: self.size,
            modified: self.modified,
        }
    }
}

/// How an item ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemOutcome {
    /// Copied, moved or deleted.
    Done,
    /// Left alone because the user chose not to overwrite what is there.
    Skipped,
    /// Something went wrong; the message says what.
    Failed(String),
    /// The job was cancelled first.
    Cancelled,
}

/// The totals of a finished job.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobSummary {
    /// Items done.
    pub done: usize,
    /// Items skipped (not overwritten).
    pub skipped: usize,
    /// Items that failed.
    pub failed: usize,
    /// Items not done because the job was cancelled.
    pub cancelled: usize,
    /// Bytes of the files copied (0 for deletes).
    pub bytes: u64,
    /// Why the job could not start at all (e.g. a source could not be read).
    pub error: Option<String>,
}

impl JobSummary {
    /// Whether everything the job set out to do was done or deliberately skipped, so
    /// there is nothing left to resume.
    pub fn is_complete(&self) -> bool {
        self.failed == 0 && self.cancelled == 0 && self.error.is_none()
    }
}

/// What to do about a file that already exists at the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    /// Replace it.
    Overwrite,
    /// Leave it, and do not copy this file.
    Skip,
    /// Stop the whole job.
    CancelJob,
}

/// The user's answer to a [`TransferEvent::Conflict`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConflictAnswer {
    /// What to do.
    pub choice: ConflictChoice,
    /// Do the same for the job's remaining conflicts without asking.
    pub apply_to_all: bool,
}

/// Where the answer to a [`TransferEvent::Conflict`] goes. The job waits until it is
/// answered (or dropped unanswered, which cancels the job).
#[derive(Clone)]
pub struct ConflictReply(Arc<Mutex<Option<oneshot::Sender<ConflictAnswer>>>>);

impl ConflictReply {
    fn new(sender: oneshot::Sender<ConflictAnswer>) -> Self {
        Self(Arc::new(Mutex::new(Some(sender))))
    }

    /// Answer the question; only the first answer counts.
    pub fn answer(&self, answer: ConflictAnswer) {
        if let Some(sender) = self.0.lock().ok().and_then(|mut slot| slot.take()) {
            // Fails only if the job is gone, when there is no one left to tell.
            let _ = sender.send(answer);
        }
    }
}

// Written by hand: the channel inside has nothing useful to show.
impl fmt::Debug for ConflictReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConflictReply")
    }
}

/// What a running job reports, in order: `Planned`, then any number of `Conflict`,
/// `Resumed`, `Progress` and `ItemFinished`, then `JobFinished` last.
#[derive(Debug, Clone)]
pub enum TransferEvent {
    /// The job's items, parents before children.
    Planned(Vec<PlanItem>),
    /// A file already exists at the destination: what should happen to it? Answer
    /// through `reply`; the job waits meanwhile (or stops if cancelled).
    Conflict {
        /// Item id.
        id: usize,
        /// The file there now.
        existing: FileInfo,
        /// The file that would replace it.
        incoming: FileInfo,
        /// Where the answer goes.
        reply: ConflictReply,
    },
    /// A file continues an earlier, interrupted copy: its first `from` bytes were
    /// copied before (they count as done, but not towards the current speed).
    Resumed {
        /// Item id.
        id: usize,
        /// Bytes already copied.
        from: u64,
    },
    /// A file has `done` of its bytes copied.
    Progress {
        /// Item id.
        id: usize,
        /// Bytes copied so far.
        done: u64,
    },
    /// An item finished.
    ItemFinished {
        /// Item id.
        id: usize,
        /// How it ended.
        outcome: ItemOutcome,
    },
    /// The whole job finished.
    JobFinished(JobSummary),
}

/// Asks a running job to stop. Clones share the request.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    /// Ask the job to stop: files being copied stop at their next chunk and their
    /// partial copies are deleted; items not started are skipped.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancelling was asked for.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Receives a job's events; called from background tasks.
type Sink = Arc<dyn Fn(TransferEvent) + Send + Sync>;

/// Run `job`, reporting to `on_event` as it goes.
///
/// # Arguments
///
/// * `job` - What to do
/// * `remote` - The server connection; needed unless the job only deletes local files
/// * `cancel` - Lets the caller stop the job
/// * `on_event` - Receives progress; called from background tasks, so it must be cheap
///   and thread-safe (e.g. send to a channel)
///
/// # Returns
///
/// The job's totals (also sent as [`TransferEvent::JobFinished`])
pub async fn run(
    job: Job,
    remote: Option<Remote>,
    cancel: CancelFlag,
    on_event: impl Fn(TransferEvent) + Send + Sync + 'static,
) -> JobSummary {
    let sink: Sink = Arc::new(on_event);
    let started = std::time::Instant::now();
    let endpoint = |side: Side| match side {
        Side::Local => Some(Endpoint::Local),
        Side::Remote => remote.clone().map(Endpoint::Remote),
    };
    let summary = match endpoint(job.side) {
        None => failed_to_start("not connected to a server"),
        Some(source) => {
            let dest = endpoint(job.dest_side());
            execute(&job, source, dest, &cancel, &sink).await
        }
    };
    let seconds = started.elapsed().as_secs_f64();
    let amount = if job.kind == JobKind::Delete || summary.bytes == 0 {
        String::new()
    } else {
        // Rounded to whole bytes for display.
        let speed = (summary.bytes as f64 / seconds.max(0.001)) as u64;
        format!(
            "; {} at {}/s",
            format_size(summary.bytes),
            format_size(speed)
        )
    };
    tracing::info!(
        "{}: {} done, {} skipped, {} failed, {} cancelled in {seconds:.1} s{amount}{}",
        job.title(),
        summary.done,
        summary.skipped,
        summary.failed,
        summary.cancelled,
        summary
            .error
            .as_ref()
            .map_or_else(String::new, |error| format!(" ({error})"))
    );
    sink(TransferEvent::JobFinished(summary.clone()));
    summary
}

/// A summary for a job that could not start.
fn failed_to_start(error: &str) -> JobSummary {
    JobSummary {
        error: Some(error.to_owned()),
        ..JobSummary::default()
    }
}

/// Plan and carry out a job between two endpoints.
///
/// # Arguments
///
/// * `job` - What to do
/// * `src` - Where the sources are
/// * `dst` - Where copies go; `None` when there is no connection (fine for deleting)
/// * `cancel` - Stop request
/// * `sink` - Receives events
async fn execute(
    job: &Job,
    src: Endpoint,
    dst: Option<Endpoint>,
    cancel: &CancelFlag,
    sink: &Sink,
) -> JobSummary {
    tracing::info!(
        "{}{}",
        job.title(),
        if job.resume { " (resuming)" } else { "" }
    );
    let items = match plan::plan(job, &src, dst.as_ref()).await {
        Ok(items) => items,
        Err(error) => {
            tracing::warn!("cannot start {}: {error}", job.title());
            return failed_to_start(&error);
        }
    };
    sink(TransferEvent::Planned(items.clone()));
    let outcomes = match (job.kind, dst) {
        (JobKind::Delete, _) => copy::delete_items(&items, &src, cancel, sink).await,
        (_, Some(dst)) => copy::copy_items(job, &items, &src, &dst, cancel, sink).await,
        (_, None) => return failed_to_start("not connected to a server"),
    };
    let mut summary = JobSummary::default();
    if job.kind != JobKind::Delete {
        summary.bytes = items
            .iter()
            .filter(|item| item.kind == ItemKind::File && outcomes[item.id] == ItemOutcome::Done)
            .map(|item| item.size)
            .sum();
    }
    for outcome in outcomes {
        match outcome {
            ItemOutcome::Done => summary.done += 1,
            ItemOutcome::Skipped => summary.skipped += 1,
            ItemOutcome::Failed(_) => summary.failed += 1,
            ItemOutcome::Cancelled => summary.cancelled += 1,
        }
    }
    summary
}

#[cfg(test)]
mod tests;
