//! One session: a tab in the main window with a local file pane on the left and the
//! remote pane of its connection (if any) on the right, plus its own transfer lists
//! (Queue, Completed, Failed) in the bottom panel.
//!
//! Each session is drawn in its own colour: an outline around its tab and panes, and
//! around its transfer tabs (which sit side by side) and their lists, with a light tint
//! of the colour over the theme's background, so it is clear which lists belong to
//! which connection. While a session's transfers run, its tab shows turning arrows and
//! its tabs pulse gently.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use relm4::gtk::glib;
use relm4::gtk::prelude::*;
use relm4::prelude::*;

use shuttlescp_core::format::shorten_middle;
use shuttlescp_core::remote::Remote;
use shuttlescp_core::sites::Site;
use shuttlescp_core::transfer::Side;

use crate::app::{App, AppMsg, NOT_CONNECTED};
use crate::file_pane::{FilePane, FilePaneInit, FilePaneMsg, FilePaneOutput};
use crate::transfer_panel::{
    PanelConnection, PendingStore, TransferPanel, TransferPanelMsg, TransferPanelOutput,
};

/// Identifies a session for its whole life (positions change as tabs close).
pub type SessionId = u64;

/// How many session colours there are (`group-0` ... in style.css); a seventh session
/// shares the first one's colour.
pub const COLOURS: usize = 6;
/// Longest tab title, in characters; longer titles are cut in the middle.
const TAB_TITLE_MAX_CHARS: usize = 32;
/// Icon of a session tab.
const SESSION_TAB_ICON: &str = "computer-symbolic";
/// Icon shown (turning, see style.css) on a session tab while its transfers run. Not a
/// `gtk::Spinner`: GTK 4.14 measures one wrongly inside a notebook tab.
const ACTIVITY_ICON: &str = "emblem-synchronizing-symbolic";
/// How long the Completed tab stays highlighted after a transfer lands in it.
const TAB_HIGHLIGHT: std::time::Duration = std::time::Duration::from_millis(1500);
/// CSS class of a session's transfer list pages in the bottom panel.
const TRANSFERS_PAGE_CLASS: &str = "session-transfers";
/// CSS class marking the tabs of a session while its transfers run (they pulse).
const BUSY_CLASS: &str = "busy";

/// The open connection shown in the right half of a session.
pub struct Connection {
    /// "user@host" of the site, for the status bar.
    pub name: String,
    /// Server host name, for the tab title.
    host: String,
    /// Folder shown in the remote pane.
    pub dir: PathBuf,
    /// The remote file pane; dropping it (and stopping the transfers, which hold the
    /// session too) closes the connection.
    pub pane: Controller<FilePane>,
}

/// A session tab and everything that belongs to it.
pub struct Session {
    pub id: SessionId,
    /// Its colour, `0..COLOURS`.
    pub colour: usize,
    pub local_pane: Controller<FilePane>,
    /// Folder shown in the local pane.
    pub local_dir: PathBuf,
    pub connection: Option<Connection>,
    /// The page in the sessions notebook: an outline around the two panes.
    pub page: gtk::Box,
    /// Left/right split; the right half shows the remote pane.
    split: gtk::Paned,
    /// Shown in the right half while not connected.
    placeholder: gtk::Label,
    tab: SessionTab,
    pub transfers: Controller<TransferPanel>,
    /// The Queue, Completed and Failed pages, in the bottom panel.
    pub transfer_pages: [gtk::Widget; 3],
    transfer_tabs: TransferTabs,
    /// Transfers running.
    pub running: usize,
}

/// The tab label of a session.
struct SessionTab {
    root: gtk::Box,
    activity: gtk::Image,
    label: gtk::Label,
    close: gtk::Button,
}

/// Everything a new session needs.
pub struct SessionInit<'a> {
    pub id: SessionId,
    pub colour: usize,
    /// Folder the local pane starts in.
    pub start_dir: PathBuf,
    /// Last folder shown on each disk, for the local pane's disk drop-down.
    pub last_dir_on_disk: BTreeMap<PathBuf, PathBuf>,
    /// Unfinished transfers, shared by all sessions.
    pub pending: Rc<PendingStore>,
    /// Initial position of the left/right divider, in pixels.
    pub split_position: i32,
    pub sender: &'a ComponentSender<App>,
}

