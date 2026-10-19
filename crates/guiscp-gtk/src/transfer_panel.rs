//! The transfers tab: one entry per job, each showing its folders and files as a tree
//! with a progress bar per item, speeds, and a time-left estimate for long transfers.
//!
//! Every job runs as its own background task (see [`guiscp_core::transfer`]), so jobs
//! proceed side by side; their progress arrives here as command messages.

use std::collections::BTreeMap;
use std::time::Instant;

use relm4::gtk;
use relm4::gtk::prelude::*;
use relm4::prelude::*;

use guiscp_core::format::format_size;
use guiscp_core::remote::Remote;
use guiscp_core::transfer::{
    self, CancelFlag, ItemKind, ItemOutcome, Job, JobKind, JobSummary, PlanItem, RateMeter, Side,
    TransferEvent,
};

/// Jobs with more items than this start with their folders collapsed.
const EXPAND_LIMIT: usize = 200;
/// Width of the progress bars, in pixels.
const BAR_WIDTH: i32 = 160;
/// Indent of each tree level, in pixels.
const INDENT: i32 = 18;
/// Icon of a finished item.
const DONE_ICON: &str = "object-select-symbolic";
/// Icon of a failed item.
const FAILED_ICON: &str = "dialog-error-symbolic";
/// Icon of a cancelled item.
const CANCELLED_ICON: &str = "process-stop-symbolic";
/// Width of the slot at the end of each line for the result icon (or a job's Cancel
/// button), so the bars and texts stay lined up whether or not it shows anything.
const END_SLOT_WIDTH: i32 = 24;

/// Messages handled by the transfers panel.
#[derive(Debug)]
pub enum TransferPanelMsg {
    /// Run a job.
    Start {
        /// What to do.
        job: Job,
        /// The server connection, if any.
        remote: Option<Remote>,
    },
    /// Stop the job with this id.
    Cancel(u64),
    /// Stop every job that uses the server (it is being disconnected).
    CancelRemote,
    /// Remove finished jobs from the list.
    ClearFinished,
    /// The connection changed: its name, or `None` when disconnected.
    ShowConnection(Option<String>),
}

/// Notifications sent to the main window.
#[derive(Debug)]
pub enum TransferPanelOutput {
    /// A job finished; the file panes may need refreshing.
    JobFinished,
}

/// The widgets of one line: progress bar, text and result icon.
struct RowWidgets {
    bar: gtk::ProgressBar,
    info: gtk::Label,
    status: gtk::Image,
}

impl RowWidgets {
    /// Show how the item ended.
    fn finish(&self, outcome: &ItemOutcome, done_text: &str) {
        let (icon, text) = match outcome {
            ItemOutcome::Done => (DONE_ICON, done_text.to_owned()),
            ItemOutcome::Failed(error) => (FAILED_ICON, error.clone()),
            ItemOutcome::Cancelled => (CANCELLED_ICON, String::from("Cancelled")),
        };
        if *outcome == ItemOutcome::Done {
            self.bar.set_fraction(1.0);
        }
        self.status.set_icon_name(Some(icon));
        self.info.set_label(&text);
        self.info.set_tooltip_text(Some(&text));
        if matches!(outcome, ItemOutcome::Failed(_)) {
            self.info.add_css_class("error");
        }
    }
}

/// One file or folder of a job.
struct ItemView {
    widgets: RowWidgets,
    kind: ItemKind,
    parent: Option<usize>,
    /// Bytes in all (a folder: the files inside it).
    size: u64,
    /// Bytes done.
    done: u64,
    /// Speed of this item, from its first progress report on.
    meter: Option<RateMeter>,
    /// Holds the children of a folder.
    children: Option<gtk::Box>,
}

/// One job in the list.
struct JobView {
    kind: JobKind,
    side: Side,
    cancel: CancelFlag,
    /// The job's whole entry.
    root: gtk::Box,
    header: RowWidgets,
    cancel_button: gtk::Button,
    /// Holds the job's top-level items.
    items_box: gtk::Box,
    /// Shows or hides the items.
    expander: gtk::Button,
    items: Vec<ItemView>,
    /// Bytes in all and done, over every file.
    total: u64,
    done: u64,
    /// Items finished, for deletes (which have no bytes to count).
    finished_items: usize,
    meter: RateMeter,
    finished: bool,
}

