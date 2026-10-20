//! The transfer lists: Queue (jobs running), Completed and Failed. Each job shows its
//! folders and files as a tree with a progress bar per item, speeds, and a time-left
//! estimate for long transfers. A finished job flashes green (or red), fades, and moves
//! from the Queue to Completed (or Failed, where it can be retried).
//!
//! Every job runs as its own background task (see [`shuttlescp_core::transfer`]), so jobs
//! proceed side by side; their progress arrives here as command messages.
//!
//! Each list is a [`gtk::ListView`] of the rows currently expanded: only the rows on
//! screen have widgets, so a job with hundreds of thousands of files stays fast.
//! Progress updates go straight to the widgets of visible rows.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use relm4::gtk::prelude::*;
use relm4::gtk::{gio, glib};
use relm4::prelude::*;

use shuttlescp_core::format::format_size;
use shuttlescp_core::pending::{PendingJob, PendingTransfers};
use shuttlescp_core::remote::Remote;
use shuttlescp_core::sites::new_site_id;
use shuttlescp_core::transfer::{
    self, CancelFlag, ItemKind, ItemOutcome, Job, JobKind, JobSummary, PlanItem, RateMeter, Side,
    TransferEvent,
};

use crate::conflict_dialog;

/// Jobs with more items than this start with their folders collapsed.
const EXPAND_LIMIT: usize = 200;
/// Width of the progress bars, in pixels.
const BAR_WIDTH: i32 = 160;
/// Indent of each tree level, in pixels.
const INDENT: i32 = 18;
/// Width of the slot at the end of each line for the result icon (or a job's Cancel /
/// Retry button), so bars and texts stay lined up whatever it shows.
const END_SLOT_WIDTH: i32 = 48;
/// A finished job flashes (green, or red if it failed) in the Queue this long...
const FLASH_FOR: Duration = Duration::from_millis(900);
/// ...then fades out this long (matching style.css) before moving to Completed or Failed.
const FADE_FOR: Duration = Duration::from_millis(450);
/// The Completed list keeps this many jobs; older ones are dropped.
const MAX_COMPLETED: usize = 500;
/// Width of the slot for the expander arrow; files get an empty one, so their names
/// line up with folder names.
const EXPANDER_SLOT_WIDTH: i32 = 20;
/// Icon of a finished item.
const DONE_ICON: &str = "object-select-symbolic";
/// Icon of a failed item.
const FAILED_ICON: &str = "dialog-error-symbolic";
/// Icon of a cancelled or skipped item.
const STOPPED_ICON: &str = "process-stop-symbolic";
/// Name of the panel's action group, used by the row buttons.
const ACTION_GROUP: &str = "transfers";

/// The three transfer lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    /// Jobs running (and just finished, on their way out).
    Queue,
    /// Jobs that finished without problems.
    Completed,
    /// Jobs that failed or were cancelled; they can be retried.
    Failed,
}

impl ListKind {
    /// Position in arrays indexed by list.
    fn index(self) -> usize {
        match self {
            Self::Queue => 0,
            Self::Completed => 1,
            Self::Failed => 2,
        }
    }
}

/// The unfinished-transfers file, shared by the transfer panels of every session so
/// they never overwrite each other's records.
///
/// A record is *claimed* while a job using it is in some panel's lists, so another
/// session connected to the same site does not offer to resume it as well.
pub struct PendingStore {
    path: PathBuf,
    transfers: RefCell<PendingTransfers>,
    claimed: RefCell<HashSet<String>>,
}

impl PendingStore {
    /// Read the file at `path` (see [`PendingTransfers::load`]).
    pub fn load(path: PathBuf) -> Rc<Self> {
        Rc::new(Self {
            transfers: RefCell::new(PendingTransfers::load(&path)),
            path,
            claimed: RefCell::default(),
        })
    }

    /// Record a job, claimed by the panel showing it.
    fn add(&self, pending: PendingJob) {
        self.claimed.borrow_mut().insert(pending.key.clone());
        self.transfers.borrow_mut().add(pending);
    }

    /// Forget a job's record.
    fn remove(&self, key: &str) {
        self.claimed.borrow_mut().remove(key);
        self.transfers.borrow_mut().remove(key);
    }

    /// Let other sessions offer a record again (its panel is gone).
    fn release(&self, key: &str) {
        self.claimed.borrow_mut().remove(key);
    }

    /// Whether some panel shows a job using this record.
    fn is_claimed(&self, key: &str) -> bool {
        self.claimed.borrow().contains(key)
    }

    /// A site's records that no panel shows: its interrupted jobs.
    fn unclaimed_for_site(&self, site_id: &str) -> Vec<PendingJob> {
        let claimed = self.claimed.borrow();
        self.transfers
            .borrow()
            .for_site(site_id)
            .filter(|pending| !claimed.contains(&pending.key))
            .cloned()
            .collect()
    }

    /// Write the file, logging (not failing) on error.
    fn save(&self) {
        if let Err(error) = self.transfers.borrow().save(&self.path) {
            tracing::warn!("cannot record unfinished transfers: {error}");
        }
    }
}

/// The server connection jobs use.
#[derive(Debug, Clone)]
pub struct PanelConnection {
    /// Id of the connected site, to record unfinished jobs against.
    pub site_id: String,
    /// The open session.
    pub remote: Remote,
}

/// Messages handled by the transfers panel.
#[derive(Debug)]
pub enum TransferPanelMsg {
    /// Run a job.
    Start(Job),
    /// Stop the job with this id.
    Cancel(u64),
    /// Run an unfinished job again, continuing where it stopped.
    Retry(u64),
    /// Show or hide the contents of a job (`item` is `None`) or folder.
    Toggle {
        /// The job.
        job: u64,
        /// The folder, or `None` for the whole job.
        item: Option<usize>,
    },
    /// Remove every job from the Completed or Failed list.
    Clear(ListKind),
    /// Remove one finished job from its list.
    Dismiss(u64),
    /// Start fading out a finished job in the Queue.
    Fade(u64),
    /// Move a finished job from the Queue to Completed or Failed.
    Shelve(u64),
    /// A server was connected, or (`None`) disconnected: jobs using the old one stop.
    Connection(Option<PanelConnection>),
    /// Resume the jobs interrupted in an earlier session.
    ResumeInterrupted,
    /// Forget the jobs interrupted in an earlier session.
    DiscardInterrupted,
}

