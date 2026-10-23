//! Main window: menu bar on top, then one tab per session (see [`crate::session`]),
//! each holding a local explorer on the left and a remote explorer on the right, and
//! the bottom panel with the log and each session's transfer lists. The last tab,
//! "New Connection", opens the Login dialog for another connection.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use relm4::actions::{ActionGroupName, ActionName, RelmAction, RelmActionGroup};
use relm4::gtk::prelude::*;
use relm4::gtk::{gio, glib};
use relm4::prelude::*;

use shuttlescp_core::config::{Config, ThemeChoice, WindowSize};
use shuttlescp_core::local_fs;
use shuttlescp_core::logging::{LogLevel, Logger};
use shuttlescp_core::path::{RemotePath, SidePath};
use shuttlescp_core::pending::PendingTransfers;
use shuttlescp_core::remote::Remote;
use shuttlescp_core::sites::{Site, SiteBook};
use shuttlescp_core::transfer::{Job, JobKind, Side};
use shuttlescp_core::update::{self, Manifest};

use crate::build_info::{build_info, show_about};
use crate::confirm_dialog::{self, Confirmation};
use crate::dialog;
use crate::log_tail::log_tail;
use crate::login_dialog::{LoginDialog, LoginInit, LoginMsg, LoginOutput};
use crate::quit_dialog::{self, QuitChoice};
use crate::resize_grips;
use crate::session::{self, Session, SessionId, SessionInit};
use crate::theme;
use crate::theme_list::ThemeList;
use crate::transfer_panel::{PendingStore, TransferPanelMsg};
use crate::update_dialog::{self, RestartChoice};

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
/// Exit reason when the app restarts into a newly installed version.
const RESTARTING_FOR_UPDATE: &str = "restarting into the new version";
/// Look for a newer release at startup at most this often.
const UPDATE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
/// Wait this long after startup before looking for a newer release, so it never slows
/// the start down.
const UPDATE_CHECK_DELAY: std::time::Duration = std::time::Duration::from_secs(5);
/// Longest wait, when quitting, for the sessions to delete their unfinished transfers'
/// partial files.
const DISCARD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// How long the transfers must have stopped before an update waiting for them restarts
/// the app.
const IDLE_RESTART_DELAY: std::time::Duration = std::time::Duration::from_secs(3);
/// Icon of the "New Connection" tab, which opens the Login dialog for another connection.
const NEW_TAB_ICON: &str = "tab-new-symbolic";
/// Initial height of the session area above the bottom panel, in pixels.
const SESSION_AREA_HEIGHT: i32 = 470;
/// Text shown in the remote half of the split (and the transfers tab) while not connected.
pub const NOT_CONNECTED: &str = "Not connected";
/// Pause between the main window's first frame and opening the Login dialog.
const LOGIN_AFTER_FIRST_FRAME: std::time::Duration = std::time::Duration::from_millis(250);

// These macros declare marker types that name GIO actions in a type-safe way.
// "win" is the action group GTK attaches to the window, so actions are "win.quit" etc.
relm4::new_action_group!(WindowActions, "win");
relm4::new_stateless_action!(QuitAction, WindowActions, "quit");
relm4::new_stateless_action!(LoginAction, WindowActions, "login");
relm4::new_stateless_action!(DisconnectAction, WindowActions, "disconnect");
relm4::new_stateless_action!(OpenLogAction, WindowActions, "open-log");
relm4::new_stateless_action!(AboutAction, WindowActions, "about");
relm4::new_stateless_action!(CheckUpdatesAction, WindowActions, "check-updates");
// A check item: its state says whether updates are looked for at startup.
relm4::new_stateful_action!(AutoUpdateAction, WindowActions, "auto-update", (), bool);
// Target and state are both a `ThemeChoice::key()` string. A stateful action
// whose state matches the item's target is what makes GTK draw radio items in a menu.
relm4::new_stateful_action!(ThemeAction, WindowActions, "theme", String, String);
// Target and state are both a `LogLevel::as_str()` string.
relm4::new_stateful_action!(LogLevelAction, WindowActions, "log-level", String, String);

/// Where the menus are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuBar {
    /// GTK's menu bar inside the window, under the title bar.
    InWindow,
    /// The macOS menu bar at the top of the screen. GTK adds the app menu in front of
    /// it (About, Hide, Quit, ...), which calls the "app.about" and "app.quit" actions.
    System,
}