/// The transfers panel component.
pub struct TransferPanel {
    jobs: BTreeMap<u64, JobView>,
    next_id: u64,
    /// Holds the job entries.
    list: gtk::Box,
    /// Shown while there are no jobs.
    placeholder: gtk::Label,
    clear_button: gtk::Button,
}

#[relm4::component(pub)]
impl Component for TransferPanel {
    type Init = ();
    type Input = TransferPanelMsg;
    type Output = TransferPanelOutput;
    type CommandOutput = (u64, TransferEvent);

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            add_css_class: "transfer-panel",

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                add_css_class: "compact-toolbar",
                set_halign: gtk::Align::End,

                #[local_ref]
                clear_button -> gtk::Button {
                    set_label: "Clear Finished",
                    set_has_frame: false,
                    set_sensitive: false,
                    connect_clicked => TransferPanelMsg::ClearFinished,
                },
            },
            gtk::ScrolledWindow {
                set_vexpand: true,
                set_hscrollbar_policy: gtk::PolicyType::Never,

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,

                    #[local_ref]
                    placeholder -> gtk::Label {
                        set_vexpand: true,
                        add_css_class: "dim-label",
                    },
                    #[local_ref]
                    list -> gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 4,
                    },
                },
            },
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let model = Self {
            jobs: BTreeMap::new(),
            next_id: 0,
            list: gtk::Box::default(),
            placeholder: gtk::Label::new(Some(crate::app::NOT_CONNECTED)),
            clear_button: gtk::Button::default(),
        };
        let (list, placeholder, clear_button) =
            (&model.list, &model.placeholder, &model.clear_button);
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            TransferPanelMsg::Start { job, remote } => self.start(job, remote, &sender),
            TransferPanelMsg::Cancel(id) => {
                if let Some(job) = self.jobs.get(&id) {
                    tracing::info!("cancelling transfer {id}");
                    job.cancel.cancel();
                }
            }
            TransferPanelMsg::CancelRemote => {
                for (id, job) in &self.jobs {
                    let uses_server = !(job.kind == JobKind::Delete && job.side == Side::Local);
                    if !job.finished && uses_server {
                        tracing::info!("cancelling transfer {id}: disconnecting");
                        job.cancel.cancel();
                    }
                }
            }
            TransferPanelMsg::ClearFinished => {
                self.jobs.retain(|_, job| {
                    if job.finished {
                        self.list.remove(&job.root);
                    }
                    !job.finished
                });
                self.update_chrome();
            }
            TransferPanelMsg::ShowConnection(name) => {
                self.placeholder.set_label(if name.is_some() {
                    "No transfers yet"
                } else {
                    crate::app::NOT_CONNECTED
                });
            }
        }
    }

    fn update_cmd(
        &mut self,
        (id, event): Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        let Some(job) = self.jobs.get_mut(&id) else {
            return;
        };
        let now = Instant::now();
        match event {
            TransferEvent::Planned(items) => job.show_plan(&items, now),
            TransferEvent::Progress { id: item, done } => job.progress(item, done, now),
            TransferEvent::ItemFinished { id: item, outcome } => {
                job.item_finished(item, &outcome, now)
            }
            TransferEvent::JobFinished(summary) => {
                job.job_finished(&summary);
                self.update_chrome();
                let _ = sender.output(TransferPanelOutput::JobFinished);
            }
        }
    }
}