/// Notifications sent to the main window.
#[derive(Debug)]
pub enum TransferPanelOutput {
    /// A job finished; the file panes may need refreshing.
    JobFinished,
    /// The files and folders running jobs use changed: the panes show them as busy.
    InUse {
        /// On this computer.
        local: Vec<PathBuf>,
        /// On the server.
        remote: Vec<PathBuf>,
    },
    /// The number of jobs in the lists changed.
    Counts {
        /// Jobs still running.
        running: usize,
        /// Jobs in the Completed list.
        completed: usize,
        /// Jobs in the Failed list.
        failed: usize,
    },
}

/// Identifies a row: a job's header line, or one of its items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RowKey {
    job: u64,
    item: Option<usize>,
}

/// One file or folder of a job.
struct ItemState {
    name: String,
    source: PathBuf,
    kind: ItemKind,
    parent: Option<usize>,
    depth: i32,
    /// Bytes in all (a folder: the files inside it) and done.
    size: u64,
    done: u64,
    /// Speed, from the first progress report on (for folders: from the start).
    meter: Option<RateMeter>,
    outcome: Option<ItemOutcome>,
    expanded: bool,
    /// Waiting for the user to decide about an existing file.
    asking: bool,
}

/// One job in the list.
struct JobState {
    job: Job,
    /// Its entry in the unfinished-transfers file, if it uses the server.
    pending_key: Option<String>,
    cancel: CancelFlag,
    /// Cancelled by the user (not by a disconnect), so it is not offered for resuming.
    user_cancelled: bool,
    items: Vec<ItemState>,
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
    planned: bool,
    /// Bytes in all and done, over every file.
    total: u64,
    done: u64,
    /// Items finished, for deletes (which have no bytes to count).
    finished_items: usize,
    meter: RateMeter,
    summary: Option<JobSummary>,
    expanded: bool,
    /// Which list it is in.
    list: ListKind,
    /// Just finished: flashing before it leaves the Queue.
    flashing: bool,
    /// Fading out, about to leave the Queue.
    fading: bool,
    /// When it reached Completed or Failed (a counter), to drop the oldest first.
    shelved: u64,
}

impl JobState {
    fn is_running(&self) -> bool {
        self.summary.is_none()
    }
}

/// The widgets of one row. Rows are recycled as the list scrolls; `key` says which
/// row they currently show.
#[derive(Clone)]
struct RowWidgets {
    key: Rc<Cell<Option<RowKey>>>,
    indent: gtk::Box,
    expander: gtk::Button,
    icon: gtk::Image,
    name: gtk::Label,
    bar: gtk::ProgressBar,
    info: gtk::Label,
    status: gtk::Image,
    cancel: gtk::Button,
    retry: gtk::Button,
    /// The whole line, faded out with its job.
    line: gtk::Box,
    /// Removes a finished job from the list.
    remove: gtk::Button,
}

/// State shared by the panel and its list rows.
#[derive(Default)]
struct Shared {
    jobs: BTreeMap<u64, JobState>,
    /// The widgets of the rows on screen.
    bound: HashMap<RowKey, RowWidgets>,
}

/// The transfers panel component. Its root is the Queue page; the Completed and Failed
/// pages are added to the window separately (see [`TransferPanel::history_pages`]).
pub struct TransferPanel {
    shared: Rc<RefCell<Shared>>,
    /// The visible rows of each list, in order (each a `BoxedAnyObject` holding a
    /// [`RowKey`]), indexed by [`ListKind::index`].
    rows: [gio::ListStore; 3],
    next_id: u64,
    connection: Option<PanelConnection>,
    pending: Rc<PendingStore>,
    /// Jobs of this site interrupted in an earlier session, offered for resuming.
    interrupted: Vec<PendingJob>,
    /// "Queue empty" and the like, shown over each list while it is empty.
    placeholders: [gtk::Label; 3],
    /// The Completed and Failed pages.
    completed_page: gtk::Box,
    failed_page: gtk::Box,
    /// The Clear buttons of the Completed and Failed pages.
    clear_buttons: [gtk::Button; 2],
    banner: gtk::Box,
    banner_text: gtk::Label,
    /// Counts last reported to the main window: running, completed, failed.
    counts: (usize, usize, usize),
    /// Paths in use last reported to the main window: local, remote.
    in_use: (Vec<PathBuf>, Vec<PathBuf>),
    /// Counter giving each shelved job its place in time.
    next_shelf: u64,
}

#[relm4::component(pub)]
impl Component for TransferPanel {
    /// Where unfinished transfers are recorded (shared by all sessions).
    type Init = Rc<PendingStore>;
    type Input = TransferPanelMsg;
    type Output = TransferPanelOutput;
    type CommandOutput = (u64, TransferEvent);

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            add_css_class: "transfer-panel",

            #[local_ref]
            banner -> gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 6,
                add_css_class: "transfer-banner",
                set_visible: false,