impl MenuBar {
    /// The menu bar of the system the app runs on.
    fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::System
        } else {
            Self::InWindow
        }
    }

    /// The action group its items name. The system menu bar only reaches a window's
    /// actions while that window is the active one, so there the window's actions are
    /// also added to the application (see [`register_actions`]).
    fn group(self) -> &'static str {
        match self {
            Self::InWindow => WindowActions::NAME,
            Self::System => "app",
        }
    }

    /// A menu item for an action.
    ///
    /// # Arguments
    ///
    /// * `label` - The item's text
    fn item<A: ActionName>(self, label: &str) -> gio::MenuItem {
        gio::MenuItem::new(Some(label), Some(&format!("{}.{}", self.group(), A::NAME)))
    }

    /// A radio item: an action's item that sends `target` (see [`ThemeAction`]).
    ///
    /// # Arguments
    ///
    /// * `label` - The item's text
    /// * `target` - The value the item stands for
    fn radio_item<A: ActionName<Target = String>>(
        self,
        label: &str,
        target: &str,
    ) -> gio::MenuItem {
        let item = self.item::<A>(label);
        item.set_action_and_target_value(
            Some(&format!("{}.{}", self.group(), A::NAME)),
            Some(&target.to_variant()),
        );
        item
    }
}

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
    /// A session's local pane moved to `dir`, on the disk mounted at `disk_root` (if
    /// known).
    LocalDirChanged {
        session: SessionId,
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
    /// The Login dialog connected to a server: shown in the current session if it is
    /// not connected yet, otherwise in a new session tab.
    Connected {
        /// The site connected to.
        site: Site,
        /// The open SFTP session.
        remote: Remote,
    },
    /// The Login dialog connected to two sites, for copying between them.
    ConnectedPair {
        /// The left pane's site and connection.
        left: (Site, Remote),
        /// The right pane's site and connection.
        right: (Site, Remote),
    },
    /// A session's remote pane moved to another folder.
    RemoteDirChanged { session: SessionId, dir: RemotePath },
    /// A server said its own name, for its pane's title bar.
    ServerNamed {
        session: SessionId,
        /// The pane showing that server (left is `Side::Local`).
        pane: Side,
        name: String,
    },
    /// Which ways a site-to-site session's transfers are copying.
    Relaying {
        session: SessionId,
        /// Left server to right server.
        to_right: bool,
        /// Right server to left server.
        to_left: bool,
    },
    /// A site-to-site session's left pane (a server) moved to another folder.
    LeftDirChanged { session: SessionId, dir: RemotePath },
    /// Another session tab was brought forward: its transfer lists follow.
    SessionSwitched,
    /// Another bottom panel tab was brought forward: its session follows.
    BottomTabSwitched,
    /// Close the current session's connection (File > Disconnect).
    Disconnect,
    /// A session tab's X was clicked: disconnect and close that session (the app exits
    /// with the last one). Asks first while its transfers are running.
    CloseSession(SessionId),
    /// Close a session without asking.
    CloseSessionNow(SessionId),
    /// A file pane asked to copy, move or delete its selected items.
    PaneRequest {
        session: SessionId,
        /// The pane's side.
        side: Side,
        /// What to do.
        kind: JobKind,
        /// The selected items.
        paths: Vec<SidePath>,
    },
    /// Items were dropped on a file pane and the user chose to copy or move them.
    PaneDrop {
        session: SessionId,
        /// The side of the pane they were dropped on.
        side: Side,
        /// Copy or move.
        kind: JobKind,
        /// The dropped items.
        paths: Vec<SidePath>,
        /// They were dragged within that pane (so they stay on its side), not from the
        /// other pane.
        from_this_pane: bool,
        /// The folder they go into, on `side`.
        dest_dir: SidePath,
    },
    /// Run a job in a session (a delete, once confirmed).
    StartJob { session: SessionId, job: Job },
    /// The files and folders a session's running transfers use changed.
    PathsInUse {
        session: SessionId,
        /// On this computer.
        local: Vec<SidePath>,
        /// On the server.
        remote: Vec<SidePath>,
    },
    /// A session's transfer wrote something new on one side: refresh that pane.
    TargetChanged { session: SessionId, side: Side },
    /// A session's transfer finished: show its panes' new contents.
    TransferFinished(SessionId),
    /// Development aid for the UI gallery: open a second session and show the first
    /// as busy, to picture the session colours.
    #[cfg(debug_assertions)]
    GalleryDemo,
    /// Development aid for the UI gallery: bring the first session tab forward, to
    /// picture the transfer lists following it.
    #[cfg(debug_assertions)]
    GalleryFocusFirst,
    /// The number of a session's transfers running, completed or failed changed.
    TransferCounts {
        session: SessionId,
        /// Jobs running.
        running: usize,
        /// Jobs paused.
        paused: usize,
        /// Jobs in the Completed list.
        completed: usize,
        /// Jobs in the Failed list.
        failed: usize,
    },
    /// A session gave up its unfinished transfers (see `QuitDiscarding`).
    TransfersDiscarded(SessionId),
    /// Quit, after giving up the unfinished transfers and deleting their partial files.
    QuitDiscarding(&'static str),
    /// Look for a newer release: asked for (Help > Check for Updates) or automatic.
    CheckForUpdates { asked: bool },
    /// The look for a newer release ended: the newer release, none, or an error.
    UpdateChecked {
        asked: bool,
        result: Result<Option<Manifest>, String>,
    },
    /// The user turned the automatic update check on or off.
    AutoUpdateChecks(bool),
    /// A new version is in place (the AppImage replaced, or on Windows the installer
    /// downloaded): restart into it (asking first while transfers run).
    RestartIntoUpdate {
        /// The program to start after quitting.
        program: PathBuf,
        /// The new version, for the status bar until the restart.
        version: String,
    },
    /// Restart into the installed update once no transfer runs.
    RestartWhenIdle,
    /// Do not restart into the installed update by itself (it waits for a click on the
    /// status bar's notice).
    RestartLater,
    /// The transfers may have finished: restart into the update if they have.
    RestartIfIdle,
    /// The status bar's update notice was clicked: offer the update again, or restart
    /// into the installed one.
    ShowUpdate,
    /// The status bar's update notice was closed.
    DismissUpdateNotice,
    /// Restart into the new version without asking.
    RestartNow(PathBuf),
    /// The main window was resized, maximized or restored.
    WindowChanged {
        /// The window's normal size; `None` while maximized (its size then is the
        /// screen's, not one to restore later).
        size: Option<WindowSize>,
        /// Whether the window is maximized.
        maximized: bool,
    },
}

/// Root component of the application.
pub struct App {
    /// The session tabs, in tab order (the "New Connection" tab comes after them).
    sessions: Vec<Session>,
    /// Id the next session gets.
    next_session: SessionId,
    /// The session tabs.
    session_notebook: gtk::Notebook,
    /// Unfinished transfers, shared by every session's transfer lists.
    pending: Rc<PendingStore>,
    /// Name of this computer, for the titles of sessions that are not connected.
    host: String,
    config: Config,
    config_path: PathBuf,
    logger: Logger,
    /// Theme list inside the View menu.
    theme_list: ThemeList,
    /// Why the app is closing, written to the log's session-end banner.
    exit_reason: &'static str,
    /// The Login dialog (hidden until opened from File > Login or "New Connection").
    login: Controller<LoginDialog>,
    /// File > Disconnect, enabled while the current session is connected.
    disconnect_action: RelmAction<DisconnectAction>,
    /// The bottom panel: the log, then each session's transfer lists.
    bottom_panel: gtk::Notebook,
    /// The main window, for dialogs.
    window: gtk::ApplicationWindow,
    /// Transfers running in all sessions.
    running_transfers: Rc<Cell<usize>>,
    /// Transfers running or paused in all sessions; shared with the window's close
    /// handler, which asks before quitting while there are any.
    unfinished_transfers: Rc<Cell<usize>>,
    /// Quitting after giving up the unfinished transfers: the sessions still deleting
    /// partial files, and the reason to quit with.
    discarding: Option<(usize, &'static str)>,
    /// Set once quitting is decided, so closing the window no longer asks.
    quitting: Rc<Cell<bool>>,
    /// Start this program (the updated AppImage) once the app has closed.
    restart_with: Option<PathBuf>,
    /// A look for a newer release is under way.
    checking_for_updates: bool,
    /// What the status bar says about a newer release, until it is installed or the
    /// user closes the notice.
    update_notice: Option<UpdateNotice>,
}

/// The status bar's notice of a newer release.
#[derive(Debug, Clone)]
enum UpdateNotice {
    /// A newer release is out; clicking the notice offers it. (Boxed: a manifest is far
    /// larger than the other variant.)
    Available(Box<Manifest>),
    /// The new version is in place; restarting finishes the update.
    Installed {
        /// The new version.
        version: String,
        /// The program to start after quitting.
        program: PathBuf,
        /// Restart by itself once the running transfers have finished.
        when_idle: bool,
    },
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
            set_title: Some("ShuttleSCP"),
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

                    // Slim, with the window buttons where the system puts them.
                    append: &dialog::title_bar(None, dialog::MAIN_WINDOW_BUTTONS),

                    #[name = "menu_bar"]
                    gtk::PopoverMenuBar::from_model(Some(&menu_model)) {
                        set_visible: MenuBar::current() == MenuBar::InWindow,
                    },

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
                    #[local_ref]
                    set_start_child = session_notebook -> gtk::Notebook {
                        set_vexpand: true,
                        set_scrollable: true,
                        // Square tabs, like the bottom panel's (see style.css).
                        add_css_class: "session-tabs",
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
                        // A newer release: click to see it, x to hide the notice.
                        gtk::Box {
                            add_css_class: "update-notice",
                            #[watch]
                            set_visible: model.update_notice.is_some(),

                            gtk::Button {
                                add_css_class: "flat",
                                #[watch]
                                set_label: &model.update_notice_text(),
                                #[watch]
                                set_tooltip_text: Some(&model.update_notice_tooltip()),
                                connect_clicked => AppMsg::ShowUpdate,
                            },
                            gtk::Button {
                                add_css_class: "flat",
                                set_icon_name: "window-close-symbolic",
                                set_tooltip_text: Some("Hide this notice"),
                                connect_clicked => AppMsg::DismissUpdateNotice,
                            },
                        },
                        gtk::Separator {
                            set_orientation: gtk::Orientation::Vertical,
                            #[watch]
                            set_visible: model.update_notice.is_some(),
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
        // On macOS the app follows the system's Light/Dark appearance instead, and has
        // no theme menu; a theme saved on Linux stays in the settings untouched.
        if cfg!(target_os = "macos") {
            theme::apply(&ThemeChoice::System);
        } else {
            theme::apply(config.theme());
        }
        theme::ensure_cursor_theme();

        let disconnect_action = register_actions(&root, &config, logger.file_path(), &sender);
        let running_transfers = Rc::new(Cell::new(0));
        let unfinished_transfers = Rc::new(Cell::new(0));
        let quitting = Rc::new(Cell::new(false));
        {
            // Closing the window with unfinished transfers asks first (see `AppMsg::Quit`).
            let (running, quitting, sender) = (
                Rc::clone(&unfinished_transfers),
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
        let bottom_panel = bottom_panel(logger.file_path().map(Path::to_path_buf));
        let bar = MenuBar::current();
        let menu_model = build_menu(bar);
        if bar == MenuBar::System {
            relm4::main_application().set_menubar(Some(&menu_model));
        }
        let theme_list = ThemeList::new(themes, config.theme());
        #[cfg(unix)]
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
        let session_notebook = gtk::Notebook::new();
        let mut model = Self {
            sessions: Vec::new(),
            next_session: 0,
            session_notebook: session_notebook.clone(),
            pending: PendingStore::load(PendingTransfers::default_path()),
            host: computer_name(),
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
                    LoginOutput::ConnectedPair { left, right } => {
                        AppMsg::ConnectedPair { left, right }
                    }
                }),
            disconnect_action,
            bottom_panel: bottom_panel.clone(),
            window: root.clone(),
            running_transfers,
            unfinished_transfers,
            discarding: None,
            quitting,
            restart_with: None,
            checking_for_updates: false,
            update_notice: None,
        };
        // The "New Connection" tab, always last: clicking it opens the Login dialog.
        let new_tab = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        new_tab.append(&gtk::Image::from_icon_name(NEW_TAB_ICON));
        new_tab.append(&gtk::Label::new(Some("New Connection")));
        new_tab.set_tooltip_text(Some("Connect to another server in a new tab"));
        session_notebook.append_page(&gtk::Box::default(), Some(&new_tab));
        model.add_session(start_dir, window_size.width / 2, None, &sender);
        // `#[local_ref]` in the view takes a reference.
        let session_notebook = &session_notebook;
        let widgets = view_output!();
        if bar == MenuBar::InWindow {
            widgets
                .menu_bar
                .add_child(model.theme_list.widget(), THEME_LIST_ID);
        }
        resize_grips::add_corner_grips(&root, &widgets.overlay);
        #[cfg(target_os = "macos")]
        {
            theme::follow_system_appearance(&root);
            theme::use_mac_text_rendering();
            theme::use_mac_icons();
        }
        track_window_size(&root, &sender);
        let about_click = gtk::GestureClick::new();
        let about_window = root.clone();
        about_click.connect_released(move |_, _, _, _| {
            let _ = WidgetExt::activate_action(&about_window, "win.about", None);
        });
        widgets.build_label.add_controller(about_click);
        // Choosing the "New Connection" tab (by click or keyboard) opens the Login dialog
        // instead of showing its empty page. Our handler runs before GTK's default one,
        // which performs the switch, so stopping the emission here cancels it.
        {
            let sender = sender.clone();
            session_notebook.connect_switch_page(move |notebook, _page, page_num| {
                if page_num + 1 == notebook.n_pages() {
                    notebook.stop_signal_emission_by_name("switch-page");
                    sender.input(AppMsg::ShowLogin);
                } else {
                    sender.input(AppMsg::SessionSwitched);
                }
            });
        }
        {
            let sender = sender.clone();
            bottom_panel.connect_switch_page(move |_, _, _| {
                sender.input(AppMsg::BottomTabSwitched);
            });
        }
        // Development aid: save pictures of every window (see `ui_gallery`).
        #[cfg(debug_assertions)]
        if let Some(dir) = std::env::var_os(crate::ui_gallery::ENV_VAR) {
            let sender = sender.clone();
            let (demo_sender, focus_sender) = (sender.clone(), sender.clone());
            // With two servers named, the gallery also pictures a site-to-site session.
            if let Some(pair) = std::env::var_os(crate::ui_gallery::PAIR_VAR) {
                crate::ui_gallery::connect_pair(&pair.to_string_lossy(), sender.clone());
            }
            crate::ui_gallery::run(
                &root,
                dir.into(),
                move || sender.input(AppMsg::ShowLogin),
                move || demo_sender.input(AppMsg::GalleryDemo),
                move || focus_sender.input(AppMsg::GalleryFocusFirst),
            );
        }
        if model.update_check_due() {
            let sender = sender.clone();
            glib::timeout_add_local_once(UPDATE_CHECK_DELAY, move || {
                sender.input(AppMsg::CheckForUpdates { asked: false });
            });
        }
        if show_login {
            open_login_after_first_frame(&root, sender);
        }
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            AppMsg::CheckForUpdates { asked } => self.check_for_updates(asked, &sender),
            AppMsg::UpdateChecked { asked, result } => {
                self.checking_for_updates = false;
                self.update_checked(asked, result, &sender);
            }
            AppMsg::AutoUpdateChecks(check) => {
                tracing::info!("check for updates at startup: {check}");
                self.config.set_check_for_updates(check);
                self.save_config();
            }
            AppMsg::RestartIntoUpdate { program, version } => {
                let running = self.running_transfers.get();
                if running == 0 {
                    self.restart_into(program);
                    return;
                }
                // Kept in the status bar while the restart waits.
                let when_idle = matches!(
                    self.update_notice,
                    Some(UpdateNotice::Installed {
                        when_idle: true,
                        ..
                    })
                );
                self.update_notice = Some(UpdateNotice::Installed {
                    version: version.clone(),
                    program: program.clone(),
                    when_idle,
                });
                let sender = sender.clone();
                update_dialog::ask_restart(
                    self.window.upcast_ref(),
                    &version,
                    running,
                    move |choice| {
                        sender.input(match choice {
                            RestartChoice::Now => AppMsg::RestartNow(program.clone()),
                            RestartChoice::WhenTransfersFinish => AppMsg::RestartWhenIdle,
                            RestartChoice::Later => AppMsg::RestartLater,
                        });
                    },
                );
            }
            AppMsg::RestartNow(program) => self.restart_into(program),
            AppMsg::RestartWhenIdle | AppMsg::RestartLater => {
                let wait = matches!(message, AppMsg::RestartWhenIdle);
                if let Some(UpdateNotice::Installed { when_idle, .. }) = &mut self.update_notice {
                    *when_idle = wait;
                    tracing::info!("restart into the update when transfers finish: {wait}");
                }
            }
            AppMsg::RestartIfIdle => {
                if self.running_transfers.get() == 0
                    && self.restart_with.is_none()
                    && let Some(UpdateNotice::Installed {
                        program,
                        when_idle: true,
                        ..
                    }) = self.update_notice.clone()
                {
                    tracing::info!("transfers finished: restarting into the update");
                    self.restart_into(program);
                }
            }
            AppMsg::ShowUpdate => match self.update_notice.clone() {
                Some(UpdateNotice::Available(manifest)) => self.offer_update(*manifest, &sender),
                Some(UpdateNotice::Installed {
                    version, program, ..
                }) => {
                    sender.input(AppMsg::RestartIntoUpdate { program, version });
                }
                None => {}
            },
            AppMsg::DismissUpdateNotice => {
                tracing::info!("update notice hidden");
                self.update_notice = None;
            }
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
            AppMsg::Connected { site, remote } => self.connect(&site, remote, &sender),
            AppMsg::ConnectedPair { left, right } => self.connect_pair(left, right, &sender),
            AppMsg::RemoteDirChanged { session, dir } => {
                if let Some(connection) = self
                    .session_mut(session)
                    .and_then(|session| session.connection.as_mut())
                {
                    tracing::debug!("remote pane now shows {dir}");
                    connection.dir = dir;
                }
            }
            AppMsg::ServerNamed {
                session,
                pane,
                name,
            } => {
                if let Some(session) = self.session(session) {
                    session.name_server(pane, name);
                }
            }
            AppMsg::Relaying {
                session,
                to_right,
                to_left,
            } => {
                if let Some(session) = self.session(session) {
                    session.show_relaying(to_right, to_left);
                }
            }
            AppMsg::LeftDirChanged { session, dir } => {
                if let Some(left) = self
                    .session_mut(session)
                    .and_then(|session| session.left_server.as_mut())
                {
                    tracing::debug!("left pane now shows {dir}");
                    left.dir = dir;
                }
            }
            AppMsg::SessionSwitched => {
                self.transfers_follow_session();
                // Show what changed in its folders while another tab was in front.
                if let Some(session) = self.current() {
                    session.refresh_panes();
                }
            }
            AppMsg::BottomTabSwitched => self.session_follows_transfers(),
            #[cfg(debug_assertions)]
            AppMsg::GalleryFocusFirst => self.session_notebook.set_current_page(Some(0)),
            #[cfg(debug_assertions)]
            AppMsg::GalleryDemo => {
                if let Some(first) = self.sessions.first_mut() {
                    first.show_counts(2, 0, 1, 0);
                    // Two items of its local folder in use, to picture them in red.
                    let mut in_use: Vec<SidePath> = std::fs::read_dir(&first.local_dir)
                        .map(|entries| {
                            entries
                                .flatten()
                                .map(|entry| SidePath::Local(entry.path()))
                                .collect()
                        })
                        .unwrap_or_default();
                    in_use.sort();
                    in_use.truncate(2);
                    first.show_in_use(in_use, Vec::new());
                }
                let start = self.sessions[0].local_dir.clone();
                self.add_session(start, self.session_notebook.width() / 2, None, &sender);
                self.update_notice = Some(UpdateNotice::Available(Box::new(
                    crate::ui_gallery::sample_manifest(),
                )));
            }
            AppMsg::Disconnect => {
                if let Some(index) = self.current_index() {
                    // A site-to-site session is its two servers: disconnecting closes it.
                    if self.sessions[index].left_server.is_some() {
                        let id = self.sessions[index].id;
                        sender.input(AppMsg::CloseSession(id));
                    } else {
                        self.sessions[index].disconnect();
                    }
                }
            }
            AppMsg::CloseSession(id) => {
                let running = self.session(id).map_or(0, |session| session.running);
                if running > 0 {
                    self.confirm_stopping_transfers(
                        running,
                        "Close Session",
                        "Close the session?",
                        "Close Session",
                        &sender,
                        move || AppMsg::CloseSessionNow(id),
                    );
                } else {
                    self.close_session(id);
                }
            }
            AppMsg::CloseSessionNow(id) => self.close_session(id),
            AppMsg::ShowLoginAtStartup(show) => {
                tracing::info!("show Login dialog at startup: {show}");
                self.config.set_show_login_at_startup(show);
                self.save_config();
            }
            AppMsg::Quit(reason)
                if self.unfinished_transfers.get() > 0
                    && self.discarding.is_none()
                    && !QUIT_SIGNALS.iter().any(|&(_, signal)| signal == reason) =>
            {
                self.ask_quit(reason, &sender);
            }
            AppMsg::QuitDiscarding(reason) => {
                // Each session with unfinished transfers gives them up; the app quits once
                // all have, or after DISCARD_TIMEOUT (a server that does not answer must
                // not keep it open).
                let busy: Vec<usize> = (0..self.sessions.len())
                    .filter(|&index| self.sessions[index].running + self.sessions[index].paused > 0)
                    .collect();
                if busy.is_empty() {
                    self.quit(reason);
                    return;
                }
                tracing::info!("deleting the partial files of unfinished transfers, then quitting");
                self.discarding = Some((busy.len(), reason));
                for index in busy {
                    self.sessions[index]
                        .transfers
                        .emit(TransferPanelMsg::DiscardAll);
                }
                let sender = sender.clone();
                glib::timeout_add_local_once(DISCARD_TIMEOUT, move || {
                    sender.input(AppMsg::QuitNow(reason));
                });
            }
            AppMsg::TransfersDiscarded(session) => {
                tracing::info!("session {session}: unfinished transfers given up");
                if let Some((left, reason)) = self.discarding {
                    if left <= 1 {
                        self.quit(reason);
                    } else {
                        self.discarding = Some((left - 1, reason));
                    }
                }
            }
            AppMsg::Quit(reason) | AppMsg::QuitNow(reason) => self.quit(reason),
            AppMsg::LogLevelSelected(level) => {
                self.logger.set_level(level);
                self.config.set_log_level(self.logger.level());
                self.save_config();
            }
            AppMsg::PaneRequest {
                session,
                side,
                kind,
                paths,
            } => {
                self.request_job(session, side, kind, paths, &sender);
            }
            AppMsg::PaneDrop {
                session,
                side,
                kind,
                paths,
                from_this_pane,
                dest_dir,
            } => {
                // Items dragged from the other pane come from the other side.
                let source_side = if from_this_pane { side } else { side.other() };
                self.start_job(
                    session,
                    Job {
                        kind,
                        side: source_side,
                        sources: paths,
                        dest_dir,
                        same_side: from_this_pane,
                        between_servers: false,
                        left_server: false,
                        resume: false,
                    },
                );
            }
            AppMsg::StartJob { session, job } => self.start_job(session, job),
            AppMsg::TransferCounts {
                session,
                running,
                paused,
                completed,
                failed,
            } => {
                if let Some(session) = self.session_mut(session) {
                    session.show_counts(running, paused, completed, failed);
                }
                self.count_transfers();
            }
            AppMsg::PathsInUse {
                session,
                local,
                remote,
            } => {
                if let Some(session) = self.session_mut(session) {
                    session.show_in_use(local, remote);
                }
            }
            AppMsg::TargetChanged { session, side } => {
                if let Some(session) = self.session(session) {
                    session.refresh_side(side);
                }
            }
            AppMsg::TransferFinished(session) => {
                if let Some(session) = self.session(session) {
                    session.refresh_panes();
                }
            }
            AppMsg::WindowChanged { size, maximized } => {
                // Only kept in memory here; written to disk once, in `shutdown`.
                if let Some(size) = size {
                    self.config.set_window_size(size);
                }
                self.config.set_window_maximized(maximized);
            }
            AppMsg::LocalDirChanged {
                session,
                dir,
                disk_root,
            } => {
                tracing::debug!("local pane now shows {}", dir.display());
                // Only kept in memory here; written to disk once, in `shutdown`.
                if let Some(root) = disk_root {
                    self.config.set_last_dir_on_disk(root, dir.clone());
                }
                self.config.set_last_local_dir(dir.clone());
                if let Some(session) = self.session_mut(session) {
                    session.local_dir = dir;
                }
            }
        }
        self.refresh_sessions();
        // Whatever stopped the last transfer (it finished, its session closed, ...), an
        // update waiting for it can go ahead.
        self.restart_if_idle_soon(&sender);
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
        if let Some(path) = &self.restart_with {
            restart_after_exit(path);
        }
    }
}

impl App {
    /// Position of the current session tab in `sessions`.
    fn current_index(&self) -> Option<usize> {
        let page = usize::try_from(self.session_notebook.current_page()?).ok()?;
        (page < self.sessions.len()).then_some(page)
    }

    /// The current session.
    fn current(&self) -> Option<&Session> {
        self.current_index().map(|index| &self.sessions[index])
    }

    fn session(&self, id: SessionId) -> Option<&Session> {
        self.sessions.iter().find(|session| session.id == id)
    }

    fn session_mut(&mut self, id: SessionId) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|session| session.id == id)
    }

    /// Open a new, unconnected session tab (before "New Connection") and bring it forward.
    ///
    /// # Arguments
    ///
    /// * `start_dir` - Folder its local pane starts in
    /// * `split_position` - Initial position of its left/right divider, in pixels
    /// * `sender` - Receives the session's messages
    fn add_session(
        &mut self,
        start_dir: PathBuf,
        split_position: i32,
        left: Option<(Site, Remote)>,
        sender: &ComponentSender<Self>,
    ) -> usize {
        let id = self.next_session;
        self.next_session += 1;
        // The first colour no open session uses.
        let colour = (0..session::COLOURS)
            .find(|colour| {
                self.sessions
                    .iter()
                    .all(|session| session.colour != *colour)
            })
            .unwrap_or(0);
        let session = Session::new(SessionInit {
            id,
            colour,
            start_dir,
            last_dir_on_disk: self.config.last_dir_on_disk().clone(),
            pending: Rc::clone(&self.pending),
            split_position,
            left,
            sender,
        });
        // A session's transfer lists follow the log and the earlier sessions' lists.
        for (page, tab) in session
            .transfer_pages
            .iter()
            .zip(session.transfer_tab_widgets())
        {
            self.bottom_panel.append_page(page, Some(tab));
        }
        // Fill in the tab's title before GTK first measures it.
        session.refresh(&self.host, true);
        let index = self.sessions.len();
        // `index` counts sessions, so it fits; "New Connection" stays after them.
        let position = u32::try_from(index).unwrap_or(u32::MAX);
        self.session_notebook.insert_page(
            &session.page,
            Some(session.tab_widget()),
            Some(position),
        );
        tracing::info!("opened session {id}");
        self.sessions.push(session);
        self.session_notebook.set_current_page(Some(position));
        self.show_transfers(index);
        self.refresh_sessions();
        index
    }

    /// Show a newly connected server: in the current session if it is not connected,
    /// otherwise in a new session tab.
    fn connect(&mut self, site: &Site, remote: Remote, sender: &ComponentSender<Self>) {
        let index = match self.current_index() {
            Some(index) if self.sessions[index].connection.is_none() => index,
            current => {
                // The new tab starts where the current one's local pane is.
                let (start_dir, split) = current.map_or_else(
                    || (PathBuf::from("/"), 600),
                    |index| {
                        let session = &self.sessions[index];
                        (session.local_dir.clone(), self.session_notebook.width() / 2)
                    },
                );
                self.add_session(start_dir, split, None, sender)
            }
        };
        self.sessions[index].connect(site, remote, sender);
        self.session_notebook
            .set_current_page(u32::try_from(index).ok());
    }

    /// Open a site-to-site session in a new tab: `left` in the left pane, `right` in the
    /// right one.
    fn connect_pair(
        &mut self,
        left: (Site, Remote),
        right: (Site, Remote),
        sender: &ComponentSender<Self>,
    ) {
        let start_dir = self
            .current()
            .map_or_else(|| PathBuf::from("/"), |session| session.local_dir.clone());
        let split = self.session_notebook.width() / 2;
        let index = self.add_session(start_dir, split, Some(left), sender);
        let (site, remote) = right;
        self.sessions[index].connect(&site, remote, sender);
        self.session_notebook
            .set_current_page(u32::try_from(index).ok());
    }

    /// Close a session: disconnect it and remove its tabs. Closing the last one exits
    /// the app, as WinSCP does.
    fn close_session(&mut self, id: SessionId) {
        let Some(index) = self.sessions.iter().position(|session| session.id == id) else {
            return;
        };
        if self.sessions.len() == 1 {
            self.sessions[index].disconnect();
            self.quit(SESSION_TAB_CLOSED);
            return;
        }
        // Bring a neighbour forward first, so removing the page never lands on "New Connection".
        let neighbour = if index + 1 < self.sessions.len() {
            index + 1
        } else {
            index - 1
        };
        if self.current_index() == Some(index) {
            self.session_notebook
                .set_current_page(u32::try_from(neighbour).ok());
        }
        let mut session = self.sessions.remove(index);
        session.disconnect();
        if let Some(page) = self.session_notebook.page_num(&session.page) {
            self.session_notebook.remove_page(Some(page));
        }
        for page in &session.transfer_pages {
            if let Some(position) = self.bottom_panel.page_num(page) {
                self.bottom_panel.remove_page(Some(position));
            }
        }
        tracing::info!("closed session {id}");
        self.count_transfers();
    }

    /// Total the sessions' running and unfinished (running or paused) transfers.
    fn count_transfers(&self) {
        let running = self.sessions.iter().map(|session| session.running).sum();
        let paused: usize = self.sessions.iter().map(|session| session.paused).sum();
        self.running_transfers.set(running);
        self.unfinished_transfers.set(running + paused);
    }

    /// Ask what to do with the unfinished transfers before quitting: keep them to resume
    /// later, or give them up and delete their partial files.
    fn ask_quit(&self, reason: &'static str, sender: &ComponentSender<Self>) {
        let running = self.running_transfers.get();
        let paused = self.unfinished_transfers.get().saturating_sub(running);
        let sender = sender.clone();
        quit_dialog::ask(self.window.upcast_ref(), running, paused, move |choice| {
            sender.input(match choice {
                QuitChoice::Keep => AppMsg::QuitNow(reason),
                QuitChoice::Discard => AppMsg::QuitDiscarding(reason),
            });
        });
    }

    /// Bring every session's tabs up to date, and File > Disconnect to the current one.
    fn refresh_sessions(&self) {
        let several = self.sessions.len() > 1;
        for session in &self.sessions {
            // Every tab can be closed while several are open; the only one, once it is
            // connected (closing it exits the app).
            let closable = several || session.connection.is_some();
            session.refresh(&self.host, closable);
        }
        // Each tab strip's base line takes the colour of the session it shows, so the
        // selected tab runs into its page (see style.css).
        set_colour(&self.session_notebook, self.current());
        set_colour(
            &self.bottom_panel,
            self.bottom_session()
                .map(|(index, _)| &self.sessions[index]),
        );
        self.disconnect_action.set_enabled(
            self.current()
                .is_some_and(|session| session.connection.is_some()),
        );
    }

    /// The session whose transfer list the bottom panel shows, and which list (0 Queue,
    /// 1 Completed, 2 Failed); `None` while it shows the log.
    fn bottom_session(&self) -> Option<(usize, usize)> {
        let page = self
            .bottom_panel
            .nth_page(self.bottom_panel.current_page())?;
        self.sessions
            .iter()
            .enumerate()
            .find_map(|(index, session)| session.transfer_list_of(&page).map(|list| (index, list)))
    }

    /// The current session changed: show the same transfer list (Queue, Completed or
    /// Failed) of the new session. The log stays if it is showing.
    fn transfers_follow_session(&self) {
        let (Some(current), Some((shown, list))) = (self.current_index(), self.bottom_session())
        else {
            return;
        };
        if shown != current
            && let Some(page) = self
                .bottom_panel
                .page_num(&self.sessions[current].transfer_pages[list])
        {
            self.bottom_panel.set_current_page(Some(page));
        }
    }

    /// Another session's transfer list was chosen: bring that session forward.
    fn session_follows_transfers(&self) {
        if let Some((shown, _)) = self.bottom_session()
            && self.current_index() != Some(shown)
        {
            self.session_notebook
                .set_current_page(u32::try_from(shown).ok());
        }
    }

    /// Bring a session's Queue tab forward in the bottom panel.
    fn show_transfers(&self, index: usize) {
        if let Some(page) = self
            .sessions
            .get(index)
            .and_then(|session| self.bottom_panel.page_num(&session.transfer_pages[0]))
        {
            self.bottom_panel.set_current_page(Some(page));
        }
    }

    /// Title of the current session, for the window's status bar.
    fn connection_status(&self) -> String {
        self.current()
            .map_or_else(|| String::from(NOT_CONNECTED), Session::status)
    }

    /// Whether the automatic update check should run now: it is on, and the last one
    /// was long enough ago.
    fn update_check_due(&self) -> bool {
        #[cfg(debug_assertions)]
        if std::env::var_os(crate::ui_gallery::ENV_VAR).is_some() {
            return false;
        }
        let now = unix_now();
        self.config.check_for_updates()
            && self
                .config
                .last_update_check()
                .is_none_or(|last| now.saturating_sub(last) >= UPDATE_CHECK_INTERVAL.as_secs())
    }

    /// Offer a newer release in the update dialog; once it is in place, restart into it.
    fn offer_update(&self, manifest: Manifest, sender: &ComponentSender<Self>) {
        let (sender, version) = (sender.clone(), manifest.version.clone());
        update_dialog::offer(self.window.upcast_ref(), manifest, move |program| {
            sender.input(AppMsg::RestartIntoUpdate {
                program,
                version: version.clone(),
            });
        });
    }

    /// While an installed update waits for the transfers, check again shortly whether
    /// they have finished. The pause lets a transfer that starts right after another
    /// ends (a queue of drops, say) count as running.
    fn restart_if_idle_soon(&self, sender: &ComponentSender<Self>) {
        if self.running_transfers.get() == 0
            && matches!(
                self.update_notice,
                Some(UpdateNotice::Installed {
                    when_idle: true,
                    ..
                })
            )
        {
            let sender = sender.clone();
            glib::timeout_add_local_once(IDLE_RESTART_DELAY, move || {
                sender.input(AppMsg::RestartIfIdle);
            });
        }
    }

    /// Quit, then start `program`: the new AppImage, or on Windows the installer.
    fn restart_into(&mut self, program: PathBuf) {
        self.restart_with = Some(program);
        self.quit(RESTARTING_FOR_UPDATE);
    }

    /// The status bar's update notice.
    fn update_notice_text(&self) -> String {
        match &self.update_notice {
            Some(UpdateNotice::Available(manifest)) => {
                format!("Update available: {}", manifest.version)
            }
            Some(UpdateNotice::Installed {
                when_idle: true, ..
            }) => String::from("Update waits for transfers"),
            Some(UpdateNotice::Installed { .. }) => String::from("Restart to finish updating"),
            None => String::new(),
        }
    }

    /// What clicking the status bar's update notice does.
    fn update_notice_tooltip(&self) -> String {
        match &self.update_notice {
            Some(UpdateNotice::Available(manifest)) => format!(
                "ShuttleSCP {} is available (you have {}). Click to see it.",
                manifest.version,
                env!("CARGO_PKG_VERSION")
            ),
            Some(UpdateNotice::Installed {
                version,
                when_idle: true,
                ..
            }) => format!(
                "ShuttleSCP restarts into {version} once the running transfers finish. \
                 Click to restart now or later instead."
            ),
            Some(UpdateNotice::Installed { version, .. }) => {
                format!("ShuttleSCP {version} is ready. Click to restart into it.")
            }
            None => String::new(),
        }
    }

    /// Look for a newer release on a worker thread (it waits on the network).
    fn check_for_updates(&mut self, asked: bool, sender: &ComponentSender<Self>) {
        if self.checking_for_updates {
            return;
        }
        self.checking_for_updates = true;
        tracing::info!("checking for updates");
        let sender = sender.clone();
        std::thread::spawn(move || {
            let result = update::check(env!("CARGO_PKG_VERSION"));
            sender.input(AppMsg::UpdateChecked { asked, result });
        });
    }

    /// Show the result of a look for a newer release. An automatic check only speaks up
    /// when there is something new; problems are just logged.
    fn update_checked(
        &mut self,
        asked: bool,
        result: Result<Option<Manifest>, String>,
        sender: &ComponentSender<Self>,
    ) {
        if !asked {
            self.config.set_last_update_check(unix_now());
            self.save_config();
        }
        let window = self.window.upcast_ref::<gtk::Window>();
        match result {
            Ok(Some(manifest)) => {
                tracing::info!("version {} is available", manifest.version);
                if let Some(UpdateNotice::Installed { .. }) = self.update_notice {
                    // Already downloaded: only the restart is left.
                    if asked {
                        sender.input(AppMsg::ShowUpdate);
                    }
                    return;
                }
                self.update_notice = Some(UpdateNotice::Available(Box::new(manifest.clone())));
                self.offer_update(manifest, sender);
            }
            Ok(None) => {
                tracing::info!("ShuttleSCP is up to date");
                if asked {
                    update_dialog::report_up_to_date(window);
                }
            }
            Err(error) => {
                tracing::warn!("cannot check for updates: {error}");
                if asked {
                    update_dialog::report_error(window, &error);
                }
            }
        }
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
    /// * `running` - How many transfers would stop
    /// * `title` - Window title
    /// * `question` - What is about to happen, e.g. "Quit ShuttleSCP?"
    /// * `confirm_label` - Label of the button that goes ahead
    /// * `sender` - Receives the message
    /// * `on_yes` - Makes the message to send when the user goes ahead
    fn confirm_stopping_transfers(
        &self,
        running: usize,
        title: &str,
        question: &str,
        confirm_label: &str,
        sender: &ComponentSender<Self>,
        on_yes: impl Fn() -> AppMsg + 'static,
    ) {
        let detail = match running {
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

    /// Turn a pane's request into a job: copies go to the folder the session's other
    /// pane shows; deletes are confirmed first.
    ///
    /// # Arguments
    ///
    /// * `session` - The session of the pane
    /// * `side` - The pane the items are in
    /// * `kind` - Copy, move or delete
    /// * `paths` - The selected items
    /// * `sender` - Receives the confirmed delete
    fn request_job(
        &self,
        session: SessionId,
        side: Side,
        kind: JobKind,
        paths: Vec<SidePath>,
        sender: &ComponentSender<Self>,
    ) {
        let Some(current) = self.session(session) else {
            return;
        };
        // A delete has no destination: it stays on the pane's side, so deleting local
        // files works without a connection.
        let dest_pane = if kind == JobKind::Delete {
            side
        } else {
            side.other()
        };
        let Some(dest_dir) = current.pane_dir(dest_pane) else {
            return;
        };
        let job = Job {
            kind,
            side,
            sources: paths,
            dest_dir,
            same_side: false,
            between_servers: false,
            left_server: false,
            resume: false,
        };
        if kind != JobKind::Delete {
            self.start_job(session, job);
            return;
        }
        let question = format!("{}?", current.engine_job(job.clone()).title());
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
                    sender.input(AppMsg::StartJob {
                        session,
                        job: job.clone(),
                    });
                }
            },
        );
    }

    /// Run a job in a session and bring its Queue forward to show it.
    fn start_job(&self, session: SessionId, job: Job) {
        let Some(index) = self.sessions.iter().position(|s| s.id == session) else {
            return;
        };
        // Site to site, the job's sides become servers (see `Session::engine_job`).
        let job = self.sessions[index].engine_job(job);
        tracing::info!("session {session}: starting: {}", job.title());
        self.show_transfers(index);
        self.sessions[index]
            .transfers
            .emit(TransferPanelMsg::Start(job));
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

    let check_sender = sender.clone();
    let check_updates = RelmAction::<CheckUpdatesAction>::new_stateless(move |_| {
        check_sender.input(AppMsg::CheckForUpdates { asked: true });
    });
    let auto_sender = sender.clone();
    let auto_update =
        RelmAction::<AutoUpdateAction>::new_stateful(&config.check_for_updates(), move |_, on| {
            *on = !*on;
            auto_sender.input(AppMsg::AutoUpdateChecks(*on));
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
    group.add_action(check_updates);
    group.add_action(auto_update);
    group.add_action(theme_action);
    group.add_action(level_action);
    let group = group.into_action_group();
    window.insert_action_group(WindowActions::NAME, Some(&group));
    let app = relm4::main_application();
    let bar = MenuBar::current();
    if bar == MenuBar::System {
        // The same action objects, so a state (the theme, the log level) is shared.
        // "quit" and "about" become the "app.quit" and "app.about" that the macOS app
        // menu and the Dock's Quit call.
        for name in group.list_actions() {
            if let Some(action) = group.lookup_action(&name) {
                app.add_action(&action);
            }
        }
    }
    let group = bar.group();
    app.set_accels_for_action(&format!("{group}.{}", LoginAction::NAME), &["<primary>l"]);
    // On macOS GTK already gives "app.quit" Cmd+Q.
    if bar == MenuBar::InWindow {
        app.set_accels_for_action(&format!("{group}.{}", QuitAction::NAME), &["<primary>q"]);
    }
    disconnect
}

/// Open the Login dialog once the main window is really on screen: after its first
/// frame is painted, plus a moment for the desktop to show it. Opening on "map" alone
/// could put the dialog up before the window it belongs to.
///
/// # Arguments
///
/// * `window` - The main window
/// * `sender` - Receives [`AppMsg::ShowLogin`]
fn open_login_after_first_frame(window: &gtk::ApplicationWindow, sender: ComponentSender<App>) {
    let handled = Rc::new(Cell::new(false));
    window.connect_map(move |window| {
        // "map" can fire again if the window is re-shown; only act the first time.
        if handled.replace(true) {
            return;
        }
        let Some(clock) = window.frame_clock() else {
            sender.input(AppMsg::ShowLogin);
            return;
        };
        let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::default();
        let sender = sender.clone();
        let id = clock.connect_after_paint({
            let handler = Rc::clone(&handler);
            move |clock| {
                if let Some(id) = handler.borrow_mut().take() {
                    clock.disconnect(id);
                }
                let sender = sender.clone();
                glib::timeout_add_local_once(LOGIN_AFTER_FIRST_FRAME, move || {
                    sender.input(AppMsg::ShowLogin);
                });
            }
        });
        handler.replace(Some(id));
    });
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

/// Seconds since 1970.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Start `program` (the updated AppImage) once this process has exited: a second copy
/// started while this one runs would only hand over to it (GTK keeps one instance per
/// app) and close. A small shell waits for this process to end, then starts it.
///
/// The environment the AppImage set up for this copy (paths into its mount, which goes
/// away with it) is left out; the new AppImage sets up its own.
#[cfg(unix)]
fn restart_after_exit(program: &Path) {
    let mut command = std::process::Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; exec \"$2\"")
        .arg("shuttlescp-restart")
        .arg(std::process::id().to_string())
        .arg(program);
    if let Some(mount) = std::env::var_os("APPDIR") {
        let mount = mount.to_string_lossy().into_owned();
        for (name, value) in std::env::vars_os() {
            let from_mount = value.to_string_lossy().contains(&mount);
            if from_mount
                || ["APPDIR", "APPIMAGE", "ARGV0", "OWD"].contains(&name.to_string_lossy().as_ref())
            {
                command.env_remove(&name);
            }
        }
    }
    match command.spawn() {
        Ok(_) => tracing::info!("will start {} once closed", program.display()),
        Err(error) => tracing::error!("cannot restart into {}: {error}", program.display()),
    }
}

/// Windows: run `installer` (the downloaded update) once this process has exited, so it
/// can replace the app's files, then start the new version. PowerShell waits for this
/// process, runs the installer silently (it shows only its progress) and waits for it,
/// then starts the program from where this one ran.
#[cfg(windows)]
fn restart_after_exit(installer: &Path) {
    use std::os::windows::process::CommandExt;

    /// Process creation flag: no console window for PowerShell.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // PowerShell takes text in single quotes, with any quote inside doubled.
    let quoted = |path: &Path| path.display().to_string().replace('\'', "''");
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(error) => {
            tracing::error!("cannot find this program to restart it: {error}");
            return;
        }
    };
    let script = format!(
        "Wait-Process -Id {} -ErrorAction SilentlyContinue; \
         Start-Process -Wait -FilePath '{}' -ArgumentList '/SILENT','/NORESTART'; \
         Start-Process -FilePath '{}'",
        std::process::id(),
        quoted(installer),
        quoted(&program)
    );
    let spawned = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    match spawned {
        Ok(_) => tracing::info!("will run {} once closed", installer.display()),
        Err(error) => tracing::error!("cannot run {}: {error}", installer.display()),
    }
}

/// This computer's name, for the titles of sessions that are not connected. The UI
/// gallery uses a made-up one, so its pictures can be shared.
pub fn computer_name() -> String {
    #[cfg(debug_assertions)]
    if std::env::var_os(crate::ui_gallery::ENV_VAR).is_some() {
        return String::from("my-computer");
    }
    glib::host_name().into()
}

/// Mark a notebook with the colour of the session it shows (none: no colour).
fn set_colour(notebook: &gtk::Notebook, session: Option<&Session>) {
    for colour in 0..session::COLOURS {
        notebook.remove_css_class(&format!("shows-{}", session::colour_class(colour)));
    }
    if let Some(session) = session {
        notebook.add_css_class(&format!("shows-{}", session.colour_class()));
    }
}

/// The tabbed panel along the bottom of the window: the log first; each session adds
/// its transfer Queue, Completed and Failed lists after it.
///
/// # Arguments
///
/// * `log_path` - The log file, if logging to a file works
fn bottom_panel(log_path: Option<PathBuf>) -> gtk::Notebook {
    let panel = gtk::Notebook::new();
    panel.add_css_class("bottom-panel");
    panel.set_scrollable(true);
    let (log_tab, log_label) = session::panel_tab("text-x-generic-symbolic");
    log_label.set_label("Log");
    panel.append_page(&log_tail(log_path), Some(&log_tab));
    panel
}

/// Close the app gracefully on SIGHUP, SIGINT (Ctrl+C) and SIGTERM, so settings are saved
/// and the log gets its session-end banner. SIGKILL cannot be caught by any program.
/// Unix only: Windows has no such signals, and a GUI app there is closed through its
/// window.
///
/// # Arguments
///
/// * `sender` - Sender used to ask [`App::update`] to quit
#[cfg(unix)]
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
/// The macOS menu bar follows the Mac's layout: Quit and About sit in the app menu GTK
/// adds, and Edit and Window menus are added (GTK's own, which it shows only when the
/// app has no menu bar). It has no View menu: there the app follows the system's
/// Light/Dark appearance (see [`theme::follow_system_appearance`]), and the menu bar
/// could not show the theme list widget anyway.
///
/// # Arguments
///
/// * `bar` - Which menu bar the model is for
///
/// # Returns
///
/// The menu model for a [`gtk::PopoverMenuBar`] or the application's menu bar
fn build_menu(bar: MenuBar) -> gio::Menu {
    let in_window = bar == MenuBar::InWindow;
    let file_menu = gio::Menu::new();
    let session_section = gio::Menu::new();
    session_section.append_item(&bar.item::<LoginAction>("_Login…"));
    session_section.append_item(&bar.item::<DisconnectAction>("_Disconnect"));
    file_menu.append_section(None, &session_section);
    if in_window {
        let quit_section = gio::Menu::new();
        quit_section.append_item(&bar.item::<QuitAction>("_Quit"));
        file_menu.append_section(None, &quit_section);
    }

    let theme_section = gio::Menu::new();
    let theme_list_item = gio::MenuItem::new(None, None);
    theme_list_item.set_attribute_value("custom", Some(&THEME_LIST_ID.to_variant()));
    theme_section.append_item(&theme_list_item);
    let view_menu = gio::Menu::new();
    view_menu.append_section(Some("Theme"), &theme_section);

    let level_section = gio::Menu::new();
    for level in LogLevel::ALL {
        level_section.append_item(&bar.radio_item::<LogLevelAction>(level.label(), level.as_str()));
    }
    let log_file_section = gio::Menu::new();
    log_file_section.append_item(&bar.item::<OpenLogAction>("_Open Log File"));
    let tools_menu = gio::Menu::new();
    tools_menu.append_section(Some("Log Level"), &level_section);
    tools_menu.append_section(None, &log_file_section);

    let update_section = gio::Menu::new();
    update_section.append_item(&bar.item::<CheckUpdatesAction>("Check for _Updates…"));
    update_section.append_item(&bar.item::<AutoUpdateAction>("Check for Updates at _Startup"));
    let help_menu = gio::Menu::new();
    help_menu.append_section(None, &update_section);
    if in_window {
        help_menu.append_item(&bar.item::<AboutAction>("_About ShuttleSCP"));
    }

    let menu_bar = gio::Menu::new();
    menu_bar.append_submenu(Some("_File"), &file_menu);
    if !in_window {
        menu_bar.append_submenu(Some("_Edit"), &edit_menu());
    }
    if in_window {
        menu_bar.append_submenu(Some("_View"), &view_menu);
    }
    menu_bar.append_submenu(Some("_Tools"), &tools_menu);
    if !in_window {
        // GTK fills it with the system's window items (Minimize, Zoom, the open windows).
        let window_menu = gio::MenuItem::new_submenu(Some("_Window"), &gio::Menu::new());
        window_menu.set_attribute_value("gtk-macos-special", Some(&"window-submenu".to_variant()));
        menu_bar.append_item(&window_menu);
    }
    menu_bar.append_submenu(Some("_Help"), &help_menu);
    menu_bar
}

/// The macOS Edit menu: GTK's standard items, which act on the focused text field
/// (also in the system's own windows, such as the file chooser).
fn edit_menu() -> gio::Menu {
    let section = |items: &[(&str, &str)]| {
        let section = gio::Menu::new();
        for (label, action) in items {
            section.append(Some(label), Some(action));
        }
        section
    };
    let menu = gio::Menu::new();
    menu.append_section(
        None,
        &section(&[("Undo", "text.undo"), ("Redo", "text.redo")]),
    );
    menu.append_section(
        None,
        &section(&[
            ("Cut", "clipboard.cut"),
            ("Copy", "clipboard.copy"),
            ("Paste", "clipboard.paste"),
            ("Delete", "selection.delete"),
            ("Select All", "selection.select-all"),
        ]),
    );
    menu
}
