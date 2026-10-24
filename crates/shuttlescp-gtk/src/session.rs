//! One session: a tab in the main window with a local file pane on the left and the
//! remote pane of its connection (if any) on the right, plus its own transfer lists
//! (Queue, Completed, Failed) in the bottom panel.
//!
//! A site-to-site session has a server in each pane instead: the left pane shows the
//! left server, and between the panes a picture of this computer, which the copied data
//! passes through, with two lights that blink while transfers run.
//!
//! Each session is drawn in its own colour: an outline around its tab and panes, and
//! around its transfer tabs (which sit side by side) and their lists, with a light tint
//! of the colour over the theme's background, so it is clear which lists belong to
//! which connection. While a session's transfers run, its tab shows turning arrows and
//! its tabs pulse gently.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use relm4::gtk::glib;
use relm4::gtk::prelude::*;
use relm4::prelude::*;

use shuttlescp_core::format::shorten_middle;
use shuttlescp_core::path::{RemotePath, SidePath};
use shuttlescp_core::remote::Remote;
use shuttlescp_core::sites::Site;
use shuttlescp_core::transfer::{Job, JobKind, Side};

use crate::app::{App, AppMsg, NOT_CONNECTED};
use crate::file_pane::{FilePane, FilePaneInit, FilePaneMsg, FilePaneOutput, Peer};
use crate::login_dialog::Resume;
use crate::transfer_panel::{
    LeftConnection, PanelConnection, PendingStore, TransferPanel, TransferPanelMsg,
    TransferPanelOutput,
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
    pub dir: RemotePath,
    /// The remote file pane; dropping it (and stopping the transfers, which hold the
    /// session too) closes the connection.
    pub pane: Controller<FilePane>,
}