                #[local_ref]
                banner_text -> gtk::Label {
                    set_hexpand: true,
                    set_xalign: 0.0,
                },
                gtk::Button {
                    set_label: "Resume",
                    connect_clicked => TransferPanelMsg::ResumeInterrupted,
                },
                gtk::Button {
                    set_label: "Discard",
                    set_tooltip_text: Some("Forget them; their partial files stay where they are"),
                    connect_clicked => TransferPanelMsg::DiscardInterrupted,
                },
            },
            gtk::Overlay {
                set_vexpand: true,
                add_overlay: placeholder,

                gtk::ScrolledWindow {
                    set_hscrollbar_policy: gtk::PolicyType::Never,

                    #[local_ref]
                    list -> gtk::ListView {},
                },
            },
        }
    }

    fn init(
        pending: Rc<PendingStore>,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let shared = Rc::new(RefCell::new(Shared::default()));
        let rows: [gio::ListStore; 3] =
            std::array::from_fn(|_| gio::ListStore::new::<glib::BoxedAnyObject>());
        let lists: [gtk::ListView; 3] = std::array::from_fn(|index| {
            let list = gtk::ListView::new(
                Some(gtk::NoSelection::new(Some(rows[index].clone()))),
                Some(row_factory(&shared)),
            );
            list.add_css_class("transfer-list");
            list
        });
        let placeholders = [
            "Queue empty",
            "No completed transfers",
            "No failed transfers",
        ]
        .map(|text| {
            let placeholder = gtk::Label::new(Some(text));
            placeholder.add_css_class("dim-label");
            // Clicks go through to the list underneath.
            placeholder.set_can_target(false);
            placeholder
        });
        let clear_buttons = [ListKind::Completed, ListKind::Failed].map(|kind| {
            let button = gtk::Button::builder()
                .label("Clear")
                .has_frame(false)
                .sensitive(false)
                .build();
            let sender = sender.clone();
            button.connect_clicked(move |_| sender.input(TransferPanelMsg::Clear(kind)));
            button
        });
        let [queue_list, completed_list, failed_list] = lists;
        let completed_page = history_page(&completed_list, &placeholders[1], &clear_buttons[0]);
        let failed_page = history_page(&failed_list, &placeholders[2], &clear_buttons[1]);
        // The row buttons of all three pages trigger the same actions.
        for page in [&root, &completed_page, &failed_page] {
            register_actions(page, &sender);
        }

        let model = Self {
            shared,
            rows,
            next_id: 0,
            connection: None,
            pending,
            interrupted: Vec::new(),
            placeholders,
            completed_page,
            failed_page,
            clear_buttons,
            banner: gtk::Box::default(),
            banner_text: gtk::Label::default(),
            counts: (0, 0, 0),
            in_use: (Vec::new(), Vec::new()),
            next_shelf: 0,
        };
        let (placeholder, banner, banner_text, list) = (
            &model.placeholders[0],
            &model.banner,
            &model.banner_text,
            &queue_list,
        );
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            TransferPanelMsg::Start(job) => self.start(job, None, &sender),
            TransferPanelMsg::Cancel(id) => {
                if let Some(job) = self.shared.borrow_mut().jobs.get_mut(&id) {
                    tracing::info!("cancelling transfer {id}");
                    job.user_cancelled = true;
                    job.cancel.cancel();
                }
            }
            TransferPanelMsg::Retry(id) => self.retry(id, &sender),
            TransferPanelMsg::Toggle { job, item } => self.toggle(RowKey { job, item }),
            TransferPanelMsg::Clear(kind) => self.clear(kind),
            TransferPanelMsg::Dismiss(id) => {
                let finished = self
                    .shared
                    .borrow()
                    .jobs
                    .get(&id)
                    .is_some_and(|job| !job.is_running());
                if finished {
                    self.remove_job(id);
                    self.save_pending();
                }
            }
            TransferPanelMsg::Fade(id) => {
                let mut shared = self.shared.borrow_mut();
                if let Some(job) = shared.jobs.get_mut(&id) {
                    job.fading = true;
                }
                refresh_job(&shared, id);
            }
            TransferPanelMsg::Shelve(id) => self.shelve(id),
            TransferPanelMsg::Connection(connection) => self.set_connection(connection),
            TransferPanelMsg::ResumeInterrupted => {
                for pending in std::mem::take(&mut self.interrupted) {
                    // Another session may have resumed it meanwhile.
                    if self.pending.is_claimed(&pending.key) {
                        continue;
                    }
                    tracing::info!("resuming interrupted transfer: {}", pending.job.title());
                    let mut job = pending.job;
                    job.resume = true;
                    self.start(job, Some(pending.key), &sender);
                }
                self.banner.set_visible(false);
            }
            TransferPanelMsg::DiscardInterrupted => {
                for pending in std::mem::take(&mut self.interrupted) {
                    tracing::info!("discarding interrupted transfer: {}", pending.job.title());
                    self.pending.remove(&pending.key);
                }
                self.save_pending();
                self.banner.set_visible(false);
            }
        }
        self.update_chrome(&sender);
    }

    fn update_cmd(
        &mut self,
        (id, event): Self::CommandOutput,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        let now = Instant::now();
        match event {
            TransferEvent::Planned(items) => self.show_plan(id, &items, now),
            TransferEvent::Resumed { id: item, from } => {
                let mut shared = self.shared.borrow_mut();
                let changed = resumed(&mut shared, id, item, from, now);
                refresh(&shared, &changed, now);
            }
            TransferEvent::Progress { id: item, done } => {
                let mut shared = self.shared.borrow_mut();
                let changed = progress(&mut shared, id, item, done, now);
                refresh(&shared, &changed, now);
            }
            TransferEvent::ItemFinished { id: item, outcome } => {
                let mut shared = self.shared.borrow_mut();
                let changed = item_finished(&mut shared, id, item, outcome, now);
                refresh(&shared, &changed, now);
            }
            TransferEvent::Conflict {
                id: item,
                existing,
                incoming,
                reply,
            } => self.ask_conflict(root, id, item, existing, incoming, reply, now),
            TransferEvent::JobFinished(summary) => {
                self.job_finished(id, summary, &sender);
                self.update_chrome(&sender);
                let _ = sender.output(TransferPanelOutput::JobFinished);
            }
        }
    }
}

impl Drop for TransferPanel {
    /// The session closed: its unfinished jobs can be offered again by others (or the
    /// next time the site is connected).
    fn drop(&mut self) {
        for job in self.shared.borrow().jobs.values() {
            if let Some(key) = &job.pending_key {
                self.pending.release(key);
            }
        }
    }
}

