//! Copying, moving and deleting files and folders between this computer and a server,
//! reporting progress as it goes.
//!
//! A [`Job`] is first *planned*: its sources are walked into a tree of [`PlanItem`]s
//! (folders and the files inside them, with sizes), which front ends show. The job then
//! runs: folders are created first, then up to [`FILES_AT_ONCE`] files are copied at the
//! same time, each reporting [`TransferEvent::Progress`]. A move deletes each source file
//! only after it was copied, and a source folder only once everything in it was moved.
//!
//! Every job runs as its own async task, so several jobs proceed side by side. All jobs to
//! one server share its single SSH connection.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::format::{format_duration, format_size};
use crate::remote::Remote;

/// Files of one job copied at the same time. More would not add speed (they share one
/// network link and disk) but would make each file slower to finish.
pub const FILES_AT_ONCE: usize = 4;
/// Bytes read and written per step of a file copy.
const CHUNK_BYTES: usize = 256 * 1024;
/// Shortest time between two progress reports for one file, so a fast copy does not
/// flood the user interface with updates.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// Deepest folder nesting walked; deeper usually means a symbolic link loop.
const MAX_DEPTH: usize = 64;
/// Speed is measured over at least this long, so one burst does not swing the estimate.
const MIN_SAMPLE: Duration = Duration::from_millis(250);
/// How much each new speed sample moves the smoothed speed (0 = never, 1 = jump to it).
const SMOOTHING: f64 = 0.3;
/// A time-left estimate is shown once a transfer has run this long...
const ESTIMATE_AFTER: Duration = Duration::from_secs(2);
/// ...and only if it looks like taking at least this long in total.
const ESTIMATE_MIN_TOTAL: Duration = Duration::from_secs(10);

/// Where files are: this computer or the connected server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    /// Copy to the other side, keeping the originals.
    Copy,
    /// Copy to the other side, then delete the originals.
    Move,
    /// Delete the sources.
    Delete,
}

/// One user request: copy, move or delete some files and folders.
#[derive(Debug, Clone)]
pub struct Job {
    /// What to do.
    pub kind: JobKind,
    /// Where the sources are.
    pub side: Side,
    /// Files and folders to act on (absolute paths on `side`).
    pub sources: Vec<PathBuf>,
    /// Folder on the other side that copies go into; unused when deleting.
    pub dest_dir: PathBuf,
}

