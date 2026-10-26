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

use std::collections::HashSet;
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

/// Files of one job copied at the same time on this computer alone. More would not add
/// speed (they share one disk) but would make each file slower to finish. A job using a
/// server copies as many as its site is set to (see [`Site::files_at_once`]).
///
/// [`Site::files_at_once`]: crate::sites::Site::files_at_once
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
    /// Site to site: the sources are on one server and copies go to another server
    /// (`side` is [`Side::Remote`] and so is `dest_dir`). The data passes through this
    /// computer, as with `scp -3`; [`run`] gets both connections.
    pub between_servers: bool,
    /// In a site-to-site session (two servers, one per pane), the server the job's
    /// sources are on is the left pane's; otherwise the right pane's, the only server of
    /// an ordinary session.
    pub left_server: bool,
    /// Continue an earlier, interrupted run of this job: files already copied (same
    /// size and date at the destination) are not copied again or asked about, and
    /// sources already moved away are passed over.
    pub resume: bool,
}

impl Job {
    /// Short description, e.g. "Upload report.pdf to /home/me" or "Delete 3 items from
    /// the server".
    pub fn title(&self) -> String {
        let what = self.subject();
        let dest = &self.dest_dir;
        let on_server = match self.side {
            Side::Local => "",
            Side::Remote => " on the server",
        };
        match (self.kind, self.side) {
            (JobKind::Copy, _) if self.between_servers => {
                format!("Copy {what} to {dest} on the other server")
            }
            (JobKind::Move, _) if self.between_servers => {
                format!("Move {what} to {dest} on the other server")
            }
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

    /// The pane the sources are in (left is [`Side::Local`], right [`Side::Remote`]). In
    /// an ordinary session that is `side`; site to site, both panes are servers and
    /// [`Job::left_server`] says which.
    pub fn source_pane(&self) -> Side {
        if self.left_server {
            Side::Local
        } else {
            self.side
        }
    }

    /// The pane copies go to (left is [`Side::Local`], right [`Side::Remote`]): the
    /// other pane, or the same one for a [`Job::same_side`] job or a delete.
    pub fn dest_pane(&self) -> Side {
        if self.kind == JobKind::Delete || self.same_side {
            self.source_pane()
        } else {
            self.source_pane().other()
        }
    }

    /// The folders a pane showed when the job was made, as far as the job tells: the one
    /// holding its sources and, if copies go to the other pane, the one they go to. For
    /// reopening an interrupted job's session where it was.
    ///
    /// # Returns
    ///
    /// Each pane (left is [`Side::Local`], right [`Side::Remote`]) with its folder
    pub fn pane_folders(&self) -> Vec<(Side, SidePath)> {
        let mut folders: Vec<(Side, SidePath)> = self
            .sources
            .first()
            .and_then(SidePath::parent)
            .map(|dir| (self.source_pane(), dir))
            .into_iter()
            .collect();
        if self.dest_pane() != self.source_pane() {
            folders.push((self.dest_pane(), self.dest_dir.clone()));
        }
        folders
    }

    /// Pick the job's connections from a session's: the server its sources are on and,
    /// site to site, the server copies go to (see [`run`]).
    ///
    /// # Arguments
    ///
    /// * `left` - The left pane's server (site to site only)
    /// * `right` - The right pane's server
    pub fn connections<T: Clone>(&self, left: Option<&T>, right: &T) -> (Option<T>, Option<T>) {
        let (from, to) = if self.left_server {
            (left.cloned(), Some(right.clone()))
        } else {
            (Some(right.clone()), left.cloned())
        };
        (from, to.filter(|_| self.between_servers))
    }

    /// The pane whose files the job changes: where copies go, or what a delete removes
    /// (left is [`Side::Local`], right [`Side::Remote`]).
    pub fn changed_pane(&self) -> Side {
        if self.kind == JobKind::Delete {
            self.source_pane()
        } else {
            self.dest_pane()
        }
    }

    /// Whether the two jobs (of one session) would change the same files: in the same
    /// pane, a path one changes is a path the other changes, or inside it, or holds it.
    /// Two such jobs must not run at once: they would write the same partial files.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self.changed_pane() == other.changed_pane()
            && self.paths_in_use().iter().any(|mine| {
                other
                    .paths_in_use()
                    .iter()
                    .any(|theirs| mine.starts_with(theirs) || theirs.starts_with(mine))
            })
    }

    /// What the job acts on, for titles: the name of its one source, or "3 items".
    pub fn subject(&self) -> String {
        match self.sources.as_slice() {
            [single] => single.file_name().map_or_else(
                || single.to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
            many => format!("{} items", many.len()),
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
    /// Site to site, both are servers ([`Side::Remote`]).
    pub fn dest_side(&self) -> Side {
        if self.same_side || self.between_servers {
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
    between_servers: bool,
    #[serde(default)]
    left_server: bool,
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
            between_servers: record.between_servers,
            left_server: record.left_server,
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
            between_servers: job.between_servers,
            left_server: job.left_server,
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
    /// The user stopped this item on its own; the rest of the job went on. A folder is
    /// stopped when items inside it were (and nothing in it failed).
    Stopped,
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
    /// Items the user stopped on their own (the job is still complete without them).
    pub stopped: usize,
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

/// Asks a running job to stop (or pause), or to leave out some of its items. Clones
/// share the requests.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag {
    /// The whole job should stop.
    cancelled: Arc<AtomicBool>,
    /// ...and keep its partial copies, to be continued later.
    paused: Arc<AtomicBool>,
    /// The items to leave out, by id.
    stopped: Arc<Mutex<HashSet<usize>>>,
    /// The files paused on their own, by id.
    paused_items: Arc<Mutex<HashSet<usize>>>,
}

impl CancelFlag {
    /// Ask the job to pause: it stops like [`CancelFlag::cancel`], but files being
    /// copied keep their partial copies, so running the job again with `resume`
    /// continues them where they stopped.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Whether pausing was asked for.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Ask the job to leave out one item (by id): a file being copied stops at its next
    /// chunk and its partial copy is deleted; a file not started is not copied. The
    /// rest of the job goes on. For a folder, stop each item inside it.
    pub fn stop_item(&self, id: usize) {
        if let Ok(mut stopped) = self.stopped.lock() {
            stopped.insert(id);
        }
    }

    /// Pause one file (by id) while the rest of the job goes on: it stops at its next
    /// chunk, keeps its partial copy and gives up its place to the next file, then
    /// waits until [`CancelFlag::resume_item`]. The job does not end while a file
    /// waits. For a folder, pause each file inside it.
    pub fn pause_item(&self, id: usize) {
        if let Ok(mut paused) = self.paused_items.lock() {
            paused.insert(id);
        }
    }

    /// Let a paused file continue where it stopped.
    pub fn resume_item(&self, id: usize) {
        if let Ok(mut paused) = self.paused_items.lock() {
            paused.remove(&id);
        }
    }

    /// Whether this file is paused.
    pub fn is_item_paused(&self, id: usize) -> bool {
        self.paused_items
            .lock()
            .is_ok_and(|paused| paused.contains(&id))
    }

    /// Whether the user asked to leave out this item.
    pub fn is_item_stopped(&self, id: usize) -> bool {
        self.stopped
            .lock()
            .is_ok_and(|stopped| stopped.contains(&id))
    }

    /// Ask the job to stop: files being copied stop at their next chunk and their
    /// partial copies are deleted; items not started are skipped.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Whether cancelling (or pausing) was asked for.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// Receives a job's events; called from background tasks.
type Sink = Arc<dyn Fn(TransferEvent) + Send + Sync>;

/// Run `job`, reporting to `on_event` as it goes.
///
/// # Arguments
///
/// * `job` - What to do
/// * `remote` - The server connection; needed unless the job only deletes local files.
///   Site to site, the connection to the server the sources are on
/// * `dest_remote` - Site to site ([`Job::between_servers`]), the connection to the
///   server copies go to; otherwise unused
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
    dest_remote: Option<Remote>,
    cancel: CancelFlag,
    on_event: impl Fn(TransferEvent) + Send + Sync + 'static,
) -> JobSummary {
    let sink: Sink = Arc::new(on_event);
    let started = std::time::Instant::now();
    let endpoint = |side: Side| match side {
        Side::Local => Some(Endpoint::Local),
        Side::Remote => remote.clone().map(Endpoint::Remote),
    };
    let dest = if job.between_servers {
        dest_remote.map(Endpoint::Remote)
    } else {
        endpoint(job.dest_side())
    };
    let summary = match endpoint(job.side) {
        None => failed_to_start("not connected to a server"),
        Some(_) if job.between_servers && dest.is_none() => {
            failed_to_start("not connected to the server copies go to")
        }
        Some(source) => execute(&job, source, dest, &cancel, &sink).await,
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

/// Delete the partial files (see [`PARTIAL_SUFFIX`]) an unfinished job left at its
/// destination, when the user gives it up rather than resuming it later. The job is
/// planned again to find them; sources it already moved away are passed over.
///
/// # Arguments
///
/// * `job` - The unfinished job (stopped: a running one would write them again)
/// * `remote`, `dest_remote` - Its connections, as for [`run`]
///
/// # Returns
///
/// How many partial files were deleted
///
/// # Errors
///
/// Returns a message if the job cannot be planned (e.g. not connected)
pub async fn discard_partials(
    mut job: Job,
    remote: Option<Remote>,
    dest_remote: Option<Remote>,
) -> Result<usize, String> {
    if job.kind == JobKind::Delete {
        return Ok(0);
    }
    let endpoint = |side: Side| match side {
        Side::Local => Some(Endpoint::Local),
        Side::Remote => remote.clone().map(Endpoint::Remote),
    };
    let dest = if job.between_servers {
        dest_remote.map(Endpoint::Remote)
    } else {
        endpoint(job.dest_side())
    };
    let (Some(src), Some(dst)) = (endpoint(job.side), dest) else {
        return Err(String::from("not connected to a server"));
    };
    // As when resuming: sources already moved are not an error.
    job.resume = true;
    let items = plan::plan(&job, &src, Some(&dst)).await?;
    let mut removed = 0;
    for item in items.iter().filter(|item| item.partial > 0) {
        let partial = endpoint::partial_path(&item.dest);
        match dst.remove_file(&partial).await {
            Ok(()) => {
                tracing::info!("deleted the partial copy {partial}");
                removed += 1;
            }
            Err(error) => tracing::warn!("{error}"),
        }
    }
    Ok(removed)
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
            ItemOutcome::Stopped => summary.stopped += 1,
        }
    }
    summary
}

#[cfg(test)]
mod tests;