impl TransferPanel {
    /// Add a job to the list and start it in the background.
    ///
    /// # Arguments
    ///
    /// * `job` - What to do
    /// * `pending_key` - Its entry in the unfinished-transfers file, when resuming
    /// * `sender` - Runs the job and receives its events
    fn start(&mut self, job: Job, pending_key: Option<String>, sender: &ComponentSender<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        let remote = self
            .connection
            .as_ref()
            .map(|connection| connection.remote.clone());
        // Record jobs that use the server, so they can be resumed if interrupted.
        let pending_key = match (&self.connection, job.uses_server()) {
            (Some(connection), true) => {
                let key = pending_key.unwrap_or_else(new_site_id);
                self.pending.add(PendingJob {
                    key: key.clone(),
                    site_id: connection.site_id.clone(),
                    job: job.clone(),
                });
                self.save_pending();
                Some(key)
            }
            _ => None,
        };
        let cancel = CancelFlag::default();
        self.shared.borrow_mut().jobs.insert(
            id,
            JobState {
                job: job.clone(),
                pending_key,
                cancel: cancel.clone(),
                user_cancelled: false,
                items: Vec::new(),
                children: Vec::new(),
                roots: Vec::new(),
                planned: false,
                total: 0,
                done: 0,
                finished_items: 0,
                meter: RateMeter::new(Instant::now()),
                summary: None,
                expanded: true,
                list: ListKind::Queue,
                flashing: false,
                fading: false,
                shelved: 0,
            },
        );
        self.store(ListKind::Queue).append(&row_object(RowKey {
            job: id,
            item: None,
        }));

        // The job runs as its own task on the async runtime's worker threads; its events
        // come back to `update_cmd` on the UI thread.
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    transfer::run(job, remote, cancel, move |event| {
                        // Fails only once the panel is gone, when nobody needs the event.
                        let _ = out.send((id, event));
                    })
                    .await;
                })
                .drop_on_shutdown()
        });
    }

    /// Run a finished, incomplete job again (back in the Queue), continuing where it
    /// stopped.
    fn retry(&mut self, id: u64, sender: &ComponentSender<Self>) {
        let Some(old) = self.shared.borrow_mut().jobs.remove(&id) else {
            return;
        };
        self.remove_rows(id, old.list);
        tracing::info!("retrying: {}", old.job.title());
        let mut job = old.job;
        job.resume = true;
        self.start(job, old.pending_key, sender);
    }

    /// Remove a job from the list, and its entry in the unfinished-transfers file (the
    /// user has seen how it ended). Call [`TransferPanel::save_pending`] afterwards.
    fn remove_job(&mut self, id: u64) {
        let Some(removed) = self.shared.borrow_mut().jobs.remove(&id) else {
            return;
        };
        if let Some(key) = &removed.pending_key {
            self.pending.remove(key);
        }
        self.remove_rows(id, removed.list);
    }

    /// Empty the Completed or Failed list (and forget its jobs' entries in the
    /// unfinished-transfers file: the user has seen how they ended).
    fn clear(&mut self, kind: ListKind) {
        let cleared: Vec<u64> = self
            .shared
            .borrow()
            .jobs
            .iter()
            .filter(|(_, job)| job.list == kind && !job.is_running())
            .map(|(&id, _)| id)
            .collect();
        for id in cleared {
            self.remove_job(id);
        }
        self.save_pending();
    }

    /// Move a finished job from the Queue to the top of Completed (or Failed), folded
    /// to its header line.
    fn shelve(&mut self, id: u64) {
        let target = {
            let shared = self.shared.borrow();
            match shared.jobs.get(&id) {
                Some(job) if job.list == ListKind::Queue && !job.is_running() => {
                    if job.summary.as_ref().is_some_and(JobSummary::is_complete) {
                        ListKind::Completed
                    } else {
                        ListKind::Failed
                    }
                }
                _ => return,
            }
        };
        self.remove_rows(id, ListKind::Queue);
        self.next_shelf += 1;
        if let Some(job) = self.shared.borrow_mut().jobs.get_mut(&id) {
            job.list = target;
            job.expanded = false;
            job.flashing = false;
            job.fading = false;
            job.shelved = self.next_shelf;
        }
        self.store(target).insert(
            0,
            &row_object(RowKey {
                job: id,
                item: None,
            }),
        );
        // Keep the Completed list from growing without end.
        let oldest: Vec<u64> = {
            let shared = self.shared.borrow();
            let mut completed: Vec<(u64, u64)> = shared
                .jobs
                .iter()
                .filter(|(_, job)| job.list == ListKind::Completed)
                .map(|(&id, job)| (job.shelved, id))
                .collect();
            completed.sort_unstable();
            let excess = completed.len().saturating_sub(MAX_COMPLETED);
            completed
                .into_iter()
                .take(excess)
                .map(|(_, id)| id)
                .collect()
        };
        for id in oldest {
            self.remove_job(id);
        }
    }

    /// Switch to a new connection (or none): stop jobs using the old one, and offer to
    /// resume the new site's interrupted jobs.
    fn set_connection(&mut self, connection: Option<PanelConnection>) {
        for (id, job) in &self.shared.borrow().jobs {
            if job.is_running() && job.job.uses_server() {
                tracing::info!("stopping transfer {id}: disconnected");
                job.cancel.cancel();
            }
        }
        self.interrupted.clear();
        if let Some(connection) = &connection {
            self.interrupted = self.pending.unclaimed_for_site(&connection.site_id);
        }
        let count = self.interrupted.len();
        if count > 0 {
            tracing::info!("{count} interrupted transfers can be resumed");
        }
        self.banner.set_visible(count > 0);
        self.banner_text.set_label(&match count {
            1 => String::from("1 transfer was interrupted and can be resumed."),
            count => format!("{count} transfers were interrupted and can be resumed."),
        });
        self.connection = connection;
    }

    /// Ask the user what to do about a file already at the destination.
    #[allow(clippy::too_many_arguments)]
    fn ask_conflict(
        &self,
        root: &gtk::Box,
        id: u64,
        item: usize,
        existing: transfer::FileInfo,
        incoming: transfer::FileInfo,
        reply: transfer::ConflictReply,
        now: Instant,
    ) {
        let (name, dest) = {
            let mut shared = self.shared.borrow_mut();
            let Some(job) = shared.jobs.get_mut(&id) else {
                return;
            };
            let dest = job.job.dest_dir.display().to_string();
            let Some(state) = job.items.get_mut(item) else {
                return;
            };
            state.asking = true;
            let name = state.name.clone();
            refresh(
                &shared,
                &[RowKey {
                    job: id,
                    item: Some(item),
                }],
                now,
            );
            (name, dest)
        };
        let Some(window) = root.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let shared = Rc::clone(&self.shared);
        conflict_dialog::ask(&window, &name, &dest, existing, incoming, move |answer| {
            if let Some(state) = shared
                .borrow_mut()
                .jobs
                .get_mut(&id)
                .and_then(|job| job.items.get_mut(item))
            {
                state.asking = false;
            }
            reply.answer(answer);
        });
    }

    /// Show a job's items once its plan arrives.
    fn show_plan(&mut self, id: u64, plan: &[PlanItem], now: Instant) {
        let new_rows = {
            let mut shared = self.shared.borrow_mut();
            let Some(job) = shared.jobs.get_mut(&id) else {
                return;
            };
            let expanded = plan.len() <= EXPAND_LIMIT;
            let mut depths: Vec<i32> = Vec::with_capacity(plan.len());
            job.items = plan
                .iter()
                .map(|item| {
                    let depth = item.parent.map_or(0, |parent| depths[parent] + 1);
                    depths.push(depth);
                    ItemState {
                        name: item.name.clone(),
                        source: item.source.clone(),
                        kind: item.kind,
                        parent: item.parent,
                        depth,
                        size: item.size,
                        done: 0,
                        meter: (item.kind == ItemKind::Folder).then(|| RateMeter::new(now)),
                        outcome: None,
                        expanded,
                        asking: false,
                    }
                })
                .collect();
            job.children = vec![Vec::new(); plan.len()];
            job.roots.clear();
            for item in plan {
                match item.parent {
                    Some(parent) => job.children[parent].push(item.id),
                    None => job.roots.push(item.id),
                }
            }
            job.total = plan
                .iter()
                .filter(|item| item.kind == ItemKind::File)
                .map(|item| item.size)
                .sum();
            job.planned = true;
            job.meter = RateMeter::new(now);
            let rows = visible_below(
                job,
                RowKey {
                    job: id,
                    item: None,
                },
            );
            refresh(
                &shared,
                &[RowKey {
                    job: id,
                    item: None,
                }],
                now,
            );
            rows
        };
        // The borrow is released first: adding rows makes GTK fill them right away.
        if let Some((list, position)) = self.position_of(RowKey {
            job: id,
            item: None,
        }) {
            let objects: Vec<glib::Object> = new_rows.into_iter().map(row_object).collect();
            self.store(list).splice(position + 1, 0, &objects);
        }
    }

    /// The whole job finished: it flashes green (or red), fades, and moves to
    /// Completed (or Failed).
    fn job_finished(&mut self, id: u64, summary: JobSummary, sender: &ComponentSender<Self>) {
        let forget = {
            let mut shared = self.shared.borrow_mut();
            let Some(job) = shared.jobs.get_mut(&id) else {
                return;
            };
            // Nothing left to resume: done, or the user chose to stop it.
            let forget = (summary.is_complete() || job.user_cancelled)
                .then(|| job.pending_key.clone())
                .flatten();
            job.summary = Some(summary);
            job.flashing = true;
            refresh_job(&shared, id);
            forget
        };
        if let Some(key) = forget {
            self.pending.remove(&key);
            self.save_pending();
        }
        for (delay, message) in [
            (
                FLASH_FOR,
                TransferPanelMsg::Fade as fn(u64) -> TransferPanelMsg,
            ),
            (FLASH_FOR + FADE_FOR, TransferPanelMsg::Shelve),
        ] {
            let sender = sender.clone();
            glib::timeout_add_local_once(delay, move || sender.input(message(id)));
        }
    }

    /// Expand or collapse a job or folder.
    fn toggle(&mut self, key: RowKey) {
        let Some((list, position)) = self.position_of(key) else {
            return;
        };
        let (removed, added) = {
            let mut shared = self.shared.borrow_mut();
            let Some(job) = shared.jobs.get_mut(&key.job) else {
                return;
            };
            let before = visible_below(job, key).len();
            match key.item {
                None => job.expanded = !job.expanded,
                Some(item) => {
                    if let Some(state) = job.items.get_mut(item) {
                        state.expanded = !state.expanded;
                    }
                }
            }
            let after = visible_below(job, key);
            refresh(&shared, &[key], Instant::now());
            (before, after)
        };
        let objects: Vec<glib::Object> = added.into_iter().map(row_object).collect();
        // `splice` counts in u32; a job never has anywhere near 4 billion rows.
        let removed = u32::try_from(removed).unwrap_or(u32::MAX);
        self.store(list).splice(position + 1, removed, &objects);
    }

    /// Remove a job's rows from a list.
    fn remove_rows(&mut self, id: u64, list: ListKind) {
        let store = self.store(list);
        let header = row_object(RowKey {
            job: id,
            item: None,
        });
        let Some(start) = position_in(store, row_key(&header)) else {
            return;
        };
        let mut end = start + 1;
        while store
            .item(end)
            .is_some_and(|object| row_key(&object).is_some_and(|key| key.job == id))
        {
            end += 1;
        }
        store.splice(start, end - start, &[] as &[glib::Object]);
    }

    /// The list a row is in, and its position there.
    fn position_of(&self, key: RowKey) -> Option<(ListKind, u32)> {
        let list = self.shared.borrow().jobs.get(&key.job)?.list;
        position_in(self.store(list), Some(key)).map(|position| (list, position))
    }

    /// The rows of a list.
    fn store(&self, list: ListKind) -> &gio::ListStore {
        &self.rows[list.index()]
    }

    /// The Completed and Failed pages, for the main window to show as tabs.
    pub fn history_pages(&self) -> (gtk::Widget, gtk::Widget) {
        (
            self.completed_page.clone().upcast(),
            self.failed_page.clone().upcast(),
        )
    }

    /// Show each list's placeholder while it is empty, enable the Clear buttons while
    /// there is something to clear, and tell the main window the counts and the paths
    /// in use.
    fn update_chrome(&mut self, sender: &ComponentSender<Self>) {
        let counts = {
            let shared = self.shared.borrow();
            let count =
                |list: ListKind| shared.jobs.values().filter(|job| job.list == list).count();
            (
                shared.jobs.values().filter(|job| job.is_running()).count(),
                count(ListKind::Completed),
                count(ListKind::Failed),
            )
        };
        let queued = self
            .shared
            .borrow()
            .jobs
            .values()
            .any(|job| job.list == ListKind::Queue);
        self.placeholders[0].set_visible(!queued);
        self.placeholders[1].set_visible(counts.1 == 0);
        self.placeholders[2].set_visible(counts.2 == 0);
        self.clear_buttons[0].set_sensitive(counts.1 > 0);
        self.clear_buttons[1].set_sensitive(counts.2 > 0);
        let in_use = {
            let shared = self.shared.borrow();
            let mut in_use = (Vec::new(), Vec::new());
            for (side, path) in shared
                .jobs
                .values()
                .filter(|job| job.is_running())
                .flat_map(|job| job.job.paths_in_use())
            {
                match side {
                    Side::Local => in_use.0.push(path),
                    Side::Remote => in_use.1.push(path),
                }
            }
            in_use
        };
        if in_use != self.in_use {
            self.in_use = in_use.clone();
            let _ = sender.output(TransferPanelOutput::InUse {
                local: in_use.0,
                remote: in_use.1,
            });
        }
        if counts != self.counts {
            self.counts = counts;
            let _ = sender.output(TransferPanelOutput::Counts {
                running: counts.0,
                completed: counts.1,
                failed: counts.2,
            });
        }
    }

    /// Write the unfinished-transfers file, logging (not failing) on error.
    fn save_pending(&self) {
        self.pending.save();
    }
}