impl Job {
    /// Short description, e.g. "Upload report.pdf to /home/me" or "Delete 3 items from
    /// the server".
    pub fn title(&self) -> String {
        let what = match self.sources.as_slice() {
            [single] => single.file_name().map_or_else(
                || single.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
            many => format!("{} items", many.len()),
        };
        let dest = self.dest_dir.display();
        match (self.kind, self.side) {
            (JobKind::Copy, Side::Local) => format!("Upload {what} to {dest}"),
            (JobKind::Copy, Side::Remote) => format!("Download {what} to {dest}"),
            (JobKind::Move, Side::Local) => format!("Move {what} to the server, {dest}"),
            (JobKind::Move, Side::Remote) => format!("Move {what} to this computer, {dest}"),
            (JobKind::Delete, Side::Local) => format!("Delete {what} from this computer"),
            (JobKind::Delete, Side::Remote) => format!("Delete {what} from the server"),
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
    pub source: PathBuf,
    /// Where it is copied to (unused when deleting).
    pub dest: PathBuf,
    /// The source's modification time, given to the copy.
    modified: Option<SystemTime>,
}

/// How an item ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemOutcome {
    /// Copied, moved or deleted.
    Done,
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
    /// Items that failed.
    pub failed: usize,
    /// Items not done because the job was cancelled.
    pub cancelled: usize,
    /// Why the job could not start at all (e.g. a source could not be read).
    pub error: Option<String>,
}

/// What a running job reports, in order: `Planned`, then any number of `Progress` and
/// `ItemFinished`, then `JobFinished` last.
#[derive(Debug, Clone)]
pub enum TransferEvent {
    /// The job's items, parents before children.
    Planned(Vec<PlanItem>),
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
    let endpoint = |side: Side| match side {
        Side::Local => Some(Endpoint::Local),
        Side::Remote => remote.clone().map(Endpoint::Remote),
    };
    let summary = match endpoint(job.side) {
        None => JobSummary {
            error: Some(String::from("not connected to a server")),
            ..JobSummary::default()
        },
        Some(source) => {
            let dest = endpoint(job.side.other());
            execute(&job, source, dest, &cancel, &sink).await
        }
    };
    tracing::info!(
        "{}: {} done, {} failed, {} cancelled{}",
        job.title(),
        summary.done,
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
    tracing::info!("{}", job.title());
    let items = match plan(job, &src).await {
        Ok(items) => items,
        Err(error) => {
            tracing::warn!("cannot start {}: {error}", job.title());
            return JobSummary {
                error: Some(error),
                ..JobSummary::default()
            };
        }
    };
    sink(TransferEvent::Planned(items.clone()));
    let outcomes = match (job.kind, dst) {
        (JobKind::Delete, _) => delete_items(&items, &src, cancel, sink).await,
        (kind, Some(dst)) => {
            copy_items(&items, kind == JobKind::Move, &src, &dst, cancel, sink).await
        }
        (_, None) => {
            return JobSummary {
                error: Some(String::from("not connected to a server")),
                ..JobSummary::default()
            };
        }
    };
    let mut summary = JobSummary::default();
    for outcome in outcomes {
        match outcome {
            ItemOutcome::Done => summary.done += 1,
            ItemOutcome::Failed(_) => summary.failed += 1,
            ItemOutcome::Cancelled => summary.cancelled += 1,
        }
    }
    summary
}

/// Walk the job's sources into a tree of items.
///
/// Copies follow symbolic links (the copy gets what the link points to); deletes do not
/// (deleting a link removes the link, never what it points to).
///
/// # Errors
///
/// Returns a message if a source or one of its folders cannot be read
async fn plan(job: &Job, src: &Endpoint) -> Result<Vec<PlanItem>, String> {
    let follow = job.kind != JobKind::Delete;
    let mut items: Vec<PlanItem> = Vec::new();
    let mut folders = Vec::new();
    for source in &job.sources {
        let name = source
            .file_name()
            .ok_or_else(|| format!("{} cannot be copied or deleted", source.display()))?;
        let node = src.stat(source, follow).await?;
        let id = push_item(
            &mut items,
            None,
            name.to_os_string(),
            &node,
            source.clone(),
            &job.dest_dir,
        );
        if node.is_dir {
            folders.push((id, 0));
        }
    }
    while let Some((folder, depth)) = folders.pop() {
        if depth >= MAX_DEPTH {
            return Err(format!(
                "{} is nested too deeply (a symbolic link loop?)",
                items[folder].source.display()
            ));
        }
        let (source, dest) = (items[folder].source.clone(), items[folder].dest.clone());
        let mut children = src.children(&source, follow).await?;
        // Folders first, then by name, like the file panes.
        children
            .sort_by(|(a_name, a), (b_name, b)| b.is_dir.cmp(&a.is_dir).then(a_name.cmp(b_name)));
        for (name, node) in children {
            let child = push_item(
                &mut items,
                Some(folder),
                name.clone(),
                &node,
                source.join(&name),
                &dest,
            );
            if node.is_dir {
                folders.push((child, depth + 1));
            }
        }
    }
    // Children always come after their parent, so walking backwards adds each folder's
    // complete total into its parent.
    for id in (0..items.len()).rev() {
        if let Some(parent) = items[id].parent {
            items[parent].size += items[id].size;
        }
    }
    Ok(items)
}

/// Add an item to the plan.
///
/// # Returns
///
/// The new item's id
fn push_item(
    items: &mut Vec<PlanItem>,
    parent: Option<usize>,
    name: OsString,
    node: &Node,
    source: PathBuf,
    dest_dir: &Path,
) -> usize {
    let id = items.len();
    items.push(PlanItem {
        id,
        parent,
        name: name.to_string_lossy().into_owned(),
        kind: if node.is_dir {
            ItemKind::Folder
        } else {
            ItemKind::File
        },
        // A folder's total is added up once the whole tree is known.
        size: if node.is_dir { 0 } else { node.size },
        source,
        dest: dest_dir.join(&name),
        modified: node.modified,
    });
    id
}

/// The items directly inside each item, by id.
fn children_of(items: &[PlanItem]) -> Vec<Vec<usize>> {
    let mut children = vec![Vec::new(); items.len()];
    for item in items {
        if let Some(parent) = item.parent {
            children[parent].push(item.id);
        }
    }
    children
}

/// Copy (or move) every planned item.
///
/// # Returns
///
/// Each item's outcome, by id
async fn copy_items(
    items: &[PlanItem],
    delete_source: bool,
    src: &Endpoint,
    dst: &Endpoint,
    cancel: &CancelFlag,
    sink: &Sink,
) -> Vec<ItemOutcome> {
    let mut outcomes: Vec<Option<ItemOutcome>> = vec![None; items.len()];
    let failed_parent = |outcomes: &[Option<ItemOutcome>], item: &PlanItem| {
        item.parent
            .is_some_and(|parent| matches!(outcomes[parent], Some(ItemOutcome::Failed(_))))
    };

    // Folders first, parents before children, so every file has somewhere to go.
    for item in items.iter().filter(|item| item.kind == ItemKind::Folder) {
        if failed_parent(&outcomes, item) {
            outcomes[item.id] = Some(ItemOutcome::Failed(String::from(
                "the folder it is in could not be created",
            )));
        } else if let Err(error) = dst.create_dir(&item.dest).await {
            tracing::warn!("{error}");
            outcomes[item.id] = Some(ItemOutcome::Failed(error));
        }
    }

    // Then the files, several at a time; each runs as its own task.
    let slots = Arc::new(Semaphore::new(FILES_AT_ONCE));
    let mut running = JoinSet::new();
    for item in items.iter().filter(|item| item.kind == ItemKind::File) {
        let outcome = if failed_parent(&outcomes, item) {
            Some(ItemOutcome::Failed(String::from(
                "the folder it goes in could not be created",
            )))
        } else {
            None
        };
        if let Some(outcome) = outcome {
            sink(TransferEvent::ItemFinished {
                id: item.id,
                outcome: outcome.clone(),
            });
            outcomes[item.id] = Some(outcome);
            continue;
        }
        // Waits here while FILES_AT_ONCE files are already being copied. The semaphore
        // is never closed, so acquiring cannot fail.
        let Ok(slot) = Arc::clone(&slots).acquire_owned().await else {
            break;
        };
        collect_finished(&mut running, &mut outcomes, sink);
        if cancel.is_cancelled() {
            sink(TransferEvent::ItemFinished {
                id: item.id,
                outcome: ItemOutcome::Cancelled,
            });
            outcomes[item.id] = Some(ItemOutcome::Cancelled);
            continue;
        }
        let (item, src, dst, cancel, sink) = (
            item.clone(),
            src.clone(),
            dst.clone(),
            cancel.clone(),
            Arc::clone(sink),
        );
        running.spawn(async move {
            // Holding the slot until the copy ends lets the next file start.
            let _slot = slot;
            let outcome = copy_file(&item, &src, &dst, delete_source, &cancel, &sink).await;
            (item.id, outcome)
        });
    }
    while let Some(finished) = running.join_next().await {
        record_finished(finished, &mut outcomes, sink);
    }

    // Folders last, deepest first: each is done once everything inside it is.
    let children = children_of(items);
    for item in items
        .iter()
        .rev()
        .filter(|item| item.kind == ItemKind::Folder)
    {
        let outcome = match &outcomes[item.id] {
            Some(failed) => failed.clone(),
            None => {
                let inside = || children[item.id].iter().map(|&child| &outcomes[child]);
                if inside().all(|outcome| *outcome == Some(ItemOutcome::Done)) {
                    if delete_source {
                        // Only an empty folder can be removed, so anything added to it
                        // since the job was planned is never lost.
                        src.remove_dir(&item.source)
                            .await
                            .map_or_else(ItemOutcome::Failed, |()| ItemOutcome::Done)
                    } else {
                        ItemOutcome::Done
                    }
                } else if inside().any(|outcome| matches!(outcome, Some(ItemOutcome::Failed(_)))) {
                    ItemOutcome::Failed(String::from("some items inside it failed"))
                } else {
                    ItemOutcome::Cancelled
                }
            }
        };
        sink(TransferEvent::ItemFinished {
            id: item.id,
            outcome: outcome.clone(),
        });
        outcomes[item.id] = Some(outcome);
    }
    outcomes
        .into_iter()
        .map(|outcome| outcome.unwrap_or(ItemOutcome::Cancelled))
        .collect()
}

/// Record the file copies that have already finished, without waiting.
fn collect_finished(
    running: &mut JoinSet<(usize, ItemOutcome)>,
    outcomes: &mut [Option<ItemOutcome>],
    sink: &Sink,
) {
    while let Some(finished) = running.try_join_next() {
        record_finished(finished, outcomes, sink);
    }
}

/// Record one finished file copy and report it.
fn record_finished(
    finished: Result<(usize, ItemOutcome), tokio::task::JoinError>,
    outcomes: &mut [Option<ItemOutcome>],
    sink: &Sink,
) {
    match finished {
        Ok((id, outcome)) => {
            sink(TransferEvent::ItemFinished {
                id,
                outcome: outcome.clone(),
            });
            outcomes[id] = Some(outcome);
        }
        // A copy task panicked: its item stays without an outcome and counts as not done.
        Err(error) => tracing::error!("file copy task failed: {error}"),
    }
}

/// Copy one file, then delete the source if moving.
async fn copy_file(
    item: &PlanItem,
    src: &Endpoint,
    dst: &Endpoint,
    delete_source: bool,
    cancel: &CancelFlag,
    sink: &Sink,
) -> ItemOutcome {
    tracing::debug!(
        "copying {} to {}",
        item.source.display(),
        item.dest.display()
    );
    let mut created = false;
    match copy_contents(item, src, dst, cancel, sink, &mut created).await {
        Ok(()) => {}
        Err(outcome) => {
            // Never leave a half-written file behind. Only remove a file this copy
            // created: if creating it failed, the file there belongs to someone else.
            if created {
                let _ = dst.remove_file(&item.dest).await;
            }
            if let ItemOutcome::Failed(error) = &outcome {
                tracing::warn!("{error}");
            }
            return outcome;
        }
    }
    if let Some(modified) = item.modified
        && let Err(error) = dst.set_modified(&item.dest, modified).await
    {
        // The copy itself is fine; only its date differs.
        tracing::debug!("{error}");
    }
    if delete_source && let Err(error) = src.remove_file(&item.source).await {
        tracing::warn!("{error}");
        return ItemOutcome::Failed(format!("copied, but {error}"));
    }
    ItemOutcome::Done
}

/// Stream a file's bytes from `src` to `dst`, reporting progress.
///
/// # Arguments
///
/// * `created` - Set once the destination file has been created
///
/// # Errors
///
/// Returns the outcome to report when the copy did not complete
async fn copy_contents(
    item: &PlanItem,
    src: &Endpoint,
    dst: &Endpoint,
    cancel: &CancelFlag,
    sink: &Sink,
    created: &mut bool,
) -> Result<(), ItemOutcome> {
    let failed = ItemOutcome::Failed;
    let mut reader = src.open(&item.source).await.map_err(failed)?;
    let mut writer = dst.create(&item.dest).await.map_err(failed)?;
    *created = true;
    let mut buffer = vec![0; CHUNK_BYTES];
    let mut done: u64 = 0;
    let mut reported = Instant::now();
    loop {
        if cancel.is_cancelled() {
            return Err(ItemOutcome::Cancelled);
        }
        let read = reader.read(&mut buffer).await.map_err(|error| {
            ItemOutcome::Failed(format!("cannot read {}: {error}", item.source.display()))
        })?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read]).await.map_err(|error| {
            ItemOutcome::Failed(format!("cannot write {}: {error}", item.dest.display()))
        })?;
        done += read as u64;
        if reported.elapsed() >= PROGRESS_INTERVAL {
            sink(TransferEvent::Progress { id: item.id, done });
            reported = Instant::now();
        }
    }
    // Waits for the last writes to be acknowledged and closes the file.
    writer.shutdown().await.map_err(|error| {
        ItemOutcome::Failed(format!("cannot finish {}: {error}", item.dest.display()))
    })?;
    sink(TransferEvent::Progress { id: item.id, done });
    Ok(())
}

/// Delete every planned item: contents before the folders holding them.
///
/// # Returns
///
/// Each item's outcome, by id
async fn delete_items(
    items: &[PlanItem],
    src: &Endpoint,
    cancel: &CancelFlag,
    sink: &Sink,
) -> Vec<ItemOutcome> {
    let mut outcomes = vec![ItemOutcome::Cancelled; items.len()];
    let children = children_of(items);
    // Children always come after their parent, so going backwards empties each folder
    // before it is removed.
    for item in items.iter().rev() {
        let outcome = if cancel.is_cancelled() {
            ItemOutcome::Cancelled
        } else {
            match item.kind {
                ItemKind::File => src.remove_file(&item.source).await,
                ItemKind::Folder
                    if children[item.id]
                        .iter()
                        .all(|&child| outcomes[child] == ItemOutcome::Done) =>
                {
                    src.remove_dir(&item.source).await
                }
                ItemKind::Folder => Err(String::from("some items inside it were not deleted")),
            }
            .map_or_else(ItemOutcome::Failed, |()| ItemOutcome::Done)
        };
        if let ItemOutcome::Failed(error) = &outcome {
            tracing::warn!("{error}");
        }
        sink(TransferEvent::ItemFinished {
            id: item.id,
            outcome: outcome.clone(),
        });
        outcomes[item.id] = outcome;
    }
    outcomes
}

/// What a walk needs to know about a file or folder.
#[derive(Debug, Clone)]
struct Node {
    is_dir: bool,
    size: u64,
    modified: Option<SystemTime>,
}

/// File access on one side of a transfer.
#[derive(Clone)]
enum Endpoint {
    Local,
    Remote(Remote),
}

/// A file being read, local or remote.
type Reader = Pin<Box<dyn AsyncRead + Send>>;
/// A file being written, local or remote.
type Writer = Pin<Box<dyn AsyncWrite + Send>>;

impl Endpoint {
    /// Details of `path`; `follow` describes what a symbolic link points to instead of
    /// the link itself.
    async fn stat(&self, path: &Path, follow: bool) -> Result<Node, String> {
        match self {
            Self::Local => {
                let metadata = if follow {
                    tokio::fs::metadata(path).await
                } else {
                    tokio::fs::symlink_metadata(path).await
                }
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                Ok(Node {
                    is_dir: metadata.is_dir(),
                    size: metadata.len(),
                    modified: metadata.modified().ok(),
                })
            }
            Self::Remote(remote) => {
                let metadata = if follow {
                    remote.stat(path).await?
                } else {
                    remote.lstat(path).await?
                };
                Ok(sftp_node(&metadata))
            }
        }
    }