impl TransferPanel {
    /// Add a job to the list and start it in the background.
    fn start(&mut self, job: Job, remote: Option<Remote>, sender: &ComponentSender<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        let cancel = CancelFlag::default();
        let view = JobView::new(&job, cancel.clone(), {
            let sender = sender.clone();
            move || sender.input(TransferPanelMsg::Cancel(id))
        });
        self.list.append(&view.root);
        self.jobs.insert(id, view);
        self.update_chrome();

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

    /// Show the placeholder only while the list is empty, and enable "Clear Finished"
    /// while there is something to clear.
    fn update_chrome(&self) {
        self.placeholder.set_visible(self.jobs.is_empty());
        self.clear_button
            .set_sensitive(self.jobs.values().any(|job| job.finished));
    }
}

impl JobView {
    /// The job's entry, with a header line until its plan arrives.
    ///
    /// # Arguments
    ///
    /// * `job` - The job
    /// * `cancel` - Its stop request
    /// * `on_cancel` - Called when the Cancel button is clicked
    fn new(job: &Job, cancel: CancelFlag, on_cancel: impl Fn() + 'static) -> Self {
        let icon = match (job.kind, job.side) {
            (JobKind::Delete, _) => "user-trash-symbolic",
            (_, Side::Local) => "go-up-symbolic",
            (_, Side::Remote) => "go-down-symbolic",
        };
        let (line, header, title) = row_line(icon, &job.title());
        title.add_css_class("heading");
        header.info.set_label("Preparing...");
        let cancel_button = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .has_frame(false)
            .tooltip_text("Cancel")
            .css_classes(["tab-close"])
            .build();
        cancel_button.connect_clicked(move |_| on_cancel());
        // The Cancel button takes the result icon's place until the job ends.
        header.status.set_visible(false);
        if let Some(slot) = header.status.parent().and_downcast::<gtk::Box>() {
            slot.append(&cancel_button);
        }

        let items_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        items_box.set_margin_start(INDENT);
        let expander = expander(&items_box, true);
        line.prepend(&expander);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("transfer-job");
        root.append(&line);
        root.append(&items_box);
        Self {
            kind: job.kind,
            side: job.side,
            cancel,
            root,
            header,
            cancel_button,
            items_box,
            expander,
            items: Vec::new(),
            total: 0,
            done: 0,
            finished_items: 0,
            meter: RateMeter::new(Instant::now()),
            finished: false,
        }
    }

    /// Build the tree of items.
    fn show_plan(&mut self, plan: &[PlanItem], now: Instant) {
        let expanded = plan.len() <= EXPAND_LIMIT;
        for item in plan {
            let icon = match item.kind {
                ItemKind::Folder => "folder",
                ItemKind::File => "text-x-generic",
            };
            let (line, widgets, _) = row_line(icon, &item.name);
            line.set_tooltip_text(Some(&item.source.display().to_string()));
            widgets.info.set_label(match (self.kind, item.kind) {
                (JobKind::Delete, _) => "",
                (_, ItemKind::File) => "Waiting",
                (_, ItemKind::Folder) => "",
            });
            if self.kind == JobKind::Delete {
                // Nothing to measure while deleting; the result icon says enough.
                widgets.bar.set_visible(false);
            }
            let entry = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let children = (item.kind == ItemKind::Folder).then(|| {
                let children = gtk::Box::new(gtk::Orientation::Vertical, 0);
                children.set_margin_start(INDENT);
                children.set_visible(expanded);
                line.prepend(&expander(&children, expanded));
                children
            });
            if children.is_none() {
                // Keeps file names lined up with folder names next to an expander.
                let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                spacer.set_size_request(16, -1);
                line.prepend(&spacer);
            }
            entry.append(&line);
            if let Some(children) = &children {
                entry.append(children);
            }
            let container = item
                .parent
                .and_then(|parent| self.items.get(parent))
                .and_then(|parent| parent.children.as_ref())
                .unwrap_or(&self.items_box);
            container.append(&entry);
            self.items.push(ItemView {
                widgets,
                kind: item.kind,
                parent: item.parent,
                size: item.size,
                done: 0,
                meter: (item.kind == ItemKind::Folder).then(|| RateMeter::new(now)),
                children,
            });
        }
        self.total = plan
            .iter()
            .filter(|item| item.kind == ItemKind::File)
            .map(|item| item.size)
            .sum();
        self.meter = RateMeter::new(now);
        self.refresh_header(now);
    }

    /// A file has `done` bytes copied: update it, the folders it is in, and the job.
    fn progress(&mut self, id: usize, done: u64, now: Instant) {
        let Some(item) = self.items.get_mut(id) else {
            return;
        };
        let added = done.saturating_sub(item.done);
        item.done = done;
        item.meter
            .get_or_insert_with(|| RateMeter::starting_from(now, done))
            .record(done, now);
        show_bytes(item, now);
        let mut parent = item.parent;
        while let Some(folder_id) = parent {
            let Some(folder) = self.items.get_mut(folder_id) else {
                break;
            };
            folder.done += added;
            if let Some(meter) = &mut folder.meter {
                meter.record(folder.done, now);
            }
            show_bytes(folder, now);
            parent = folder.parent;
        }
        self.done += added;
        self.meter.record(self.done, now);
        self.refresh_header(now);
    }

