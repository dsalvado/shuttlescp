//! Main window: menu bar on top, then one tab per session, each holding a local explorer
//! on the left and a remote explorer on the right.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use relm4::actions::{AccelsPlus, RelmAction, RelmActionGroup};
use relm4::gtk::prelude::*;
use relm4::gtk::{gio, glib};
use relm4::prelude::*;

use guiscp_core::config::{Config, ThemeChoice, WindowSize};
use guiscp_core::format::shorten_middle;
use guiscp_core::local_fs;
use guiscp_core::logging::{LogLevel, Logger};
use guiscp_core::pending::PendingTransfers;
use guiscp_core::remote::Remote;
use guiscp_core::sites::{Site, SiteBook};
use guiscp_core::transfer::{Job, JobKind, Side};

use crate::build_info::{build_info, show_about};
use crate::confirm_dialog::{self, Confirmation};
use crate::file_pane::{FilePane, FilePaneInit, FilePaneMsg, FilePaneOutput};
use crate::log_tail::log_tail;
use crate::login_dialog::{LoginDialog, LoginInit, LoginMsg, LoginOutput};
use crate::resize_grips;
use crate::theme;
use crate::theme_list::ThemeList;
use crate::transfer_panel::{
    PanelConnection, TransferPanel, TransferPanelMsg, TransferPanelOutput,
};

/// Window size the first time the app opens; after that, the size it was closed at.
const FIRST_WINDOW_SIZE: WindowSize = WindowSize {
    width: 1200,
    height: 720,
};
/// Menu model id linking the View menu's placeholder item to the theme list widget.
const THEME_LIST_ID: &str = "theme-list";
/// Signals that close the app gracefully, with the reason logged for each. The numbers
/// are the Linux values (see `man 7 signal`); GLib only accepts these few.
const QUIT_SIGNALS: [(i32, &str); 3] = [
    (1, "terminal closed (SIGHUP)"),
    (2, "Ctrl+C (SIGINT)"),
    (15, "stop request (SIGTERM)"),
];
/// Exit reason when the window is closed with its close button.
const WINDOW_CLOSED: &str = "window closed";
/// Exit reason for File > Quit and Ctrl+Q.
const QUIT_COMMAND: &str = "File > Quit";
/// Exit reason when the last session tab is closed with its X button.
const SESSION_TAB_CLOSED: &str = "session tab closed";
/// Longest session tab title, in characters; longer titles are cut in the middle.
const TAB_TITLE_MAX_CHARS: usize = 32;
/// Icon of a session tab.
const SESSION_TAB_ICON: &str = "computer-symbolic";
/// Icon of the "New Tab" tab, which will open a new connection.
const NEW_TAB_ICON: &str = "tab-new-symbolic";
/// Position of the "New Tab" tab, after the single session tab.
const NEW_TAB_PAGE: u32 = 1;
/// Initial height of the session area above the bottom panel, in pixels.
const SESSION_AREA_HEIGHT: i32 = 470;
/// Text shown in the remote half of the split (and the transfers tab) while not connected.
pub const NOT_CONNECTED: &str = "Not connected";
/// Position of the transfer Queue tab in the bottom panel (after the log).
const TRANSFERS_PAGE: u32 = 1;
/// How long the Completed tab stays highlighted after a transfer lands in it.
const TAB_HIGHLIGHT: std::time::Duration = std::time::Duration::from_millis(1500);

// These macros declare marker types that name GIO actions in a type-safe way.
// "win" is the action group GTK attaches to the window, so actions are "win.quit" etc.
relm4::new_action_group!(WindowActions, "win");
relm4::new_stateless_action!(QuitAction, WindowActions, "quit");
relm4::new_stateless_action!(LoginAction, WindowActions, "login");
relm4::new_stateless_action!(DisconnectAction, WindowActions, "disconnect");
relm4::new_stateless_action!(OpenLogAction, WindowActions, "open-log");
relm4::new_stateless_action!(AboutAction, WindowActions, "about");
// Target and state are both a `ThemeChoice::key()` string. A stateful action
// whose state matches the item's target is what makes GTK draw radio items in a menu.
relm4::new_stateful_action!(ThemeAction, WindowActions, "theme", String, String);
// Target and state are both a `LogLevel::as_str()` string.
relm4::new_stateful_action!(LogLevelAction, WindowActions, "log-level", String, String);

/// Everything `main` prepares before the window is built.
pub struct AppInit {
    /// Settings loaded from disk (or defaults).
    pub config: Config,
    /// Where the settings are saved.
    pub config_path: PathBuf,
    /// The installed logger, already running at `config.log_level()`.
    pub logger: Logger,
}