    /// The entries of a folder; see [`Endpoint::stat`] for `follow`. A link that cannot
    /// be followed (e.g. broken) is listed as itself.
    async fn children(&self, dir: &Path, follow: bool) -> Result<Vec<(OsString, Node)>, String> {
        let mut children = Vec::new();
        match self {
            Self::Local => {
                let unreadable = |error| format!("cannot open {}: {error}", dir.display());
                let mut entries = tokio::fs::read_dir(dir).await.map_err(unreadable)?;
                while let Some(entry) = entries.next_entry().await.map_err(unreadable)? {
                    let path = entry.path();
                    // `DirEntry::metadata` describes a symbolic link itself.
                    let mut node = self.stat(&path, false).await?;
                    if follow && entry.file_type().await.is_ok_and(|kind| kind.is_symlink()) {
                        node = self.stat(&path, true).await.unwrap_or(node);
                    }
                    children.push((entry.file_name(), node));
                }
            }
            Self::Remote(remote) => {
                for (name, metadata) in remote.read_dir_raw(dir).await? {
                    let mut node = sftp_node(&metadata);
                    if follow && metadata.is_symlink() {
                        node = self.stat(&dir.join(&name), true).await.unwrap_or(node);
                    }
                    children.push((OsString::from(name), node));
                }
            }
        }
        Ok(children)
    }