/// Position of a row in a list.
fn position_in(store: &gio::ListStore, key: Option<RowKey>) -> Option<u32> {
    let key = key?;
    (0..store.n_items())
        .find(|&position| store.item(position).and_then(|object| row_key(&object)) == Some(key))
}

/// A Completed or Failed page: a Clear button over the list.
fn history_page(list: &gtk::ListView, placeholder: &gtk::Label, clear: &gtk::Button) -> gtk::Box {
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    toolbar.add_css_class("compact-toolbar");
    toolbar.set_halign(gtk::Align::End);
    toolbar.append(clear);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(list)
        .build();
    let overlay = gtk::Overlay::builder()
        .vexpand(true)
        .child(&scrolled)
        .build();
    overlay.add_overlay(placeholder);
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("transfer-panel");
    page.append(&toolbar);
    page.append(&overlay);
    page
}

/// Redraw every visible row of a job.
fn refresh_job(shared: &Shared, id: u64) {
    let keys: Vec<RowKey> = shared
        .bound
        .keys()
        .filter(|key| key.job == id)
        .copied()
        .collect();
    refresh(shared, &keys, Instant::now());
}

/// A list item holding a row key.
fn row_object(key: RowKey) -> glib::Object {
    glib::BoxedAnyObject::new(key).upcast()
}