/// The left pane's server in a site-to-site session.
pub struct LeftServer {
    /// "user@host" of the site, for the status bar.
    pub name: String,
    /// Id of the site, recorded with unfinished transfers.
    site_id: String,
    /// Server host name, for the tab title.
    host: String,
    /// Folder shown in the left pane.
    pub dir: RemotePath,
    /// The open connection (the left pane holds it too).
    remote: Remote,
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
    /// Transfers paused (they too are unfinished when the app quits).
    pub paused: usize,
    /// Paths on the server that running transfers use, for a remote pane opened later.
    remote_in_use: Vec<SidePath>,
    /// Site to site: the left pane's server.
    pub left_server: Option<LeftServer>,
    /// Site to site: the right half, holding the picture of this computer and then the
    /// right pane (or the "Not connected" placeholder).
    right_half: Option<gtk::Box>,
    /// Site to site: the picture of this computer between the panes.
    relay: Option<RelayPicture>,
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
    /// Site to site: the left pane's site and its open connection (the right one comes
    /// through [`Session::connect`]).
    pub left: Option<(Site, Remote)>,
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
            left,
            sender,
        } = init;
        // The left pane shows this computer, or site to site the left server.
        let left_init = match &left {
            Some((site, remote)) => FilePaneInit {
                start_dir: SidePath::Remote(remote.home().clone()),
                last_dir_on_disk: BTreeMap::new(),
                remote: Some(remote.clone()),
                title: site.display_name(),
            },
            None => FilePaneInit {
                start_dir: SidePath::Local(start_dir.clone()),
                last_dir_on_disk,
                remote: None,
                title: format!("This computer \u{b7} {}", crate::app::computer_name()),
            },
        };
        let local_pane = FilePane::builder()
            .launch(left_init)
            .forward(sender.input_sender(), move |output| {
                pane_message(id, Side::Local, output)
            });
        if let Some((_, remote)) = &left {
            ask_server_name(id, Side::Local, remote.clone(), sender);
        }
        let placeholder = gtk::Label::new(Some(NOT_CONNECTED));
        placeholder.add_css_class("dim-label");
        placeholder.set_hexpand(true);
        // Site to site, the right half starts with the picture of this computer.
        let relay = left.as_ref().map(|_| RelayPicture::new());
        let right_half = relay.as_ref().map(|relay| {
            let half = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            half.append(&relay.root);
            // A bar like the divider on the picture's left, on its right too.
            let bar = gtk::Separator::new(gtk::Orientation::Vertical);
            bar.add_css_class("relay-divider");
            half.append(&bar);
            half.append(&placeholder);
            half
        });
        let split = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .position(split_position)
            .wide_handle(true)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .start_child(local_pane.widget())
            .build();
        match &right_half {
            Some(half) => split.set_end_child(Some(half)),
            None => split.set_end_child(Some(&placeholder)),
        }
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
                TransferPanelOutput::TargetChanged(side) => {
                    AppMsg::TargetChanged { session: id, side }
                }
                TransferPanelOutput::Relaying { to_right, to_left } => AppMsg::Relaying {
                    session: id,
                    to_right,
                    to_left,
                },
                TransferPanelOutput::InUse { local, remote } => AppMsg::PathsInUse {
                    session: id,
                    local,
                    remote,
                },
                TransferPanelOutput::Counts {
                    running,
                    paused,
                    completed,
                    failed,
                } => AppMsg::TransferCounts {
                    session: id,
                    running,
                    paused,
                    completed,
                    failed,
                },
                TransferPanelOutput::Discarded => AppMsg::TransfersDiscarded(id),
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
            paused: 0,
            remote_in_use: Vec::new(),
            left_server: left.map(|(site, remote)| LeftServer {
                name: site.display_name(),
                site_id: site.id.clone(),
                host: remote.host().to_owned(),
                dir: remote.home().clone(),
                remote,
            }),
            right_half,
            relay,
        }
    }

    /// Show `widget` (the right pane or the placeholder) in the right half.
    fn set_right(&self, widget: &impl IsA<gtk::Widget>) {
        match &self.right_half {
            Some(half) => {
                // The picture of this computer and its bar stay; the pane after them
                // changes.
                if let Some(old) = half.last_child()
                    && !old.has_css_class("relay-divider")
                    && self.relay.as_ref().is_none_or(|relay| old != relay.root)
                {
                    half.remove(&old);
                }
                half.append(widget);
            }
            None => self.split.set_end_child(Some(widget)),
        }
    }

    /// The folder a pane shows (left is [`Side::Local`]), if the pane is connected.
    pub fn pane_dir(&self, pane: Side) -> Option<SidePath> {
        match pane {
            Side::Local => Some(match &self.left_server {
                Some(left) => SidePath::Remote(left.dir.clone()),
                None => SidePath::Local(self.local_dir.clone()),
            }),
            Side::Remote => self
                .connection
                .as_ref()
                .map(|connection| SidePath::Remote(connection.dir.clone())),
        }
    }

    /// Turn a job made in pane terms (`side` is the pane its sources are in) into one
    /// the engine runs. Site to site, both panes are servers: the sources are server
    /// paths, [`Job::left_server`] says which server, and a copy or move between the panes
    /// goes from one server to the other. Elsewhere the job is already right.
    pub fn engine_job(&self, job: Job) -> Job {
        if self.left_server.is_some() {
            site_to_site_job(job)
        } else {
            job
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
        // Site to site: both servers, "left ⇄ right".
        if let Some(left) = &self.left_server {
            let right = self
                .connection
                .as_ref()
                .map_or(NOT_CONNECTED, |connection| connection.host.as_str());
            return format!("{} \u{21c4} {right}", left.host);
        }
        let (host, dir) = match &self.connection {
            Some(connection) => (
                connection.host.as_str(),
                SidePath::Remote(connection.dir.clone()),
            ),
            None => (host, SidePath::Local(self.local_dir.clone())),
        };
        format!("{host} - {}", folder_name(&dir))
    }

    /// Connection text for the status bar.
    pub fn status(&self) -> String {
        if let (Some(left), Some(right)) = (&self.left_server, &self.connection) {
            return format!("Connected to {} and {}", left.name, right.name);
        }
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
        let start_dir = remote.home().clone();
        let id = self.id;
        let pane = FilePane::builder()
            .launch(FilePaneInit {
                start_dir: SidePath::Remote(start_dir.clone()),
                last_dir_on_disk: BTreeMap::new(),
                remote: Some(remote.clone()),
                title: site.display_name(),
            })
            .forward(sender.input_sender(), move |output| {
                pane_message(id, Side::Remote, output)
            });
        pane.emit(FilePaneMsg::InUse(self.remote_in_use.clone()));
        ask_server_name(id, Side::Remote, remote.clone(), sender);
        // Each pane's menu names the other: "Download to this computer", "Upload to
        // <server>", site to site "Copy to <server>".
        pane.emit(FilePaneMsg::Peer(Some(match &self.left_server {
            Some(left) => Peer {
                name: left.host.clone(),
                is_server: true,
            },
            None => Peer {
                name: String::from("this computer"),
                is_server: false,
            },
        })));
        self.set_right(pane.widget());
        self.transfer_tabs
            .show_connection(Some(&site.display_name()));
        self.transfers
            .emit(TransferPanelMsg::Connection(Some(PanelConnection {
                site_id: site.id.clone(),
                remote: remote.clone(),
                name: remote.host().to_owned(),
                left: self.left_server.as_ref().map(|left| LeftConnection {
                    site_id: left.site_id.clone(),
                    remote: left.remote.clone(),
                    name: left.host.clone(),
                }),
            })));
        self.local_pane.emit(FilePaneMsg::Peer(Some(Peer {
            name: remote.host().to_owned(),
            is_server: true,
        })));
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
            self.set_right(&self.placeholder);
            self.transfer_tabs.show_connection(None);
            // Transfers hold their own handle on the session; the panel stops them so
            // the connection really closes.
            self.transfers.emit(TransferPanelMsg::Connection(None));
            self.local_pane.emit(FilePaneMsg::Peer(None));
            // Dropping the pane drops its session, which closes the connection.
            drop(connection);
        }
    }

    /// Show which files and folders running transfers use, in each pane.
    pub fn show_in_use(&mut self, local: Vec<SidePath>, remote: Vec<SidePath>) {
        self.local_pane.emit(FilePaneMsg::InUse(local));
        if let Some(connection) = &self.connection {
            connection.pane.emit(FilePaneMsg::InUse(remote.clone()));
        }
        self.remote_in_use = remote;
    }

    /// Re-read the folder one pane shows: a transfer changed files in it.
    pub fn refresh_side(&self, side: Side) {
        match (side, &self.connection) {
            (Side::Local, _) => self.local_pane.emit(FilePaneMsg::RefreshAfterChange),
            (Side::Remote, Some(connection)) => {
                connection.pane.emit(FilePaneMsg::RefreshAfterChange);
            }
            (Side::Remote, None) => {}
        }
    }

    /// Re-read the folders both panes show: `changed` after transfers changed files
    /// (read again even if a listing is on its way), otherwise to catch up (a listing on
    /// its way will do).
    pub fn refresh_panes(&self, changed: bool) {
        let message = || {
            if changed {
                FilePaneMsg::RefreshAfterChange
            } else {
                FilePaneMsg::RefreshIfIdle
            }
        };
        self.local_pane.emit(message());
        if let Some(connection) = &self.connection {
            connection.pane.emit(message());
        }
    }

    /// Show a server's own name in its pane's title bar (`pane`: left is
    /// [`Side::Local`]).
    pub fn name_server(&self, pane: Side, name: String) {
        match (pane, &self.connection) {
            (Side::Local, _) => self.local_pane.emit(FilePaneMsg::ServerName(name)),
            (Side::Remote, Some(connection)) => {
                connection.pane.emit(FilePaneMsg::ServerName(name));
            }
            (Side::Remote, None) => {}
        }
    }

    /// Resume transfers picked on the Login dialog's Unfinished tab, with each pane in
    /// the folder it showed for them. Call it right after connecting.
    pub fn resume(&self, resume: Resume) {
        for (pane, dir) in resume.folders {
            match (pane, &self.connection) {
                (Side::Local, _) => self.local_pane.emit(FilePaneMsg::GoTo(dir)),
                (Side::Remote, Some(connection)) => connection.pane.emit(FilePaneMsg::GoTo(dir)),
                (Side::Remote, None) => {}
            }
        }
        self.transfers
            .emit(TransferPanelMsg::ResumeChosen(resume.keys));
    }

    /// Show which ways site-to-site transfers are copying, as arrows by the picture of
    /// this computer.
    pub fn show_relaying(&self, to_right: bool, to_left: bool) {
        if let Some(relay) = &self.relay {
            relay.show_directions(to_right, to_left);
        }
    }

    /// Show the job counts on the transfer tabs.
    pub fn show_counts(&mut self, running: usize, paused: usize, completed: usize, failed: usize) {
        self.running = running;
        self.paused = paused;
        if let Some(relay) = &self.relay {
            relay.set_active(running > 0);
        }
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

/// The picture between a site-to-site session's panes: this computer, which files copied
/// between the two servers pass through, with two lights like a disk's that blink while
/// transfers run: green for receiving, amber for sending.
struct RelayPicture {
    /// The column: a title strip level with the panes' title bars, then the picture.
    root: gtk::Box,
    lights: gtk::Box,
    /// Above the computer, pointing right: files going from the left server to the right
    /// one. Below, pointing left: the other way.
    to_right: gtk::Image,
    to_left: gtk::Image,
}

impl RelayPicture {
    fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["relay"])
            .build();
        // The panes' title bars run across the top of this column too, so the right
        // pane's does not start out of nowhere.
        // Placed and sized like the panes' title bars (their 2 px margin, an icon and a
        // label), so their lines meet.
        let strip = gtk::Box::builder()
            .css_classes(["pane-title"])
            .margin_top(2)
            .build();
        let spacer = gtk::Image::from_icon_name("computer-symbolic");
        spacer.set_opacity(0.0);
        strip.append(&spacer);
        strip.append(&gtk::Label::new(Some(" ")));
        root.append(&strip);
        let picture = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .valign(gtk::Align::Center)
            .vexpand(true)
            .css_classes(["relay-picture"])
            .tooltip_text("Files copied between the two servers pass through this computer")
            .build();
        root.append(&picture);
        // The arrows keep their place while hidden (see `show_directions`), so nothing
        // moves when they appear.
        let arrow = |icon: &str, tooltip: &str| {
            let image = gtk::Image::from_icon_name(icon);
            image.add_css_class("relay-arrow");
            image.set_tooltip_text(Some(tooltip));
            image.set_opacity(0.0);
            image
        };
        let to_right = arrow(
            "go-next-symbolic",
            "Copying from the left server to the right one",
        );
        let to_left = arrow(
            "go-previous-symbolic",
            "Copying from the right server to the left one",
        );
        picture.append(&to_right);
        let computer = gtk::Image::from_icon_name("computer-symbolic");
        computer.set_pixel_size(32);
        picture.append(&computer);
        let lights = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .halign(gtk::Align::Center)
            .css_classes(["relay-lights"])
            .build();
        for (class, tooltip) in [
            ("receive", "Receiving from the server copies come from"),
            ("send", "Sending to the server copies go to"),
        ] {
            let light = gtk::Box::builder()
                .css_classes(["relay-light", class])
                .tooltip_text(tooltip)
                .build();
            lights.append(&light);
        }
        picture.append(&lights);
        picture.append(&to_left);
        Self {
            root,
            lights,
            to_right,
            to_left,
        }
    }

    /// Show the arrow of each way files are being copied.
    fn show_directions(&self, to_right: bool, to_left: bool) {
        self.to_right.set_opacity(if to_right { 1.0 } else { 0.0 });
        self.to_left.set_opacity(if to_left { 1.0 } else { 0.0 });
    }

    /// Blink the lights while transfers run.
    fn set_active(&self, active: bool) {
        set_class(&self.lights, "active", active);
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

/// A job made in pane terms in a site-to-site session, as the engine runs it: the
/// sources are on the server of the pane they came from, and a copy or move between the
/// panes goes from that server to the other one.
fn site_to_site_job(mut job: Job) -> Job {
    job.left_server = job.side == Side::Local;
    job.side = Side::Remote;
    job.between_servers = !job.same_side && job.kind != JobKind::Delete;
    job
}

/// Ask a server for its own name in the background; it arrives as
/// [`AppMsg::ServerNamed`] for its pane (left is [`Side::Local`]).
fn ask_server_name(session: SessionId, pane: Side, remote: Remote, sender: &ComponentSender<App>) {
    let sender = sender.clone();
    relm4::spawn(async move {
        if let Some(name) = remote.server_name().await {
            sender.input(AppMsg::ServerNamed {
                session,
                pane,
                name,
            });
        }
    });
}

/// The message a session's file pane output becomes.
fn pane_message(session: SessionId, side: Side, output: FilePaneOutput) -> AppMsg {
    match output {
        FilePaneOutput::DirectoryChanged { dir, disk_root } => match (dir, side) {
            (SidePath::Local(dir), _) => AppMsg::LocalDirChanged {
                session,
                dir,
                disk_root,
            },
            // Site to site, the left pane is a server too.
            (SidePath::Remote(dir), Side::Local) => AppMsg::LeftDirChanged { session, dir },
            (SidePath::Remote(dir), Side::Remote) => AppMsg::RemoteDirChanged { session, dir },
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
fn folder_name(dir: &SidePath) -> String {
    dir.file_name().map_or_else(
        || dir.to_string(),
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
        let local = |path: &str| SidePath::Local(PathBuf::from(path));
        assert_eq!(folder_name(&local("/home/me/Documents")), "Documents");
        assert_eq!(folder_name(&local("/")), "/");
        assert_eq!(folder_name(&SidePath::Remote(RemotePath::root())), "/");
    }

    /// A job as a pane makes it: `pane` holds the sources.
    fn pane_job(kind: JobKind, pane: Side, same_side: bool) -> Job {
        Job {
            kind,
            side: pane,
            sources: vec![SidePath::Remote(RemotePath::new("/a/file"))],
            dest_dir: SidePath::Remote(RemotePath::new("/b")),
            same_side,
            between_servers: false,
            left_server: false,
            resume: false,
        }
    }

    #[test]
    fn site_to_site_jobs_name_their_server_and_go_between_panes() {
        // Left pane to right pane: from the left server to the other one.
        let job = site_to_site_job(pane_job(JobKind::Copy, Side::Local, false));
        assert_eq!(job.side, Side::Remote);
        assert!(job.left_server && job.between_servers);
        assert_eq!(
            (job.source_pane(), job.dest_pane()),
            (Side::Local, Side::Remote)
        );

        // Right pane to left pane.
        let job = site_to_site_job(pane_job(JobKind::Move, Side::Remote, false));
        assert!(!job.left_server && job.between_servers);
        assert_eq!(
            (job.source_pane(), job.dest_pane()),
            (Side::Remote, Side::Local)
        );

        // A delete on the left server stays there.
        let job = site_to_site_job(pane_job(JobKind::Delete, Side::Local, false));
        assert!(job.left_server && !job.between_servers);
        assert_eq!(job.dest_pane(), Side::Local);

        // Dropped onto a folder in its own pane: stays on that server.
        let job = site_to_site_job(pane_job(JobKind::Copy, Side::Remote, true));
        assert!(!job.left_server && !job.between_servers);
        assert_eq!(job.dest_pane(), Side::Remote);
    }

    #[test]
    fn colours_repeat_after_the_palette() {
        assert_eq!(colour_class(0), "group-0");
        assert_eq!(colour_class(COLOURS + 1), "group-1");
    }
}