    async fn open(&self, path: &Path) -> Result<Reader, String> {
        Ok(match self {
            Self::Local => Box::pin(
                tokio::fs::File::open(path)
                    .await
                    .map_err(|error| format!("cannot open {}: {error}", path.display()))?,
            ),
            Self::Remote(remote) => Box::pin(remote.open(path).await?),
        })
    }

    async fn create(&self, path: &Path) -> Result<Writer, String> {
        Ok(match self {
            Self::Local => Box::pin(
                tokio::fs::File::create(path)
                    .await
                    .map_err(|error| format!("cannot create {}: {error}", path.display()))?,
            ),
            Self::Remote(remote) => Box::pin(remote.create(path).await?),
        })
    }

    async fn create_dir(&self, path: &Path) -> Result<(), String> {
        match self {
            Self::Local => match tokio::fs::create_dir(path).await {
                Err(error)
                    if !tokio::fs::metadata(path)
                        .await
                        .is_ok_and(|meta| meta.is_dir()) =>
                {
                    Err(format!("cannot create folder {}: {error}", path.display()))
                }
                _ => Ok(()),
            },
            Self::Remote(remote) => remote.create_dir(path).await,
        }
    }

    async fn remove_file(&self, path: &Path) -> Result<(), String> {
        match self {
            Self::Local => tokio::fs::remove_file(path)
                .await
                .map_err(|error| format!("cannot delete {}: {error}", path.display())),
            Self::Remote(remote) => remote.remove_file(path).await,
        }
    }

