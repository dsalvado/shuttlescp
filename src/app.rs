//! Main window: menu bar on top, then one tab per session, each holding a local explorer
//! on the left and a remote explorer on the right.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use relm4::actions::{AccelsPlus, RelmAction, RelmActionGroup};
use relm4::gtk::prelude::*;
use relm4::gtk::{gio, glib};
use relm4::prelude::*;

use crate::config::Config;
use crate::file_pane::{FilePane, FilePaneInit, FilePaneOutput};
use crate::local_fs;
use crate::logging::{LogLevel, Logger};
use crate::login_dialog::{LoginDialog, LoginMsg};
use crate::resize_grips;
use crate::sites::SiteBook;
use crate::theme::{self, ThemeChoice};
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
/// Marker inserted where a tab title was shortened.
const TAB_TITLE_ELLIPSIS: &str = "...";
/// Icon of a session tab.
const SESSION_TAB_ICON: &str = "computer-symbolic";
/// Icon of the "New Tab" tab, which will open a new connection.
const NEW_TAB_ICON: &str = "tab-new-symbolic";
/// Position of the "New Tab" tab, after the single session tab.
const NEW_TAB_PAGE: u32 = 1;

// These macros declare marker types that name GIO actions in a type-safe way.
// "win" is the action group GTK attaches to the window, so actions are "win.quit" etc.
relm4::new_action_group!(WindowActions, "win");
relm4::new_stateless_action!(QuitAction, WindowActions, "quit");
relm4::new_stateless_action!(LoginAction, WindowActions, "login");
relm4::new_stateless_action!(OpenLogAction, WindowActions, "open-log");
// Target and state are both a `ThemeChoice::action_target()` string. A stateful action
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

                    #[name = "sessions"]
                    gtk::Notebook {
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

                            #[wrap(Some)]
                            set_end_child = &gtk::Label {
                                set_label: "Not connected",
                                add_css_class: "dim-label",
                            },
                        },

                        // Placeholder page; its tab is disabled until new connections exist.
                        append_page[Some(&new_tab)] = &gtk::Box {},
                    },

                    // Status bar along the bottom of the window.
                    gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_spacing: 12,
                        add_css_class: "statusbar",

                        gtk::Label {
                            set_label: "Not connected",
                            set_hexpand: true,
                            set_xalign: 0.0,
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

        register_actions(&root, &config, logger.file_path(), &sender);
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
        let model = Self {
            // `.forward()` converts each pane output into a message for this component.
            local_pane: FilePane::builder()
                .launch(FilePaneInit {
                    start_dir: start_dir.clone(),
                    last_dir_on_disk: config.last_dir_on_disk().clone(),
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
                .launch(SiteBook::default_path())
                .detach(),
        };
        let widgets = view_output!();
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
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
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
    /// Full session tab title: `<host> - <folder shown in the local pane>`.
    fn tab_title(&self) -> String {
        // The root directory has no file name, so it is shown as its full path ("/").
        let folder = self.local_dir.file_name().map_or_else(
            || self.local_dir.to_string_lossy(),
            |name| name.to_string_lossy(),
        );
        format!("{} - {folder}", self.host)
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
fn register_actions(
    window: &gtk::ApplicationWindow,
    config: &Config,
    log_path: Option<&Path>,
    sender: &ComponentSender<App>,
) {
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
        &config.theme().action_target().to_owned(),
        move |_, state, target| {
            theme_sender.input(AppMsg::ThemeSelected(ThemeChoice::from_action_target(
                &target,
            )));
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
    group.add_action(quit);
    group.add_action(open_log);
    group.add_action(theme_action);
    group.add_action(level_action);
    group.register_for_widget(window);
    let app = relm4::main_application();
    app.set_accelerators_for_action::<LoginAction>(&["<primary>l"]);
    app.set_accelerators_for_action::<QuitAction>(&["<primary>q"]);
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

/// Shorten `text` to at most `max_chars` characters by replacing its middle with "...".
///
/// # Arguments
///
/// * `text` - Text to shorten
/// * `max_chars` - Maximum length in characters (Unicode scalar values), including "..."
///
/// # Returns
///
/// `text` unchanged (borrowed, no allocation) when it fits, otherwise its start and end
/// joined by "..."
///
/// # Examples
///
/// ```ignore
/// assert_eq!(shorten_middle("abcdefghij", 7), "ab...ij");
/// ```
fn shorten_middle(text: &str, max_chars: usize) -> Cow<'_, str> {
    let length = text.chars().count();
    if length <= max_chars {
        return Cow::Borrowed(text);
    }
    let kept = max_chars.saturating_sub(TAB_TITLE_ELLIPSIS.len());
    // The start gets the extra character when `kept` is odd.
    let head_chars = kept.div_ceil(2);
    let tail_chars = kept - head_chars;
    // Convert character counts to byte offsets: Rust strings are UTF-8, so slicing by a
    // character count directly could split a multi-byte character.
    let head_end = text
        .char_indices()
        .nth(head_chars)
        .map_or(text.len(), |(offset, _)| offset);
    let tail_start = text
        .char_indices()
        .nth(length - tail_chars)
        .map_or(text.len(), |(offset, _)| offset);
    Cow::Owned(format!(
        "{}{TAB_TITLE_ELLIPSIS}{}",
        &text[..head_end],
        &text[tail_start..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_middle_keeps_short_text() {
        assert!(matches!(
            shorten_middle("host - home", 32),
            Cow::Borrowed("host - home")
        ));
        let exact = "a".repeat(32);
        assert_eq!(shorten_middle(&exact, 32), exact);
    }

    #[test]
    fn shorten_middle_cuts_long_text_to_the_limit() {
        // Arrange
        let title = "workstation.example.com - a-very-long-project-folder-name";

        // Act
        let short = shorten_middle(title, 32);

        // Assert
        assert_eq!(short.chars().count(), 32);
        assert_eq!(short, "workstation.exa...ct-folder-name");
    }

    #[test]
    fn shorten_middle_counts_characters_not_bytes() {
        let short = shorten_middle("ééééééééééééééé", 9);
        assert_eq!(short, "ééé...ééé");
    }
}