/// Messages handled by the main window.
#[derive(Debug)]
pub enum AppMsg {
    /// The user picked a theme from the View menu.
    ThemeSelected(ThemeChoice),
    /// The user picked a log level from the Tools menu.
    LogLevelSelected(LogLevel),
    /// The local pane moved to `dir`, on the disk mounted at `disk_root` (if known).
    LocalDirChanged {
        /// Directory now shown.
        dir: PathBuf,
        /// Mount point of the disk holding `dir`.
        disk_root: Option<PathBuf>,
    },
    /// Close the app; the text says why and is written to the log. Asks first while
    /// transfers are running (except when quitting on a signal).
    Quit(&'static str),
    /// Close the app without asking (the user agreed to stop the transfers).
    QuitNow(&'static str),
    /// Open the Login dialog.
    ShowLogin,
    /// The user changed whether the Login dialog opens at startup.
    ShowLoginAtStartup(bool),
    /// The Login dialog connected to a server.
    Connected {
        /// The site connected to.
        site: Site,
        /// The open SFTP session.
        remote: Remote,
    },
    /// The remote pane moved to another folder.
    RemoteDirChanged(PathBuf),
    /// Close the connection (File > Disconnect).
    Disconnect,
    /// The session tab's X was clicked: disconnect and close the session. It is the
    /// last (only) session tab, so the app exits. Asks first while transfers are
    /// running.
    CloseSession,
    /// Close the session without asking.
    CloseSessionNow,
    /// A file pane asked to copy, move or delete its selected items.
    PaneRequest {
        /// The pane's side.
        side: Side,
        /// What to do.
        kind: JobKind,
        /// The selected items.
        paths: Vec<PathBuf>,
    },
    /// Run a job (a delete, once confirmed).
    StartJob(Job),
    /// A transfer finished: show the panes' new contents.
    TransferFinished,
    /// The number of transfers running, completed or failed changed.
    TransferCounts {
        /// Jobs running.
        running: usize,
        /// Jobs in the Completed list.
        completed: usize,
        /// Jobs in the Failed list.
        failed: usize,
    },
    /// The main window was resized, maximized or restored.
    WindowChanged {
        /// The window's normal size; `None` while maximized (its size then is the
        /// screen's, not one to restore later).
        size: Option<WindowSize>,
        /// Whether the window is maximized.
        maximized: bool,
    },
}

/// The open connection shown in the right half of the session.
struct Connection {
    /// "user@host" of the site, for the status bar.
    name: String,
    /// Server host name, for the tab title.
    host: String,
    /// Folder shown in the remote pane.
    dir: PathBuf,
    /// The remote file pane; dropping it (and stopping the transfers, which hold the
    /// session too) closes the session.
    pane: Controller<FilePane>,
}

/// Root component of the application.
pub struct App {
    /// `Controller` owns a running child component and gives access to its root widget.
    local_pane: Controller<FilePane>,
    /// Directory shown in the local pane, for the tab title.
    local_dir: PathBuf,
    /// Name of the machine the session tab shows (this computer until remote sessions exist).
    host: String,
    config: Config,
    config_path: PathBuf,
    logger: Logger,
    /// Theme list inside the View menu.
    theme_list: ThemeList,
    /// Why the app is closing, written to the log's session-end banner.
    exit_reason: &'static str,
    /// The Login dialog (hidden until opened from File > Login).
    login: Controller<LoginDialog>,
    /// The current server connection, if any.
    connection: Option<Connection>,
    /// Left/right split of the session tab; the right half shows the remote pane.
    split: Option<gtk::Paned>,
    /// Shown in the right half while not connected.
    placeholder: gtk::Label,
    /// File > Disconnect, enabled only while connected.
    disconnect_action: RelmAction<DisconnectAction>,
    /// The connection's transfers tab in the bottom panel.
    transfer_tab: TransferTabs,
    /// The transfers list shown in that tab.
    transfers: Controller<TransferPanel>,
    /// The bottom panel, to bring the transfers tab forward when a job starts.
    bottom_panel: gtk::Notebook,
    /// The main window, for dialogs.
    window: gtk::ApplicationWindow,
    /// Transfers still running; shared with the window's close handler.
    running_transfers: Rc<Cell<usize>>,
    /// Set once quitting is decided, so closing the window no longer asks.
    quitting: Rc<Cell<bool>>,
}

#[relm4::component(pub)]
impl SimpleComponent for App {
    type Init = AppInit;
    type Input = AppMsg;
    type Output = ();

    view! {
        // `#[root]` marks the window as the component's root; the named widgets after it
        // are built alongside and used as tab labels.
        #[root]
        main_window = gtk::ApplicationWindow {
            set_title: Some("GuiSCP"),
            set_default_size: (window_size.width, window_size.height),
            set_maximized: window_maximized,
            set_resizable: true,
            // An empty title bar removes GTK's default one; the header bar below is part of
            // the content instead, so the corner resize grips can cover the top corners too.
            #[wrap(Some)]
            set_titlebar = &gtk::Box {},

            #[name = "overlay"]
            gtk::Overlay {
                #[wrap(Some)]
                set_child = &gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,

                    gtk::HeaderBar {},

                    #[name = "menu_bar"]
                    gtk::PopoverMenuBar::from_model(Some(&menu_model)) {},

                    // Sessions on top, the tabbed bottom panel (log, ...) below; the
                    // divider can be dragged.
                    gtk::Paned {
                        set_orientation: gtk::Orientation::Vertical,
                        set_vexpand: true,
                        set_position: SESSION_AREA_HEIGHT,
                        set_wide_handle: true,
                        // Resizing the window grows the sessions, not the panel.
                        set_resize_end_child: false,
                        set_shrink_end_child: false,
                        add_css_class: "pane-split",
                        set_end_child: Some(&bottom_panel),

                    #[wrap(Some)]
                    #[name = "sessions"]
                    set_start_child = &gtk::Notebook {
                        set_vexpand: true,
                        set_scrollable: true,

                        // `append_page[label] = page` calls `append_page(page, label)`.
                        append_page[Some(&session_tab)] = &gtk::Paned {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_position: window_size.width / 2,
                            set_wide_handle: true,
                            set_shrink_start_child: false,
                            set_shrink_end_child: false,
                            // Gives the divider a visible, hoverable handle (see style.css).
                            add_css_class: "pane-split",

                            // `#[wrap(Some)]` passes the widget as `Some(widget)`, since the
                            // setter takes an `Option` (GTK allows clearing a child with `None`).
                            #[wrap(Some)]
                            set_start_child = model.local_pane.widget(),

                            set_end_child: Some(&placeholder),
                        },

                        // Placeholder page; its tab is disabled until new connections exist.
                        append_page[Some(&new_tab)] = &gtk::Box {},
                    },
                    },

                    // Status bar along the bottom of the window.
                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 12,
                        add_css_class: "statusbar",

                        gtk::Label {
                            set_hexpand: true,
                            set_xalign: 0.0,
                            #[watch]
                            set_label: &model.connection_status(),
                        },
                        gtk::Label {
                            #[watch]
                            set_label: &format!("Log level: {}", model.logger.level().label()),
                        },
                        gtk::Separator {
                            set_orientation: gtk::Orientation::Vertical,
                        },
                        // Build info; full details on hover, the About dialog on click.
                        #[name = "build_label"]
                        gtk::Label {
                            set_label: &build_info().summary(),
                            set_tooltip_text: Some(&crate::build_info::details(
                                model.logger.file_path(),
                            )),
                            set_cursor_from_name: Some("pointer"),
                        },
                    },
                },
            },
        },

        // Tab label of the session: "<host> - <folder>", shortened, full text on hover.
        session_tab = gtk::Box {
            set_spacing: 6,
            #[watch]
            set_tooltip_text: Some(&model.tab_title()),

            gtk::Image {
                set_icon_name: Some(SESSION_TAB_ICON),
            },
            gtk::Label {
                #[watch]
                set_label: &shorten_middle(&model.tab_title(), TAB_TITLE_MAX_CHARS),
            },
            // Close button, shown while connected: disconnects and closes the session.
            gtk::Button {
                set_icon_name: "window-close-symbolic",
                set_has_frame: false,
                add_css_class: "tab-close",
                set_tooltip_text: Some("Disconnect and close this session"),
                #[watch]
                set_visible: model.connection.is_some(),
                connect_clicked => AppMsg::CloseSession,
            },
        },

        // Tab label of the "New Tab" placeholder, greyed out while disabled.
        new_tab = gtk::Box {
            set_spacing: 6,
            set_sensitive: false,

            gtk::Image {
                set_icon_name: Some(NEW_TAB_ICON),
            },
            gtk::Label {
                set_label: "New Tab",
            },
        },
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Destructuring moves each field out of `init` into its own variable.
        let AppInit {
            mut config,
            config_path,
            logger,
        } = init;
        let themes = theme::available_themes(&theme::search_dirs());
        tracing::debug!("found {} themes", themes.len());

        // A saved theme that has since been uninstalled falls back to the system theme.
        if let ThemeChoice::Named(name) = config.theme()
            && !themes.contains(name)
        {
            tracing::warn!("saved theme {name} is no longer installed; using system theme");
            config.set_theme(ThemeChoice::System);
        }
        theme::apply(config.theme());
        theme::ensure_cursor_theme();

        let disconnect_action = register_actions(&root, &config, logger.file_path(), &sender);
        let placeholder = gtk::Label::new(Some(NOT_CONNECTED));
        placeholder.add_css_class("dim-label");
        let transfers = TransferPanel::builder()
            .launch(PendingTransfers::default_path())
            .forward(sender.input_sender(), |output| match output {
                TransferPanelOutput::JobFinished => AppMsg::TransferFinished,
                TransferPanelOutput::Counts {
                    running,
                    completed,
                    failed,
                } => AppMsg::TransferCounts {
                    running,
                    completed,
                    failed,
                },
            });
        let running_transfers = Rc::new(Cell::new(0));
        let quitting = Rc::new(Cell::new(false));
        {
            // Closing the window while transfers run asks first (see `AppMsg::Quit`).
            let (running, quitting, sender) = (
                Rc::clone(&running_transfers),
                Rc::clone(&quitting),
                sender.clone(),
            );
            root.connect_close_request(move |_| {
                if running.get() > 0 && !quitting.get() {
                    sender.input(AppMsg::Quit(WINDOW_CLOSED));
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
        }
        let (completed_page, failed_page) = transfers.model().history_pages();
        let (bottom_panel, transfer_tab) = bottom_panel(
            logger.file_path().map(Path::to_path_buf),
            [
                transfers.widget().upcast_ref(),
                &completed_page,
                &failed_page,
            ],
        );
        let menu_model = build_menu();
        let theme_list = ThemeList::new(themes, config.theme());
        quit_on_signals(&sender);

        let start_dir = config
            .last_local_dir()
            .and_then(local_fs::nearest_listable_dir)
            .map(PathBuf::from)
            .or_else(std::env::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        tracing::info!("opening local pane at {}", start_dir.display());
        let show_login = config.show_login_at_startup();
        let window_size = config
            .window_size()
            .unwrap_or(FIRST_WINDOW_SIZE)
            .at_least_min();
        let window_maximized = config.window_maximized();
        let mut model = Self {
            // `.forward()` converts each pane output into a message for this component.
            local_pane: FilePane::builder()
                .launch(FilePaneInit {
                    start_dir: start_dir.clone(),
                    last_dir_on_disk: config.last_dir_on_disk().clone(),
                    remote: None,
                })
                .forward(sender.input_sender(), |output| match output {
                    FilePaneOutput::DirectoryChanged { dir, disk_root } => {
                        AppMsg::LocalDirChanged { dir, disk_root }
                    }
                    FilePaneOutput::Request { kind, paths } => AppMsg::PaneRequest {
                        side: Side::Local,
                        kind,
                        paths,
                    },
                }),
            local_dir: start_dir,
            host: glib::host_name().into(),
            config,
            config_path,
            logger,
            theme_list,
            exit_reason: WINDOW_CLOSED,
            // `transient_for` keeps the dialog above the main window and centred on it.
            login: LoginDialog::builder()
                .transient_for(&root)
                .launch(LoginInit {
                    sites_path: SiteBook::default_path(),
                    show_at_startup: show_login,
                })
                .forward(sender.input_sender(), |output| match output {
                    LoginOutput::ShowAtStartupChanged(show) => AppMsg::ShowLoginAtStartup(show),
                    LoginOutput::Connected { site, remote } => AppMsg::Connected { site, remote },
                }),
            connection: None,
            split: None,
            placeholder: placeholder.clone(),
            disconnect_action,
            transfer_tab,
            transfers,
            bottom_panel: bottom_panel.clone(),
            window: root.clone(),
            running_transfers,
            quitting,
        };
        let widgets = view_output!();
        // The session's left/right split is the notebook's first page.
        model.split = widgets.sessions.nth_page(Some(0)).and_downcast();
        widgets
            .menu_bar
            .add_child(model.theme_list.widget(), THEME_LIST_ID);
        resize_grips::add_corner_grips(&root, &widgets.overlay);
        track_window_size(&root, &sender);
        let about_click = gtk::GestureClick::new();
        let about_window = root.clone();
        about_click.connect_released(move |_, _, _, _| {
            let _ = WidgetExt::activate_action(&about_window, "win.about", None);
        });
        widgets.build_label.add_controller(about_click);
        // Keep the "New Tab" tab from being opened (by click or keyboard) for now. Our
        // handler runs before GTK's default one, which performs the switch, so stopping
        // the emission here cancels it.
        widgets
            .sessions
            .connect_switch_page(|notebook, _page, page_num| {
                if page_num == NEW_TAB_PAGE {
                    notebook.stop_signal_emission_by_name("switch-page");
                }
            });
        if show_login {
            // Wait until the main window is on screen, so the dialog opens on top of it.
            // "map" can fire again if the window is re-shown, so only act the first time.
            let opened = std::cell::Cell::new(false);
            root.connect_map(move |_| {
                if !opened.replace(true) {
                    sender.input(AppMsg::ShowLogin);
                }
            });
        }
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            AppMsg::ThemeSelected(choice) => {
                if &choice == self.config.theme() {
                    return;
                }
                tracing::info!("theme changed to {choice:?}");
                theme::apply(&choice);
                self.theme_list.show_current(&choice);
                self.config.set_theme(choice);
                self.save_config();
            }
            AppMsg::ShowLogin => self.login.emit(LoginMsg::Show),
            AppMsg::Connected { site, remote } => self.show_remote(site, remote, &sender),
            AppMsg::RemoteDirChanged(dir) => {
                if let Some(connection) = &mut self.connection {
                    tracing::debug!("remote pane now shows {}", dir.display());
                    connection.dir = dir;
                }
            }
            AppMsg::Disconnect => self.disconnect(),
            AppMsg::CloseSession if self.running_transfers.get() > 0 => {
                self.confirm_stopping_transfers(
                    "Close Session",
                    "Close the session?",
                    "Close Session",
                    &sender,
                    || AppMsg::CloseSessionNow,
                );
            }
            AppMsg::CloseSession | AppMsg::CloseSessionNow => {
                self.disconnect();
                // There is only one session tab today, so closing it leaves none: exit,
                // as WinSCP does when its last session closes.
                self.quit(SESSION_TAB_CLOSED);
            }
            AppMsg::ShowLoginAtStartup(show) => {
                tracing::info!("show Login dialog at startup: {show}");
                self.config.set_show_login_at_startup(show);
                self.save_config();
            }
            AppMsg::Quit(reason)
                if self.running_transfers.get() > 0
                    && !QUIT_SIGNALS.iter().any(|&(_, signal)| signal == reason) =>
            {
                self.confirm_stopping_transfers(
                    "Quit",
                    "Quit GuiSCP?",
                    "Quit",
                    &sender,
                    move || AppMsg::QuitNow(reason),
                );
            }
            AppMsg::Quit(reason) | AppMsg::QuitNow(reason) => self.quit(reason),
            AppMsg::LogLevelSelected(level) => {
                self.logger.set_level(level);
                self.config.set_log_level(self.logger.level());
                self.save_config();
            }
            AppMsg::PaneRequest { side, kind, paths } => {
                self.request_job(side, kind, paths, &sender);
            }
            AppMsg::StartJob(job) => self.start_job(job),
            AppMsg::TransferCounts {
                running,
                completed,
                failed,
            } => {
                self.running_transfers.set(running);
                self.transfer_tab.show_counts(running, completed, failed);
            }
            AppMsg::TransferFinished => {
                self.local_pane.emit(FilePaneMsg::Refresh);
                if let Some(connection) = &self.connection {
                    connection.pane.emit(FilePaneMsg::Refresh);
                }
            }
            AppMsg::WindowChanged { size, maximized } => {
                // Only kept in memory here; written to disk once, in `shutdown`.
                if let Some(size) = size {
                    self.config.set_window_size(size);
                }
                self.config.set_window_maximized(maximized);
            }
            AppMsg::LocalDirChanged { dir, disk_root } => {
                tracing::debug!("local pane now shows {}", dir.display());
                // Only kept in memory here; written to disk once, in `shutdown`.
                if let Some(root) = disk_root {
                    self.config.set_last_dir_on_disk(root, dir.clone());
                }
                self.config.set_last_local_dir(dir.clone());
                self.local_dir = dir;
            }
        }
    }

    /// Runs when the application quits (window closed, File > Quit or a quit signal).
    fn shutdown(&mut self, _widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        if let Some(size) = self.config.window_size() {
            tracing::debug!(
                "window size {}x{}, maximized: {}",
                size.width,
                size.height,
                self.config.window_maximized()
            );
        }
        self.save_config();
        self.logger.end_session(self.exit_reason);
    }
}

impl App {
    /// Full session tab title: `<server> - <folder>`, for the remote server while
    /// connected, otherwise for this computer and the local pane's folder.
    fn tab_title(&self) -> String {
        let (host, dir) = match &self.connection {
            Some(connection) => (connection.host.as_str(), connection.dir.as_path()),
            None => (self.host.as_str(), self.local_dir.as_path()),
        };
        // The root directory has no file name, so it is shown as its full path ("/").
        let folder = dir
            .file_name()
            .map_or_else(|| dir.to_string_lossy(), |name| name.to_string_lossy());
        format!("{host} - {folder}")
    }

    /// Close the current connection, if any, and show "Not connected" again.
    fn disconnect(&mut self) {
        if let Some(connection) = self.connection.take() {
            tracing::info!("disconnected from {}", connection.name);
            if let Some(split) = &self.split {
                split.set_end_child(Some(&self.placeholder));
            }
            self.disconnect_action.set_enabled(false);
            self.transfer_tab.show_connection(None);
            // Transfers hold their own handle on the session; the panel stops them so
            // the connection really closes. They can be resumed on the next connection.
            self.transfers.emit(TransferPanelMsg::Connection(None));
            self.local_pane.emit(FilePaneMsg::PeerAvailable(false));
            // Dropping the pane drops its session, which closes the connection.
            drop(connection);
        }
    }

    /// Connection text for the status bar.
    fn connection_status(&self) -> String {
        self.connection.as_ref().map_or_else(
            || String::from(NOT_CONNECTED),
            |c| format!("Connected to {}", c.name),
        )
    }

    /// Show a newly connected server in the right half of the session, replacing any
    /// previous connection.
    ///
    /// # Arguments
    ///
    /// * `site` - The site connected to
    /// * `remote` - Its open SFTP session
    /// * `sender` - Receives the remote pane's folder changes
    fn show_remote(&mut self, site: Site, remote: Remote, sender: &ComponentSender<Self>) {
        let start_dir = remote.home().to_path_buf();
        let pane = FilePane::builder()
            .launch(FilePaneInit {
                start_dir: start_dir.clone(),
                last_dir_on_disk: std::collections::BTreeMap::new(),
                remote: Some(remote.clone()),
            })
            .forward(sender.input_sender(), |output| match output {
                FilePaneOutput::DirectoryChanged { dir, .. } => AppMsg::RemoteDirChanged(dir),
                FilePaneOutput::Request { kind, paths } => AppMsg::PaneRequest {
                    side: Side::Remote,
                    kind,
                    paths,
                },
            });
        if let Some(split) = &self.split {
            split.set_end_child(Some(pane.widget()));
        }
        self.disconnect_action.set_enabled(true);
        self.transfer_tab
            .show_connection(Some(&site.display_name()));
        self.transfers
            .emit(TransferPanelMsg::Connection(Some(PanelConnection {
                site_id: site.id.clone(),
                remote: remote.clone(),
            })));
        self.local_pane.emit(FilePaneMsg::PeerAvailable(true));
        self.connection = Some(Connection {
            name: site.display_name(),
            host: remote.host().to_owned(),
            dir: start_dir,
            pane,
        });
    }

    /// Close the app.
    fn quit(&mut self, reason: &'static str) {
        tracing::info!("closing: {reason}");
        self.exit_reason = reason;
        self.quitting.set(true);
        relm4::main_application().quit();
    }

    /// Ask whether to stop the running transfers, and send `on_yes`'s message if so.
    ///
    /// # Arguments
    ///
    /// * `title` - Window title
    /// * `question` - What is about to happen, e.g. "Quit GuiSCP?"
    /// * `confirm_label` - Label of the button that goes ahead
    /// * `sender` - Receives the message
    /// * `on_yes` - Makes the message to send when the user goes ahead
    fn confirm_stopping_transfers(
        &self,
        title: &str,
        question: &str,
        confirm_label: &str,
        sender: &ComponentSender<Self>,
        on_yes: impl Fn() -> AppMsg + 'static,
    ) {
        let detail = match self.running_transfers.get() {
            1 => String::from(
                "A transfer is still running. It will stop, and can be resumed the next \
                 time you connect to this server.",
            ),
            count => format!(
                "{count} transfers are still running. They will stop, and can be resumed \
                 the next time you connect to this server."
            ),
        };
        let sender = sender.clone();
        confirm_dialog::ask(
            self.window.upcast_ref(),
            Confirmation {
                title,
                question,
                detail: &detail,
                confirm_label,
            },
            move |confirmed| {
                if confirmed {
                    sender.input(on_yes());
                }
            },
        );
    }

    /// Turn a pane's request into a job: copies go to the folder the other pane shows;
    /// deletes are confirmed first.
    ///
    /// # Arguments
    ///
    /// * `side` - The pane the items are in
    /// * `kind` - Copy, move or delete
    /// * `paths` - The selected items
    /// * `sender` - Receives the confirmed delete
    fn request_job(
        &self,
        side: Side,
        kind: JobKind,
        paths: Vec<PathBuf>,
        sender: &ComponentSender<Self>,
    ) {
        let dest_dir = match side.other() {
            Side::Local => self.local_dir.clone(),
            Side::Remote => match &self.connection {
                Some(connection) => connection.dir.clone(),
                None => return,
            },
        };
        let job = Job {
            kind,
            side,
            sources: paths,
            dest_dir,
            resume: false,
        };
        if kind != JobKind::Delete {
            self.start_job(job);
            return;
        }
        let question = format!("{}?", job.title());
        let sender = sender.clone();
        confirm_dialog::ask(
            self.window.upcast_ref(),
            Confirmation {
                title: "Delete",
                question: &question,
                detail: "Folders are deleted with everything in them. This cannot be undone.",
                confirm_label: "Delete",
            },
            move |confirmed| {
                if confirmed {
                    sender.input(AppMsg::StartJob(job.clone()));
                }
            },
        );
    }

    /// Run a job and bring the transfers tab forward to show it.
    fn start_job(&self, job: Job) {
        tracing::info!("starting: {}", job.title());
        self.bottom_panel.set_current_page(Some(TRANSFERS_PAGE));
        self.transfers.emit(TransferPanelMsg::Start(job));
    }

    /// Write the settings file, logging instead of failing: losing a preference should
    /// never stop the app.
    fn save_config(&self) {
        match self.config.save(&self.config_path) {
            Ok(()) => tracing::debug!("saved settings to {}", self.config_path.display()),
            Err(error) => tracing::error!("cannot save settings: {error}"),
        }
    }
}

/// Create the window's actions and attach them to it.
///
/// # Arguments
///
/// * `window` - Window that owns the "win" action group
/// * `config` - Current settings, used for the initial radio selections
/// * `log_path` - Log file opened by Tools > Open Log File; `None` disables that item
/// * `sender` - Sender used to forward menu choices to [`App::update`]
///
/// # Returns
///
/// The File > Disconnect action, so it can be enabled while connected
fn register_actions(
    window: &gtk::ApplicationWindow,
    config: &Config,
    log_path: Option<&Path>,
    sender: &ComponentSender<App>,
) -> RelmAction<DisconnectAction> {
    let disconnect_sender = sender.clone();
    let disconnect = RelmAction::<DisconnectAction>::new_stateless(move |_| {
        disconnect_sender.input(AppMsg::Disconnect);
    });
    // Nothing to disconnect from yet.
    disconnect.set_enabled(false);

    let login_sender = sender.clone();
    let login = RelmAction::<LoginAction>::new_stateless(move |_| {
        login_sender.input(AppMsg::ShowLogin);
    });

    let quit_sender = sender.clone();
    let quit = RelmAction::<QuitAction>::new_stateless(move |_| {
        quit_sender.input(AppMsg::Quit(QUIT_COMMAND));
    });

    let log_uri = log_path.map(|path| gio::File::for_path(path).uri());
    let open_log = RelmAction::<OpenLogAction>::new_stateless(move |_| {
        if let Some(uri) = &log_uri
            && let Err(error) =
                gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>)
        {
            tracing::error!("cannot open log file {uri}: {error}");
        }
    });
    open_log.set_enabled(log_path.is_some());

    // A weak reference: the window owns this action, so a strong one would keep the
    // window alive forever.
    let about_window = window.downgrade();
    let about_log_path = log_path.map(Path::to_path_buf);
    let about = RelmAction::<AboutAction>::new_stateless(move |_| {
        if let Some(window) = about_window.upgrade() {
            show_about(window.upcast_ref(), about_log_path.as_deref());
        }
    });

    // Closures that outlive this function need their own copy of the sender.
    let theme_sender = sender.clone();
    let theme_action = RelmAction::<ThemeAction>::new_stateful_with_target_value(
        &config.theme().key().to_owned(),
        move |_, state, target| {
            theme_sender.input(AppMsg::ThemeSelected(ThemeChoice::from_key(&target)));
            *state = target;
        },
    );

    let level_sender = sender.clone();
    let level_action = RelmAction::<LogLevelAction>::new_stateful_with_target_value(
        &config.log_level().as_str().to_owned(),
        move |_, state, target| {
            if let Some(level) = LogLevel::from_name(&target) {
                level_sender.input(AppMsg::LogLevelSelected(level));
                *state = target;
            }
        },
    );

    let mut group = RelmActionGroup::<WindowActions>::new();
    group.add_action(login);
    group.add_action(disconnect.clone());
    group.add_action(quit);
    group.add_action(open_log);
    group.add_action(about);
    group.add_action(theme_action);
    group.add_action(level_action);
    group.register_for_widget(window);
    let app = relm4::main_application();
    app.set_accelerators_for_action::<LoginAction>(&["<primary>l"]);
    app.set_accelerators_for_action::<QuitAction>(&["<primary>q"]);
    disconnect
}

/// Report the window's size and maximized state to the app whenever they change, so
/// they can be saved when it closes.
///
/// # Arguments
///
/// * `window` - The main window
/// * `sender` - Receives [`AppMsg::WindowChanged`]
fn track_window_size(window: &gtk::ApplicationWindow, sender: &ComponentSender<App>) {
    let report = {
        let sender = sender.clone();
        move |window: &gtk::ApplicationWindow| {
            let maximized = window.is_maximized();
            // GTK keeps the default size in step with the user's resizing, and leaves
            // it at the normal size while maximized.
            let (width, height) = window.default_size();
            let size = (!maximized).then_some(WindowSize { width, height });
            sender.input(AppMsg::WindowChanged { size, maximized });
        }
    };
    let on_width = report.clone();
    window.connect_default_width_notify(move |window| on_width(window));
    let on_height = report.clone();
    window.connect_default_height_notify(move |window| on_height(window));
    window.connect_maximized_notify(move |window| report(window));
}

/// The tabbed panel along the bottom of the window: the log, then the transfer Queue,
/// Completed and Failed lists (one set per connection once several connections are
/// supported, each added after the log).
///
/// # Arguments
///
/// * `log_path` - The log file, if logging to a file works
/// * `pages` - The Queue, Completed and Failed pages
fn bottom_panel(
    log_path: Option<PathBuf>,
    pages: [&gtk::Widget; 3],
) -> (gtk::Notebook, TransferTabs) {
    let panel = gtk::Notebook::new();
    panel.add_css_class("bottom-panel");

    let (log_tab, _) = panel_tab("text-x-generic-symbolic", "Log");
    panel.append_page(&log_tail(log_path), Some(&log_tab));

    let [queue_page, completed_page, failed_page] = pages;
    let (queue_tab, queue) = panel_tab("network-transmit-receive-symbolic", "");
    let queue_position = panel.append_page(queue_page, Some(&queue_tab));
    let (completed_tab, completed) = panel_tab("object-select-symbolic", "");
    panel.append_page(completed_page, Some(&completed_tab));
    let (failed_tab, failed) = panel_tab("dialog-error-symbolic", "");
    panel.append_page(failed_page, Some(&failed_tab));
    let tabs = TransferTabs {
        queue,
        completed,
        completed_tab,
        failed,
        connection: RefCell::new(None),
        counts: Cell::new((0, 0, 0)),
    };
    tabs.show_connection(None);
    tabs.show_counts(0, 0, 0);

    // Selected by default: the transfer queue.
    panel.set_current_page(Some(queue_position));
    (panel, tabs)
}

/// A bottom panel tab label: a small icon and a name.
///
/// # Arguments
///
/// * `icon` - Theme icon name
/// * `name` - Tab name
///
/// # Returns
///
/// The label widget for the tab, and its text label (to rename the tab later)
fn panel_tab(icon: &str, name: &str) -> (gtk::Box, gtk::Label) {
    let label = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let text = gtk::Label::new(Some(name));
    label.append(&gtk::Image::from_icon_name(icon));
    label.append(&text);
    (label, text)
}

/// The titles of the transfer tabs (Queue, Completed, Failed); the tabs themselves
/// show a [`TransferPanel`]'s lists. With several connections, each will get a set.
struct TransferTabs {
    queue: gtk::Label,
    completed: gtk::Label,
    /// The Completed tab's label box, highlighted when a transfer lands in it.
    completed_tab: gtk::Box,
    failed: gtk::Label,
    /// Name of the connection the Queue belongs to.
    connection: RefCell<Option<String>>,
    /// Running, completed and failed jobs.
    counts: Cell<(usize, usize, usize)>,
}

impl TransferTabs {
    /// Name the Queue after its connection, or none.
    ///
    /// # Arguments
    ///
    /// * `connection` - Name of the connection (e.g. "me@host"), or `None`
    fn show_connection(&self, connection: Option<&str>) {
        self.queue.set_tooltip_text(Some(&match connection {
            Some(name) => format!("Transfers for {name}"),
            None => String::from("Transfers (not connected)"),
        }));
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

/// "Name" or "Name (n)" when there are any.
fn titled(name: &str, count: usize) -> String {
    if count == 0 {
        name.to_owned()
    } else {
        format!("{name} ({count})")
    }
}

/// Close the app gracefully on SIGHUP, SIGINT (Ctrl+C) and SIGTERM, so settings are saved
/// and the log gets its session-end banner. SIGKILL cannot be caught by any program.
///
/// # Arguments
///
/// * `sender` - Sender used to ask [`App::update`] to quit
fn quit_on_signals(sender: &ComponentSender<App>) {
    for (signal, reason) in QUIT_SIGNALS {
        let sender = sender.clone();
        // Runs on the GTK main loop, so it is safe to touch the app from here.
        glib_unix::unix_signal_add_local(signal, move || {
            sender.input(AppMsg::Quit(reason));
            glib::ControlFlow::Continue
        });
    }
}

/// Build the menu bar model: File, View (themes), Tools (logging) and Help (About).
///
/// Lists sit in sections rather than nested submenus, so every menu opens as a single
/// popup. The theme section holds one placeholder item that the [`ThemeList`] widget
/// replaces (see [`THEME_LIST_ID`]).
///
/// # Returns
///
/// The menu model for a [`gtk::PopoverMenuBar`]
fn build_menu() -> gio::Menu {
    let file_menu = gio::Menu::new();
    let session_section = gio::Menu::new();
    session_section.append_item(&RelmAction::<LoginAction>::to_menu_item("_Login…"));
    session_section.append_item(&RelmAction::<DisconnectAction>::to_menu_item("_Disconnect"));
    let quit_section = gio::Menu::new();
    quit_section.append_item(&RelmAction::<QuitAction>::to_menu_item("_Quit"));
    file_menu.append_section(None, &session_section);
    file_menu.append_section(None, &quit_section);

    let theme_section = gio::Menu::new();
    let theme_list_item = gio::MenuItem::new(None, None);
    theme_list_item.set_attribute_value("custom", Some(&THEME_LIST_ID.to_variant()));
    theme_section.append_item(&theme_list_item);
    let view_menu = gio::Menu::new();
    view_menu.append_section(Some("Theme"), &theme_section);

    let level_section = gio::Menu::new();
    for level in LogLevel::ALL {
        level_section.append_item(
            &RelmAction::<LogLevelAction>::to_menu_item_with_target_value(
                level.label(),
                &level.as_str().to_owned(),
            ),
        );
    }
    let log_file_section = gio::Menu::new();
    log_file_section.append_item(&RelmAction::<OpenLogAction>::to_menu_item("_Open Log File"));
    let tools_menu = gio::Menu::new();
    tools_menu.append_section(Some("Log Level"), &level_section);
    tools_menu.append_section(None, &log_file_section);

    let help_menu = gio::Menu::new();
    help_menu.append_item(&RelmAction::<AboutAction>::to_menu_item("_About GuiSCP"));

    let menu_bar = gio::Menu::new();
    menu_bar.append_submenu(Some("_File"), &file_menu);
    menu_bar.append_submenu(Some("_View"), &view_menu);
    menu_bar.append_submenu(Some("_Tools"), &tools_menu);
    menu_bar.append_submenu(Some("_Help"), &help_menu);
    menu_bar
}