    /// An item finished.
    fn item_finished(&mut self, id: usize, outcome: &ItemOutcome, now: Instant) {
        // A finished file counts in full, even if its last progress report was skipped.
        if *outcome == ItemOutcome::Done
            && let Some(item) = self.items.get(id)
            && item.kind == ItemKind::File
        {
            self.progress(id, item.size, now);
        }
        let Some(item) = self.items.get(id) else {
            return;
        };
        let done_text = match self.kind {
            JobKind::Delete => String::from("Deleted"),
            _ => format_size(item.size),
        };
        item.widgets.finish(outcome, &done_text);
        self.finished_items += 1;
        self.refresh_header(now);
    }

    /// The whole job finished.
    fn job_finished(&mut self, summary: &JobSummary) {
        self.finished = true;
        self.cancel_button.set_visible(false);
        self.header.status.set_visible(true);
        // A job that went well folds to its header line; one with problems stays open
        // so they can be seen.
        if summary.failed == 0 && summary.cancelled == 0 && summary.error.is_none() {
            set_expanded(&self.expander, &self.items_box, false);
        }
        let outcome = if let Some(error) = &summary.error {
            ItemOutcome::Failed(error.clone())
        } else if summary.failed > 0 {
            ItemOutcome::Failed(format!(
                "{} done, {} failed{}",
                summary.done,
                summary.failed,
                cancelled_text(summary)
            ))
        } else if summary.cancelled > 0 {
            ItemOutcome::Cancelled
        } else {
            ItemOutcome::Done
        };
        let done_text = match self.kind {
            JobKind::Delete => format!("{} deleted", items_text(summary.done)),
            _ => format!("{} · {}", items_text(summary.done), format_size(self.total)),
        };
        self.header.finish(&outcome, &done_text);
        if summary.cancelled > 0 && summary.failed == 0 && summary.error.is_none() {
            self.header.info.set_label(&format!(
                "Cancelled: {} done{}",
                summary.done,
                cancelled_text(summary)
            ));
        }
    }

    /// Update the header's bar and text.
    fn refresh_header(&self, now: Instant) {
        if self.finished {
            return;
        }
        if self.kind == JobKind::Delete {
            let total = self.items.len();
            self.header
                .bar
                .set_fraction(transfer::fraction(self.finished_items as u64, total as u64));
            self.header
                .info
                .set_label(&format!("{} of {total} deleted", self.finished_items));
        } else {
            self.header
                .bar
                .set_fraction(transfer::fraction(self.done, self.total));
            self.header
                .info
                .set_label(&self.meter.describe(self.done, self.total, now));
        }
    }
}

/// Show an item's byte progress.
fn show_bytes(item: &ItemView, now: Instant) {
    item.widgets
        .bar
        .set_fraction(transfer::fraction(item.done, item.size));
    let text = item.meter.as_ref().map_or_else(
        || format!("{}%", transfer::percent(item.done, item.size)),
        |meter| meter.describe(item.done, item.size, now),
    );
    item.widgets.info.set_label(&text);
}

/// " (n cancelled)" when some items were cancelled, else nothing.
fn cancelled_text(summary: &JobSummary) -> String {
    if summary.cancelled > 0 {
        format!(", {} cancelled", summary.cancelled)
    } else {
        String::new()
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

/// One line of the list: icon, name, progress bar, text and result icon.
///
/// # Returns
///
/// The line, its updatable widgets, and its name label
fn row_line(icon: &str, name: &str) -> (gtk::Box, RowWidgets, gtk::Label) {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    line.add_css_class("transfer-row");
    let name_label = gtk::Label::builder()
        .label(name)
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
    let end_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    end_slot.set_size_request(END_SLOT_WIDTH, -1);
    end_slot.append(&status);
    line.append(&gtk::Image::from_icon_name(icon));
    line.append(&name_label);
    line.append(&bar);
    line.append(&info);
    line.append(&end_slot);
    (line, RowWidgets { bar, info, status }, name_label)
}

/// A small arrow button that shows or hides a folder's (or job's) contents.
fn expander(children: &gtk::Box, expanded: bool) -> gtk::Button {
    let button = gtk::Button::builder()
        .has_frame(false)
        .css_classes(["tab-close"])
        .build();
    set_expanded(&button, children, expanded);
    let children = children.clone();
    button.connect_clicked(move |button| {
        set_expanded(button, &children, !children.is_visible());
    });
    button
}

/// Show or hide an expander's contents, pointing its arrow to match.
fn set_expanded(expander: &gtk::Button, children: &gtk::Box, open: bool) {
    children.set_visible(open);
    expander.set_icon_name(if open {
        "pan-down-symbolic"
    } else {
        "pan-end-symbolic"
    });
}