/// The row key held by a list item.
fn row_key(object: &glib::Object) -> Option<RowKey> {
    object
        .downcast_ref::<glib::BoxedAnyObject>()
        .and_then(|boxed| boxed.try_borrow::<RowKey>().ok().map(|key| *key))
}

/// The rows shown under `key` while it is expanded (none if it is collapsed), in tree
/// order.
fn visible_below(job: &JobState, key: RowKey) -> Vec<RowKey> {
    let (expanded, start): (bool, &[usize]) = match key.item {
        None => (job.expanded, &job.roots),
        Some(item) => (
            job.items.get(item).is_some_and(|state| state.expanded),
            job.children.get(item).map_or(&[], Vec::as_slice),
        ),
    };
    let mut rows = Vec::new();
    if !expanded {
        return rows;
    }
    // Depth-first, keeping the plan's order among siblings.
    let mut stack: Vec<usize> = start.iter().rev().copied().collect();
    while let Some(item) = stack.pop() {
        rows.push(RowKey {
            job: key.job,
            item: Some(item),
        });
        if job.items[item].expanded {
            stack.extend(job.children[item].iter().rev().copied());
        }
    }
    rows
}

/// Record a file's progress, and add it to the folders it is in and the job.
///
/// # Returns
///
/// The rows to redraw
fn progress(shared: &mut Shared, id: u64, item: usize, done: u64, now: Instant) -> Vec<RowKey> {
    let Some(job) = shared.jobs.get_mut(&id) else {
        return Vec::new();
    };
    let Some(state) = job.items.get_mut(item) else {
        return Vec::new();
    };
    let added = done.saturating_sub(state.done);
    state.done = done;
    state
        .meter
        .get_or_insert_with(|| RateMeter::starting_from(now, done))
        .record(done, now);
    let mut changed = vec![RowKey {
        job: id,
        item: Some(item),
    }];
    let mut parent = state.parent;
    while let Some(folder_id) = parent {
        let Some(folder) = job.items.get_mut(folder_id) else {
            break;
        };
        folder.done += added;
        if let Some(meter) = &mut folder.meter {
            meter.record(folder.done, now);
        }
        changed.push(RowKey {
            job: id,
            item: Some(folder_id),
        });
        parent = folder.parent;
    }
    job.done += added;
    job.meter.record(job.done, now);
    changed.push(RowKey {
        job: id,
        item: None,
    });
    changed
}

/// A file continues an earlier copy: count its first `from` bytes as done, for it, the
/// folders it is in and the job, without counting them towards their speeds.
///
/// # Returns
///
/// The rows to redraw
fn resumed(shared: &mut Shared, id: u64, item: usize, from: u64, now: Instant) -> Vec<RowKey> {
    let Some(job) = shared.jobs.get_mut(&id) else {
        return Vec::new();
    };
    let Some(state) = job.items.get_mut(item) else {
        return Vec::new();
    };
    state.done = from;
    state.meter = Some(RateMeter::starting_from(now, from));
    let mut changed = vec![RowKey {
        job: id,
        item: Some(item),
    }];
    let mut parent = state.parent;
    while let Some(folder_id) = parent {
        let Some(folder) = job.items.get_mut(folder_id) else {
            break;
        };
        folder.done += from;
        if let Some(meter) = &mut folder.meter {
            meter.add_done_earlier(from);
        }
        changed.push(RowKey {
            job: id,
            item: Some(folder_id),
        });
        parent = folder.parent;
    }
    job.done += from;
    job.meter.add_done_earlier(from);
    changed.push(RowKey {
        job: id,
        item: None,
    });
    changed
}

/// Record how an item ended.
///
/// # Returns
///
/// The rows to redraw
fn item_finished(
    shared: &mut Shared,
    id: u64,
    item: usize,
    outcome: ItemOutcome,
    now: Instant,
) -> Vec<RowKey> {
    // A finished file counts in full, even if its last progress report was skipped.
    let full = shared
        .jobs
        .get(&id)
        .and_then(|job| job.items.get(item))
        .filter(|state| outcome == ItemOutcome::Done && state.kind == ItemKind::File)
        .map(|state| state.size);
    let mut changed = match full {
        Some(size) => progress(shared, id, item, size, now),
        None => vec![RowKey {
            job: id,
            item: None,
        }],
    };
    if let Some(job) = shared.jobs.get_mut(&id) {
        job.finished_items += 1;
        if let Some(state) = job.items.get_mut(item) {
            state.outcome = Some(outcome);
            state.asking = false;
        }
    }
    changed.push(RowKey {
        job: id,
        item: Some(item),
    });
    changed
}

/// Redraw the given rows, if they are on screen.
fn refresh(shared: &Shared, keys: &[RowKey], now: Instant) {
    for key in keys {
        if let Some(widgets) = shared.bound.get(key) {
            fill(widgets, shared, *key, now);
        }
    }
}

/// What a row shows.
struct RowView {
    indent: i32,
    expandable: bool,
    expanded: bool,
    icon: &'static str,
    name: String,
    tooltip: String,
    heading: bool,
    fraction: Option<f64>,
    info: String,
    error: bool,
    status: Option<&'static str>,
    can_cancel: bool,
    can_retry: bool,
    /// A finished job's header: it can be removed from the list.
    can_remove: bool,
}

/// Show a row's current state in its widgets.
fn fill(widgets: &RowWidgets, shared: &Shared, key: RowKey, now: Instant) {
    let Some(job) = shared.jobs.get(&key.job) else {
        return;
    };
    let row = match key.item {
        None => job_row(job, now),
        Some(item) => match job.items.get(item) {
            Some(state) => item_row(job, state, now),
            None => return,
        },
    };
    widgets.key.set(Some(key));
    widgets.indent.set_size_request(row.indent, -1);
    widgets.expander.set_visible(row.expandable);
    widgets.expander.set_icon_name(if row.expanded {
        "pan-down-symbolic"
    } else {
        "pan-end-symbolic"
    });
    widgets.icon.set_icon_name(Some(row.icon));
    widgets.name.set_label(&row.name);
    widgets.name.set_tooltip_text(Some(&row.tooltip));
    set_class(&widgets.name, "heading", row.heading);
    widgets.bar.set_visible(row.fraction.is_some());
    widgets.bar.set_fraction(row.fraction.unwrap_or(0.0));
    widgets.info.set_label(&row.info);
    widgets.info.set_tooltip_text(Some(&row.info));
    set_class(&widgets.info, "error", row.error);
    widgets.status.set_icon_name(row.status);
    widgets.cancel.set_visible(row.can_cancel);
    widgets.retry.set_visible(row.can_retry);
    widgets
        .status
        .set_visible(!row.can_cancel && !row.can_retry);
    widgets.remove.set_visible(row.can_remove);
    let complete = job.summary.as_ref().is_some_and(JobSummary::is_complete);
    set_class(&widgets.line, "finished-ok", job.flashing && complete);
    set_class(&widgets.line, "finished-bad", job.flashing && !complete);
    set_class(&widgets.line, "fading", job.fading);
}