impl Session {
    /// Build a session (not yet connected). The caller adds its page and tab to the
    /// sessions notebook and its transfer pages to the bottom panel.
    pub fn new(init: SessionInit<'_>) -> Self {
        let SessionInit {
            id,
            colour,
            start_dir,
            last_dir_on_disk,
            pending,
            split_position,
            sender,
        } = init;
        let local_pane = FilePane::builder()
            .launch(FilePaneInit {
                start_dir: start_dir.clone(),
                last_dir_on_disk,
                remote: None,
            })
            .forward(sender.input_sender(), move |output| {
                pane_message(id, Side::Local, output)
            });
        let placeholder = gtk::Label::new(Some(NOT_CONNECTED));
        placeholder.add_css_class("dim-label");
        let split = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .position(split_position)
            .wide_handle(true)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .start_child(local_pane.widget())
            .end_child(&placeholder)
            .build();
        // Gives the divider a visible, hoverable handle (see style.css).
        split.add_css_class("pane-split");
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.add_css_class("session-page");
        page.add_css_class(&colour_class(colour));
        split.set_vexpand(true);
        page.append(&split);

        let transfers = TransferPanel::builder().launch(pending).forward(
            sender.input_sender(),
            move |output| match output {
                TransferPanelOutput::JobFinished => AppMsg::TransferFinished(id),
                TransferPanelOutput::Counts {
                    running,
                    completed,
                    failed,
                } => AppMsg::TransferCounts {
                    session: id,
                    running,
                    completed,
                    failed,
                },
            },
        );
        let (completed_page, failed_page) = transfers.model().history_pages();
        let transfer_pages: [gtk::Widget; 3] = [
            transfers.widget().clone().upcast(),
            completed_page,
            failed_page,
        ];
        for page in &transfer_pages {
            page.add_css_class(TRANSFERS_PAGE_CLASS);
            page.add_css_class(&colour_class(colour));
        }
        let transfer_tabs = TransferTabs::new();
        Self {
            id,
            colour,
            local_pane,
            local_dir: start_dir,
            connection: None,
            page,
            split,
            placeholder,
            tab: SessionTab::new(id, sender),
            transfers,
            transfer_pages,
            transfer_tabs,
            running: 0,
        }
    }

    /// The tab label widget, for the sessions notebook.
    pub fn tab_widget(&self) -> &gtk::Box {
        &self.tab.root
    }

    /// The tab label widgets of the Queue, Completed and Failed pages.
    pub fn transfer_tab_widgets(&self) -> [&gtk::Box; 3] {
        [
            &self.transfer_tabs.queue_tab,
            &self.transfer_tabs.completed_tab,
            &self.transfer_tabs.failed_tab,
        ]
    }

    /// Full tab title: `<server> - <folder>`, for the server while connected,
    /// otherwise for this computer (`host`) and the local pane's folder.
    pub fn title(&self, host: &str) -> String {
        let (host, dir) = match &self.connection {
            Some(connection) => (connection.host.as_str(), connection.dir.as_path()),
            None => (host, self.local_dir.as_path()),
        };
        format!("{host} - {}", folder_name(dir))
    }

    /// Connection text for the status bar.
    pub fn status(&self) -> String {
        self.connection.as_ref().map_or_else(
            || String::from(NOT_CONNECTED),
            |connection| format!("Connected to {}", connection.name),
        )
    }

    /// Show a newly connected server in the right half, replacing any previous
    /// connection.
    ///
    /// # Arguments
    ///
    /// * `site` - The site connected to
    /// * `remote` - Its open session
    /// * `sender` - Receives the remote pane's messages
    pub fn connect(&mut self, site: &Site, remote: Remote, sender: &ComponentSender<App>) {
        self.disconnect();
        let start_dir = remote.home().to_path_buf();
        let id = self.id;
        let pane = FilePane::builder()
            .launch(FilePaneInit {
                start_dir: start_dir.clone(),
                last_dir_on_disk: BTreeMap::new(),
                remote: Some(remote.clone()),
            })
            .forward(sender.input_sender(), move |output| {
                pane_message(id, Side::Remote, output)
            });
        self.split.set_end_child(Some(pane.widget()));
        self.transfer_tabs
            .show_connection(Some(&site.display_name()));
        self.transfers
            .emit(TransferPanelMsg::Connection(Some(PanelConnection {
                site_id: site.id.clone(),
                remote: remote.clone(),
            })));
        self.local_pane.emit(FilePaneMsg::PeerAvailable(true));
        tracing::info!("session {id}: connected to {}", site.display_name());
        self.connection = Some(Connection {
            name: site.display_name(),
            host: remote.host().to_owned(),
            dir: start_dir,
            pane,
        });
    }