    async fn remove_dir(&self, path: &Path) -> Result<(), String> {
        match self {
            Self::Local => tokio::fs::remove_dir(path)
                .await
                .map_err(|error| format!("cannot delete folder {}: {error}", path.display())),
            Self::Remote(remote) => remote.remove_dir(path).await,
        }
    }

    async fn set_modified(&self, path: &Path, time: SystemTime) -> Result<(), String> {
        match self {
            Self::Local => {
                let path = path.to_path_buf();
                tokio::task::spawn_blocking(move || {
                    std::fs::File::options()
                        .write(true)
                        .open(&path)
                        .and_then(|file| file.set_modified(time))
                        .map_err(|error| {
                            format!("cannot set the time of {}: {error}", path.display())
                        })
                })
                .await
                .map_err(|error| error.to_string())?
            }
            Self::Remote(remote) => {
                let seconds = time
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .and_then(|since| u32::try_from(since.as_secs()).ok())
                    .ok_or_else(|| format!("{} has a date SFTP cannot store", path.display()))?;
                remote.set_modified(path, seconds).await
            }
        }
    }
}

/// A [`Node`] from SFTP file attributes.
fn sftp_node(metadata: &russh_sftp::client::fs::Metadata) -> Node {
    Node {
        is_dir: metadata.is_dir(),
        size: metadata.size.unwrap_or(0),
        modified: metadata
            .mtime
            .map(|seconds| UNIX_EPOCH + Duration::from_secs(u64::from(seconds))),
    }
}

/// Measures a transfer's speed and estimates the time left, from progress reports.
#[derive(Debug, Clone)]
pub struct RateMeter {
    started: Instant,
    /// Last sample used for the speed: when, and bytes done then.
    last: (Instant, u64),
    /// Smoothed speed in bytes per second, once measured.
    rate: Option<f64>,
}

impl RateMeter {
    /// Start measuring a transfer that begins at `now` with nothing done.
    pub fn new(now: Instant) -> Self {
        Self {
            started: now,
            last: (now, 0),
            rate: None,
        }
    }