/// Add or remove a CSS class.
fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

/// A job's header line.
fn job_row(job: &JobState, now: Instant) -> RowView {
    let deleting = job.job.kind == JobKind::Delete;
    let icon = match (job.job.kind, job.job.side) {
        (JobKind::Delete, _) => "user-trash-symbolic",
        _ if job.job.same_side => "go-next-symbolic",
        (_, Side::Local) => "go-up-symbolic",
        (_, Side::Remote) => "go-down-symbolic",
    };
    let fraction = if deleting {
        transfer::fraction(job.finished_items as u64, job.items.len() as u64)
    } else {
        transfer::fraction(job.done, job.total)
    };
    let (info, error, status) = match &job.summary {
        None if !job.planned => (String::from("Preparing..."), false, None),
        None if deleting => (
            format!("{} of {} deleted", job.finished_items, job.items.len()),
            false,
            None,
        ),
        None => (job.meter.describe(job.done, job.total, now), false, None),
        Some(summary) => summary_text(job, summary),
    };
    let title = if job.job.resume {
        format!("{} (resumed)", job.job.title())
    } else {
        job.job.title()
    };
    RowView {
        indent: 0,
        expandable: job.planned && !job.roots.is_empty(),
        expanded: job.expanded,
        icon,
        tooltip: title.clone(),
        name: title,
        heading: true,
        fraction: match &job.summary {
            // A job that could not start has nothing to show progress of.
            Some(summary) if summary.error.is_some() => None,
            Some(summary) if summary.is_complete() => Some(1.0),
            _ => Some(fraction),
        },
        info,
        error,
        status,
        can_cancel: job.is_running(),
        can_retry: job
            .summary
            .as_ref()
            .is_some_and(|summary| !summary.is_complete()),
        // Finished jobs in the Queue are on their way out on their own.
        can_remove: !job.is_running() && job.list != ListKind::Queue,
    }
}

/// The text, error flag and icon of a finished job.
fn summary_text(job: &JobState, summary: &JobSummary) -> (String, bool, Option<&'static str>) {
    let count = |count: usize, what: &str| {
        if count > 0 {
            format!(", {count} {what}")
        } else {
            String::new()
        }
    };
    let (cancelled, skipped) = (
        count(summary.cancelled, "cancelled"),
        count(summary.skipped, "skipped"),
    );
    if let Some(error) = &summary.error {
        (error.clone(), true, Some(FAILED_ICON))
    } else if summary.failed > 0 {
        (
            format!(
                "{} done, {} failed{cancelled}{skipped}",
                summary.done, summary.failed
            ),
            true,
            Some(FAILED_ICON),
        )
    } else if summary.cancelled > 0 {
        (
            format!("Cancelled: {} done{cancelled}{skipped}", summary.done),
            false,
            Some(STOPPED_ICON),
        )
    } else if job.job.kind == JobKind::Delete {
        (
            format!("{} deleted", items_text(summary.done)),
            false,
            Some(DONE_ICON),
        )
    } else {
        (
            format!(
                "{} · {}{skipped}",
                items_text(summary.done + summary.skipped),
                format_size(job.total)
            ),
            false,
            Some(DONE_ICON),
        )
    }
}

/// A file or folder line.
fn item_row(job: &JobState, item: &ItemState, now: Instant) -> RowView {
    let deleting = job.job.kind == JobKind::Delete;
    let (info, error, status) = match &item.outcome {
        Some(ItemOutcome::Done) if deleting => (String::from("Deleted"), false, Some(DONE_ICON)),
        Some(ItemOutcome::Done) => (format_size(item.size), false, Some(DONE_ICON)),
        Some(ItemOutcome::Skipped) => (
            String::from("Skipped: the existing file was kept"),
            false,
            Some(STOPPED_ICON),
        ),
        Some(ItemOutcome::Failed(error)) => (error.clone(), true, Some(FAILED_ICON)),
        Some(ItemOutcome::Cancelled) => (String::from("Cancelled"), false, Some(STOPPED_ICON)),
        None if item.asking => (
            String::from("Already exists: waiting for your answer"),
            false,
            None,
        ),
        None if deleting => (String::new(), false, None),
        None if item.kind == ItemKind::File && item.meter.is_none() => {
            (String::from("Waiting"), false, None)
        }
        None => (
            item.meter.as_ref().map_or_else(
                || format!("{}%", transfer::percent(item.done, item.size)),
                |meter| meter.describe(item.done, item.size, now),
            ),
            false,
            None,
        ),
    };
    RowView {
        indent: (item.depth + 1) * INDENT,
        expandable: item.kind == ItemKind::Folder,
        expanded: item.expanded,
        icon: match item.kind {
            ItemKind::Folder => "folder",
            ItemKind::File => "text-x-generic",
        },
        name: item.name.clone(),
        tooltip: item.source.display().to_string(),
        heading: false,
        fraction: (!deleting).then(|| match item.outcome {
            Some(ItemOutcome::Done) => 1.0,
            _ => transfer::fraction(item.done, item.size),
        }),
        info,
        error,
        status,
        can_cancel: false,
        can_retry: false,
        can_remove: false,
    }
}

/// "1 item" or "n items".
fn items_text(count: usize) -> String {
    if count == 1 {
        String::from("1 item")
    } else {
        format!("{count} items")
    }
}