    /// Close the connection, if any, and show "Not connected" again. Its running
    /// transfers stop; they can be resumed on the next connection to the site.
    pub fn disconnect(&mut self) {
        if let Some(connection) = self.connection.take() {
            tracing::info!("session {}: disconnected from {}", self.id, connection.name);
            self.split.set_end_child(Some(&self.placeholder));
            self.transfer_tabs.show_connection(None);
            // Transfers hold their own handle on the session; the panel stops them so
            // the connection really closes.
            self.transfers.emit(TransferPanelMsg::Connection(None));
            self.local_pane.emit(FilePaneMsg::PeerAvailable(false));
            // Dropping the pane drops its session, which closes the connection.
            drop(connection);
        }
    }

    /// Show the job counts on the transfer tabs.
    pub fn show_counts(&mut self, running: usize, completed: usize, failed: usize) {
        self.running = running;
        self.transfer_tabs.show_counts(running, completed, failed);
    }

    /// Which of its transfer lists `page` is (0 Queue, 1 Completed, 2 Failed), if it is
    /// one of this session's.
    pub fn transfer_list_of(&self, page: &gtk::Widget) -> Option<usize> {
        self.transfer_pages.iter().position(|own| own == page)
    }

    /// The CSS class giving this session its colour, e.g. for the notebook showing it.
    pub fn colour_class(&self) -> String {
        colour_class(self.colour)
    }

    /// Bring the tabs up to date: title, close button, colour outline and activity.
    ///
    /// # Arguments
    ///
    /// * `host` - This computer's name, for the title while not connected
    /// * `closable` - The close button is shown
    pub fn refresh(&self, host: &str, closable: bool) {
        let title = self.title(host);
        self.tab
            .label
            .set_label(&shorten_middle(&title, TAB_TITLE_MAX_CHARS));
        self.tab.root.set_tooltip_text(Some(&title));
        self.tab.close.set_visible(closable);
        let busy = self.running > 0;
        self.tab.activity.set_visible(busy);

        let colour = colour_class(self.colour);
        // The notebooks draw the tabs around our labels: style those.
        let tabs = self.transfer_tab_widgets();
        let notebook_tabs: Vec<(gtk::Widget, bool)> = std::iter::once((&self.tab.root, true))
            .chain(
                tabs.iter()
                    .enumerate()
                    .map(|(index, tab)| (*tab, index == 0)),
            )
            .filter_map(|(label, pulses)| label.parent().map(|tab| (tab, pulses)))
            .collect();
        for (tab, pulses) in &notebook_tabs {
            tab.add_css_class(&colour);
            set_class(tab, BUSY_CLASS, busy && *pulses);
        }
        // The first and last of the three transfer tabs close the group's outline.
        if let [_, first, _, last] = notebook_tabs.as_slice() {
            first.0.add_css_class("group-start");
            last.0.add_css_class("group-end");
        }
    }
}

impl SessionTab {
    /// A session tab label: turning arrows (while transferring), icon, title, close
    /// button.
    fn new(id: SessionId, sender: &ComponentSender<App>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let activity = gtk::Image::from_icon_name(ACTIVITY_ICON);
        activity.add_css_class("session-activity");
        activity.set_visible(false);
        activity.set_tooltip_text(Some("Transfers running"));
        let label = gtk::Label::new(None);
        let close = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .has_frame(false)
            .css_classes(["tab-close"])
            .tooltip_text("Disconnect and close this session")
            .build();
        let sender = sender.clone();
        close.connect_clicked(move |_| sender.input(AppMsg::CloseSession(id)));
        root.append(&activity);
        root.append(&gtk::Image::from_icon_name(SESSION_TAB_ICON));
        root.append(&label);
        root.append(&close);
        Self {
            root,
            activity,
            label,
            close,
        }
    }
}

/// The titles of a session's transfer tabs (Queue, Completed, Failed).
struct TransferTabs {
    queue_tab: gtk::Box,
    queue: gtk::Label,
    completed_tab: gtk::Box,
    completed: gtk::Label,
    failed_tab: gtk::Box,
    failed: gtk::Label,
    /// Name of the connection the lists belong to.
    connection: RefCell<Option<String>>,
    /// Running, completed and failed jobs.
    counts: Cell<(usize, usize, usize)>,
}