    /// Start measuring a transfer that already has `done` bytes done at `now` (e.g. a
    /// file whose first progress report just arrived).
    pub fn starting_from(now: Instant, done: u64) -> Self {
        Self {
            started: now,
            last: (now, done),
            rate: None,
        }
    }

    /// Note that `done` bytes are done at `now`.
    pub fn record(&mut self, done: u64, now: Instant) {
        let (then, done_then) = self.last;
        let elapsed = now.saturating_duration_since(then);
        if elapsed < MIN_SAMPLE {
            return;
        }
        let sample = done.saturating_sub(done_then) as f64 / elapsed.as_secs_f64();
        self.rate = Some(
            self.rate
                .map_or(sample, |rate| rate + SMOOTHING * (sample - rate)),
        );
        self.last = (now, done);
    }

    /// Smoothed speed in bytes per second, once there is enough data.
    pub fn bytes_per_second(&self) -> Option<f64> {
        self.rate
    }

    /// Estimated time left, shown only for long transfers: after [`ESTIMATE_AFTER`],
    /// and when the whole transfer looks like taking at least [`ESTIMATE_MIN_TOTAL`].
    ///
    /// # Arguments
    ///
    /// * `done` - Bytes done
    /// * `total` - Bytes in all
    /// * `now` - Current time
    pub fn time_left(&self, done: u64, total: u64, now: Instant) -> Option<Duration> {
        let rate = self.rate.filter(|rate| *rate > 0.0)?;
        let elapsed = now.saturating_duration_since(self.started);
        let left = Duration::from_secs_f64(total.saturating_sub(done) as f64 / rate);
        (elapsed >= ESTIMATE_AFTER && elapsed + left >= ESTIMATE_MIN_TOTAL).then_some(left)
    }

