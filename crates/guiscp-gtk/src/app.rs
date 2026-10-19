//! Main window: menu bar on top, then one tab per session, each holding a local explorer
//! on the left and a remote explorer on the right.

use std::path::{Path, PathBuf};

use relm4::actions::{AccelsPlus, RelmAction, RelmActionGroup};
use relm4::gtk::prelude::*;
use relm4::gtk::{gio, glib};
use relm4::prelude::*;

use guiscp_core::config::{Config, ThemeChoice};
use guiscp_core::format::shorten_middle;
use guiscp_core::local_fs;
use guiscp_core::logging::{LogLevel, Logger};
use guiscp_core::remote::Remote;
use guiscp_core::sites::{Site, SiteBook};

use crate::file_pane::{FilePane, FilePaneInit, FilePaneOutput};
use crate::log_tail::log_tail;
use crate::login_dialog::{LoginDialog, LoginInit, LoginMsg, LoginOutput};
use crate::resize_grips;
use crate::theme;
use crate::theme_list::ThemeList;

/// Initial window width in pixels; the divider starts at half of it.
const WINDOW_WIDTH: i32 = 1200;
/// Initial window height in pixels.
const WINDOW_HEIGHT: i32 = 720;
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
/// Text shown in the remote half of the split while not connected.
const NOT_CONNECTED: &str = "Not connected";

// These macros declare marker types that name GIO actions in a type-safe way.
// "win" is the action group GTK attaches to the window, so actions are "win.quit" etc.
relm4::new_action_group!(WindowActions, "win");
relm4::new_stateless_action!(QuitAction, WindowActions, "quit");
relm4::new_stateless_action!(LoginAction, WindowActions, "login");
relm4::new_stateless_action!(DisconnectAction, WindowActions, "disconnect");
relm4::new_stateless_action!(OpenLogAction, WindowActions, "open-log");
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
    /// Close the app; the text says why and is written to the log.
    Quit(&'static str),
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
}

/// The open connection shown in the right half of the session.
struct Connection {
    /// "user@host" of the site, for the status bar.
    name: String,
    /// Server host name, for the tab title.
    host: String,
    /// Folder shown in the remote pane.
    dir: PathBuf,
    /// The remote file pane, held only to keep it running: dropping it closes the
    /// session (the leading `_` tells Rust it is deliberately never read).
    _pane: Controller<FilePane>,
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
            set_default_size: (WINDOW_WIDTH, WINDOW_HEIGHT),
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
                            set_position: WINDOW_WIDTH / 2,
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
        let bottom_panel = bottom_panel(logger.file_path().map(Path::to_path_buf));
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
        };
        let widgets = view_output!();
        // The session's left/right split is the notebook's first page.
        model.split = widgets.sessions.nth_page(Some(0)).and_downcast();
        widgets
            .menu_bar
            .add_child(model.theme_list.widget(), THEME_LIST_ID);
        resize_grips::add_corner_grips(&root, &widgets.overlay);
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
            AppMsg::Disconnect => {
                if let Some(connection) = self.connection.take() {
                    tracing::info!("disconnected from {}", connection.name);
                    if let Some(split) = &self.split {
                        split.set_end_child(Some(&self.placeholder));
                    }
                    self.disconnect_action.set_enabled(false);
                    // Dropping the pane drops its session, which closes the connection.
                    drop(connection);
                }
            }
            AppMsg::ShowLoginAtStartup(show) => {
                tracing::info!("show Login dialog at startup: {show}");
                self.config.set_show_login_at_startup(show);
                self.save_config();
            }
            AppMsg::Quit(reason) => {
                tracing::info!("closing: {reason}");
                self.exit_reason = reason;
                relm4::main_application().quit();
            }
            AppMsg::LogLevelSelected(level) => {
                self.logger.set_level(level);
                self.config.set_log_level(self.logger.level());
                self.save_config();
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
            });
        if let Some(split) = &self.split {
            split.set_end_child(Some(pane.widget()));
        }
        self.disconnect_action.set_enabled(true);
        self.connection = Some(Connection {
            name: site.display_name(),
            host: remote.host().to_owned(),
            dir: start_dir,
            _pane: pane,
        });
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
    group.add_action(theme_action);
    group.add_action(level_action);
    group.register_for_widget(window);
    let app = relm4::main_application();
    app.set_accelerators_for_action::<LoginAction>(&["<primary>l"]);
    app.set_accelerators_for_action::<QuitAction>(&["<primary>q"]);
    disconnect
}

/// The tabbed panel along the bottom of the window. Its first tab follows the log file.
///
/// # Arguments
///
/// * `log_path` - The log file, if logging to a file works
fn bottom_panel(log_path: Option<PathBuf>) -> gtk::Notebook {
    let panel = gtk::Notebook::new();
    panel.add_css_class("bottom-panel");
    panel.append_page(
        &log_tail(log_path),
        Some(&panel_tab("text-x-generic-symbolic", "Log")),
    );
    // Empty for now; a place to try out focus changes between tabs.
    let test_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    test_page.add_css_class("bottom-panel-page");
    panel.append_page(
        &test_page,
        Some(&panel_tab("applications-engineering-symbolic", "Test")),
    );
    panel
}

/// A bottom panel tab label: a small icon and a name.
///
/// # Arguments
///
/// * `icon` - Theme icon name
/// * `name` - Tab name
fn panel_tab(icon: &str, name: &str) -> gtk::Box {
    let label = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    label.append(&gtk::Image::from_icon_name(icon));
    label.append(&gtk::Label::new(Some(name)));
    label
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

/// Build the menu bar model: File, View (themes) and Tools (logging).
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

    let menu_bar = gio::Menu::new();
    menu_bar.append_submenu(Some("_File"), &file_menu);
    menu_bar.append_submenu(Some("_View"), &view_menu);
    menu_bar.append_submenu(Some("_Tools"), &tools_menu);
    menu_bar
}