impl TransferTabs {
    fn new() -> Self {
        let (queue_tab, queue) = panel_tab("network-transmit-receive-symbolic");
        let (completed_tab, completed) = panel_tab("object-select-symbolic");
        let (failed_tab, failed) = panel_tab("dialog-error-symbolic");
        let tabs = Self {
            queue_tab,
            queue,
            completed_tab,
            completed,
            failed_tab,
            failed,
            connection: RefCell::new(None),
            counts: Cell::new((0, 0, 0)),
        };
        tabs.show_connection(None);
        tabs.show_counts(0, 0, 0);
        tabs
    }

    /// Name the lists after their connection, or none.
    fn show_connection(&self, connection: Option<&str>) {
        let tooltip = |list: &str| match connection {
            Some(name) => format!("{list} for {name}"),
            None => format!("{list} (not connected)"),
        };
        self.queue_tab.set_tooltip_text(Some(&tooltip("Transfers")));
        self.completed_tab
            .set_tooltip_text(Some(&tooltip("Completed transfers")));
        self.failed_tab
            .set_tooltip_text(Some(&tooltip("Failed transfers")));
        self.connection.replace(connection.map(str::to_owned));
        self.update_queue_title();
    }

    /// Show how many jobs each list holds; highlight the Completed tab briefly when a
    /// transfer lands in it.
    fn show_counts(&self, running: usize, completed: usize, failed: usize) {
        let (_, completed_before, _) = self.counts.replace((running, completed, failed));
        self.update_queue_title();
        self.completed.set_label(&titled("Completed", completed));
        self.failed.set_label(&titled("Failed", failed));
        if completed > completed_before {
            self.completed_tab.add_css_class("tab-highlight");
            let tab = self.completed_tab.clone();
            glib::timeout_add_local_once(TAB_HIGHLIGHT, move || {
                tab.remove_css_class("tab-highlight");
            });
        }
    }

    /// "Queue (2) · me@host": the running count and the connection, shortened to fit.
    fn update_queue_title(&self) {
        let mut title = titled("Queue", self.counts.get().0);
        if let Some(name) = self.connection.borrow().as_deref() {
            title.push_str(" · ");
            title.push_str(name);
        }
        // Connection names can be long: keep the tab small by shortening the middle
        // (a GTK ellipsized label would shrink to just "..." in a tab).
        self.queue
            .set_label(&shorten_middle(&title, TAB_TITLE_MAX_CHARS));
    }
}

/// The message a session's file pane output becomes.
fn pane_message(session: SessionId, side: Side, output: FilePaneOutput) -> AppMsg {
    match output {
        FilePaneOutput::DirectoryChanged { dir, disk_root } => match side {
            Side::Local => AppMsg::LocalDirChanged {
                session,
                dir,
                disk_root,
            },
            Side::Remote => AppMsg::RemoteDirChanged { session, dir },
        },
        FilePaneOutput::Request { kind, paths } => AppMsg::PaneRequest {
            session,
            side,
            kind,
            paths,
        },
        FilePaneOutput::Dropped {
            kind,
            paths,
            from_this_pane,
            dest_dir,
        } => AppMsg::PaneDrop {
            session,
            side,
            kind,
            paths,
            from_this_pane,
            dest_dir,
        },
    }
}

/// A bottom panel tab label: a small icon and a name.
///
/// # Returns
///
/// The label widget for the tab, and its text label (to rename the tab later)
pub fn panel_tab(icon: &str) -> (gtk::Box, gtk::Label) {
    let tab = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let text = gtk::Label::new(None);
    tab.append(&gtk::Image::from_icon_name(icon));
    tab.append(&text);
    (tab, text)
}

/// The CSS class giving a session its colour.
pub fn colour_class(colour: usize) -> String {
    format!("group-{}", colour % COLOURS)
}

/// Add or remove a CSS class.
fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

/// The name of a folder for a title; the root folder has none, so it is shown as "/".
fn folder_name(dir: &Path) -> String {
    dir.file_name().map_or_else(
        || dir.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// "Name" or "Name (n)" when there are any.
fn titled(name: &str, count: usize) -> String {
    if count == 0 {
        name.to_owned()
    } else {
        format!("{name} ({count})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_count_only_when_there_is_something() {
        assert_eq!(titled("Queue", 0), "Queue");
        assert_eq!(titled("Failed", 3), "Failed (3)");
    }

    #[test]
    fn folder_names_fall_back_to_the_root() {
        assert_eq!(folder_name(Path::new("/home/me/Documents")), "Documents");
        assert_eq!(folder_name(Path::new("/")), "/");
    }

    #[test]
    fn colours_repeat_after_the_palette() {
        assert_eq!(colour_class(0), "group-0");
        assert_eq!(colour_class(COLOURS + 1), "group-1");
    }
}