    /// Progress as one line, e.g. "45% · 12.3 MiB of 27.0 MiB · 11.2 MiB/s · 0:12 left".
    /// The speed and time left are added once known.
    ///
    /// # Arguments
    ///
    /// * `done` - Bytes done
    /// * `total` - Bytes in all
    /// * `now` - Current time
    pub fn describe(&self, done: u64, total: u64, now: Instant) -> String {
        let mut text = format!(
            "{}% · {} of {}",
            percent(done, total),
            format_size(done),
            format_size(total)
        );
        if let Some(rate) = self.rate {
            // Rounded to whole bytes for display.
            text.push_str(&format!(" · {}/s", format_size(rate as u64)));
        }
        if let Some(left) = self.time_left(done, total, now) {
            text.push_str(&format!(" · {} left", format_duration(left)));
        }
        text
    }
}

/// Whole percent of `total` that `done` is; an empty total counts as complete.
pub fn percent(done: u64, total: u64) -> u64 {
    if total == 0 {
        100
    } else {
        (u128::from(done.min(total)) * 100 / u128::from(total)) as u64
    }
}

/// `done` as a fraction of `total` (0.0 to 1.0) for a progress bar.
pub fn fraction(done: u64, total: u64) -> f64 {
    if total == 0 {
        1.0
    } else {
        done.min(total) as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Build a source tree: `a/one.txt`, `a/sub/two.txt`, `top.txt`.
    fn make_tree(root: &Path) {
        std::fs::create_dir_all(root.join("a/sub")).expect("create folders");
        std::fs::write(root.join("a/one.txt"), "one").expect("write");
        std::fs::write(root.join("a/sub/two.txt"), "two!").expect("write");
        std::fs::write(root.join("top.txt"), vec![7_u8; 600_000]).expect("write");
    }

    /// Run a job between two local folders, collecting its events.
    async fn run_local(job: Job) -> (JobSummary, Vec<TransferEvent>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink: Sink = {
            let events = Arc::clone(&events);
            Arc::new(move |event| events.lock().expect("lock").push(event))
        };
        let summary = execute(
            &job,
            Endpoint::Local,
            Some(Endpoint::Local),
            &CancelFlag::default(),
            &sink,
        )
        .await;
        let events = events.lock().expect("lock").clone();
        (summary, events)
    }

    fn job(kind: JobKind, sources: Vec<PathBuf>, dest_dir: &Path) -> Job {
        Job {
            kind,
            side: Side::Local,
            sources,
            dest_dir: dest_dir.to_path_buf(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn copy_reproduces_the_tree_and_reports_every_item() {
        // Arrange
        let temp = tempfile::tempdir().expect("temp dir");
        let (src, dst) = (temp.path().join("src"), temp.path().join("dst"));
        make_tree(&src);
        std::fs::create_dir(&dst).expect("dst");

        // Act
        let (summary, events) = run_local(job(
            JobKind::Copy,
            vec![src.join("a"), src.join("top.txt")],
            &dst,
        ))
        .await;

        // Assert: 5 items (a, top.txt, one.txt, sub, two.txt), all copied, sources kept.
        assert_eq!(
            summary,
            JobSummary {
                done: 5,
                ..JobSummary::default()
            }
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("a/sub/two.txt")).expect("read"),
            "two!"
        );
        assert_eq!(
            std::fs::read(dst.join("top.txt")).expect("read").len(),
            600_000
        );
        assert!(src.join("a/sub/two.txt").exists());
        let TransferEvent::Planned(items) = &events[0] else {
            panic!("first event must be the plan");
        };
        assert_eq!(items.len(), 5);
        let folder_a = items.iter().find(|item| item.name == "a").expect("a");
        assert_eq!(folder_a.size, 7, "folder size is the total of its files");
        let finished = events
            .iter()
            .filter(|event| matches!(event, TransferEvent::ItemFinished { .. }))
            .count();
        assert_eq!(finished, 5);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn copy_keeps_the_modification_time() {
        let temp = tempfile::tempdir().expect("temp dir");
        let (src, dst) = (temp.path().join("src"), temp.path().join("dst"));
        make_tree(&src);
        std::fs::create_dir(&dst).expect("dst");
        let old = UNIX_EPOCH + Duration::from_secs(1_500_000_000);
        std::fs::File::options()
            .write(true)
            .open(src.join("top.txt"))
            .and_then(|file| file.set_modified(old))
            .expect("set time");

        run_local(job(JobKind::Copy, vec![src.join("top.txt")], &dst)).await;

        let copied = std::fs::metadata(dst.join("top.txt")).and_then(|meta| meta.modified());
        assert_eq!(copied.expect("time"), old);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn move_deletes_sources_only_after_copying() {
        let temp = tempfile::tempdir().expect("temp dir");
        let (src, dst) = (temp.path().join("src"), temp.path().join("dst"));
        make_tree(&src);
        std::fs::create_dir(&dst).expect("dst");

        let (summary, _) = run_local(job(JobKind::Move, vec![src.join("a")], &dst)).await;

        assert_eq!(summary.done, 4);
        assert!(!src.join("a").exists(), "moved folder is gone");
        assert_eq!(
            std::fs::read_to_string(dst.join("a/one.txt")).expect("read"),
            "one"
        );
        assert!(src.join("top.txt").exists(), "unselected file untouched");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn failed_copy_keeps_the_source_of_a_move() {
        let temp = tempfile::tempdir().expect("temp dir");
        let src = temp.path().join("src");
        make_tree(&src);

        // The destination folder does not exist, so the copy fails.
        let (summary, _) = run_local(job(
            JobKind::Move,
            vec![src.join("top.txt")],
            &temp.path().join("missing"),
        ))
        .await;

        assert_eq!(summary.failed, 1);
        assert!(src.join("top.txt").exists());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn delete_removes_a_link_but_not_what_it_points_to() {
        // Arrange: doomed/link -> keep (a folder outside what is deleted).
        let temp = tempfile::tempdir().expect("temp dir");
        let (doomed, keep) = (temp.path().join("doomed"), temp.path().join("keep"));
        std::fs::create_dir_all(&doomed).expect("doomed");
        std::fs::create_dir_all(&keep).expect("keep");
        std::fs::write(keep.join("precious.txt"), "x").expect("write");
        std::os::unix::fs::symlink(&keep, doomed.join("link")).expect("symlink");

        // Act
        let (summary, _) = run_local(job(JobKind::Delete, vec![doomed.clone()], temp.path())).await;

        // Assert
        assert_eq!(summary.done, 2);
        assert!(!doomed.exists());
        assert!(keep.join("precious.txt").exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancelled_job_copies_nothing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let (src, dst) = (temp.path().join("src"), temp.path().join("dst"));
        make_tree(&src);
        std::fs::create_dir(&dst).expect("dst");
        let cancel = CancelFlag::default();
        cancel.cancel();
        let sink: Sink = Arc::new(|_| {});

        let summary = execute(
            &job(JobKind::Copy, vec![src.join("top.txt")], &dst),
            Endpoint::Local,
            Some(Endpoint::Local),
            &cancel,
            &sink,
        )
        .await;

        assert_eq!(summary.cancelled, 1);
        assert!(!dst.join("top.txt").exists());
    }

    #[test]
    fn rate_meter_estimates_only_long_transfers() {
        let start = Instant::now();
        let mut meter = RateMeter::new(start);
        let total = 100_000_000;

        // 1 MB/s: 100 s in all, so an estimate appears after 2 s.
        meter.record(1_000_000, start + Duration::from_secs(1));
        assert_eq!(
            meter.time_left(1_000_000, total, start + Duration::from_secs(1)),
            None
        );
        meter.record(3_000_000, start + Duration::from_secs(3));
        let left = meter
            .time_left(3_000_000, total, start + Duration::from_secs(3))
            .expect("estimate");
        assert_eq!(left.as_secs(), 97);

        // A 5 MB file at the same speed finishes in 5 s: no estimate.
        assert_eq!(
            meter.time_left(3_000_000, 5_000_000, start + Duration::from_secs(3)),
            None
        );
    }

    #[test]
    fn rate_meter_describes_progress() {
        let start = Instant::now();
        let mut meter = RateMeter::new(start);
        assert_eq!(meter.describe(0, 2048, start), "0% · 0 B of 2.0 KiB");
        meter.record(1024, start + Duration::from_secs(1));
        assert_eq!(
            meter.describe(1024, 2048, start + Duration::from_secs(1)),
            "50% · 1.0 KiB of 2.0 KiB · 1.0 KiB/s"
        );
    }

    #[test]
    fn percent_and_fraction_handle_empty_totals() {
        assert_eq!(percent(0, 0), 100);
        assert_eq!(percent(1, 3), 33);
        assert!((fraction(1, 4) - 0.25).abs() < f64::EPSILON);
        assert!((fraction(5, 0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn job_titles_name_the_direction() {
        let one = Job {
            kind: JobKind::Copy,
            side: Side::Local,
            sources: vec![PathBuf::from("/home/me/report.pdf")],
            dest_dir: PathBuf::from("/srv"),
        };
        assert_eq!(one.title(), "Upload report.pdf to /srv");
        let many = Job {
            kind: JobKind::Delete,
            side: Side::Remote,
            sources: vec![PathBuf::from("/a"), PathBuf::from("/b")],
            dest_dir: PathBuf::new(),
        };
        assert_eq!(many.title(), "Delete 2 items from the server");
    }
}