/// The factory that builds, fills and recycles the list's rows.
fn row_factory(shared: &Rc<RefCell<Shared>>) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    // The widgets of each row, by the address of its list item (stable while it lives).
    let rows: Rc<RefCell<HashMap<usize, RowWidgets>>> = Rc::default();
    let address = |list_item: &gtk::ListItem| list_item.as_ptr() as usize;
    {
        let rows = Rc::clone(&rows);
        factory.connect_setup(move |_, object| {
            let list_item = object;
            let (root, widgets) = build_row();
            list_item.set_child(Some(&root));
            list_item.set_activatable(false);
            rows.borrow_mut().insert(address(object), widgets);
        });
    }
    {
        let (rows, shared) = (Rc::clone(&rows), Rc::clone(shared));
        factory.connect_bind(move |_, object| {
            let list_item = object;
            let (Some(key), Some(widgets)) = (
                list_item.item().as_ref().and_then(row_key),
                rows.borrow().get(&address(object)).cloned(),
            ) else {
                return;
            };
            let mut shared = shared.borrow_mut();
            shared.bound.insert(key, widgets.clone());
            fill(&widgets, &shared, key, Instant::now());
        });
    }
    {
        let (rows, shared) = (Rc::clone(&rows), Rc::clone(shared));
        factory.connect_unbind(move |_, object| {
            let Some(widgets) = rows.borrow().get(&address(object)).cloned() else {
                return;
            };
            if let Some(key) = widgets.key.take() {
                let mut shared = shared.borrow_mut();
                // Only if the row has not been bound to other widgets since.
                if shared
                    .bound
                    .get(&key)
                    .is_some_and(|bound| Rc::ptr_eq(&bound.key, &widgets.key))
                {
                    shared.bound.remove(&key);
                }
            }
        });
    }
    factory.connect_teardown(move |_, object| {
        rows.borrow_mut().remove(&address(object));
    });
    factory
}

/// Build one (empty) row: indent, expander, icon, name, bar, text, end slot.
fn build_row() -> (gtk::Box, RowWidgets) {
    let key: Rc<Cell<Option<RowKey>>> = Rc::default();
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    line.add_css_class("transfer-row");
    let indent = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let expander = small_button("pan-down-symbolic", None);
    let expander_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    expander_slot.set_size_request(EXPANDER_SLOT_WIDTH, -1);
    expander_slot.append(&expander);
    let icon = gtk::Image::new();
    let name = gtk::Label::builder()
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .width_chars(12)
        .build();
    let bar = gtk::ProgressBar::builder()
        .width_request(BAR_WIDTH)
        .valign(gtk::Align::Center)
        .build();
    let info = gtk::Label::builder()
        .xalign(0.0)
        .width_chars(44)
        .max_width_chars(44)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["dim-label", "transfer-info"])
        .build();
    let status = gtk::Image::new();
    let cancel = small_button("window-close-symbolic", Some("Cancel"));
    let retry = small_button(
        "view-refresh-symbolic",
        Some("Retry: continue where it stopped"),
    );
    let end_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    end_slot.set_size_request(END_SLOT_WIDTH, -1);
    end_slot.append(&status);
    end_slot.append(&cancel);
    end_slot.append(&retry);
    // Only finished jobs show it; running ones show Cancel, which looks the same.
    let remove = small_button("window-close-symbolic", Some("Remove from the list"));
    end_slot.append(&remove);
    line.append(&indent);
    line.append(&expander_slot);
    line.append(&icon);
    line.append(&name);
    line.append(&bar);
    line.append(&info);
    line.append(&end_slot);

    // The buttons trigger the panel's actions with the row they currently show.
    let send = |button: &gtk::Button, action: &'static str| {
        let key = Rc::clone(&key);
        button.connect_clicked(move |button| {
            let Some(RowKey { job, item }) = key.get() else {
                return;
            };
            // -1 stands for "the job itself" in the action's parameter.
            let item = item.map_or(-1, |item| i64::try_from(item).unwrap_or(-1));
            let _ = button.activate_action(
                &format!("{ACTION_GROUP}.{action}"),
                Some(&(job, item).to_variant()),
            );
        });
    };
    send(&expander, "toggle");
    send(&cancel, "cancel");
    send(&retry, "retry");
    send(&remove, "dismiss");
    (
        line.clone(),
        RowWidgets {
            key,
            indent,
            expander,
            icon,
            name,
            bar,
            info,
            status,
            cancel,
            retry,
            line,
            remove,
        },
    )
}

/// A small flat icon button.
fn small_button(icon: &str, tooltip: Option<&str>) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .has_frame(false)
        .css_classes(["tab-close"])
        .build();
    button.set_tooltip_text(tooltip);
    button
}

/// The panel's actions, triggered by the row buttons with a (job, item) parameter.
fn register_actions(page: &gtk::Box, sender: &ComponentSender<TransferPanel>) {
    let group = gio::SimpleActionGroup::new();
    let add = |name: &str, message: fn(u64, Option<usize>) -> TransferPanelMsg| {
        let action = gio::SimpleAction::new(name, Some(&<(u64, i64)>::static_variant_type()));
        let sender = sender.clone();
        action.connect_activate(move |_, parameter| {
            if let Some((job, item)) = parameter.and_then(|value| value.get::<(u64, i64)>()) {
                sender.input(message(job, usize::try_from(item).ok()));
            }
        });
        group.add_action(&action);
    };
    add("toggle", |job, item| TransferPanelMsg::Toggle { job, item });
    add("cancel", |job, _| TransferPanelMsg::Cancel(job));
    add("dismiss", |job, _| TransferPanelMsg::Dismiss(job));
    add("retry", |job, _| TransferPanelMsg::Retry(job));
    page.insert_action_group(ACTION_GROUP, Some(&group));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(key: &str) -> PendingJob {
        PendingJob {
            key: key.to_owned(),
            site_id: String::from("site"),
            job: Job {
                kind: JobKind::Copy,
                side: Side::Local,
                sources: vec![PathBuf::from("/data/a")],
                dest_dir: PathBuf::from("/srv"),
                same_side: false,
                resume: false,
            },
        }
    }

    #[test]
    fn claimed_records_are_not_offered_to_other_sessions() {
        // Arrange: one record from an earlier run, one started by a session now.
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("pending.json");
        let mut earlier = PendingTransfers::default();
        earlier.add(pending("old"));
        earlier.save(&path).expect("save");
        let store = PendingStore::load(path.clone());
        store.add(pending("running"));

        // Act + Assert: only the earlier one is offered for resuming.
        let offered: Vec<String> = store
            .unclaimed_for_site("site")
            .into_iter()
            .map(|pending| pending.key)
            .collect();
        assert_eq!(offered, ["old"]);

        // Its session closes: the running one can be offered again.
        store.release("running");
        assert_eq!(store.unclaimed_for_site("site").len(), 2);

        // Both records reach the file, which every session shares.
        store.save();
        assert_eq!(PendingTransfers::load(&path).for_site("site").count(), 2);
        store.remove("old");
        store.save();
        assert_eq!(PendingTransfers::load(&path).for_site("site").count(), 1);
    }
}
