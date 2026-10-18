//! Main window: menu bar on top, local explorer on the left, remote explorer on the right.

use std::path::{Path, PathBuf};

use relm4::actions::{AccelsPlus, RelmAction, RelmActionGroup};
use relm4::gtk::gio;
use relm4::gtk::prelude::*;
use relm4::prelude::*;

use crate::config::Config;
use crate::file_pane::{FilePane, FilePaneOutput};
use crate::local_fs;
use crate::logging::{LogLevel, Logger};
use crate::resize_grips;
use crate::theme::{self, ThemeChoice};

/// Initial window width in pixels; the divider starts at half of it.
const WINDOW_WIDTH: i32 = 1200;
/// Initial window height in pixels.
const WINDOW_HEIGHT: i32 = 720;
/// Label of the theme menu entry that follows the desktop theme.
const SYSTEM_THEME_LABEL: &str = "System default";

// These macros declare marker types that name GIO actions in a type-safe way.
// "win" is the action group GTK attaches to the window, so actions are "win.quit" etc.
relm4::new_action_group!(WindowActions, "win");
relm4::new_stateless_action!(QuitAction, WindowActions, "quit");
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
    /// The local pane moved to another directory.
    LocalDirChanged(PathBuf),
}

/// Root component of the application.
pub struct App {
    /// `Controller` owns a running child component and gives access to its root widget.
    local_pane: Controller<FilePane>,
    config: Config,
    config_path: PathBuf,
    logger: Logger,
}

#[relm4::component(pub)]
impl SimpleComponent for App {
    type Init = AppInit;
    type Input = AppMsg;
    type Output = ();

    view! {
        gtk::ApplicationWindow {
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

                    gtk::PopoverMenuBar::from_model(Some(&menu_model)) {},

                    gtk::Paned {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_vexpand: true,
                        set_position: WINDOW_WIDTH / 2,
                        set_wide_handle: true,
                        set_shrink_start_child: false,
                        set_shrink_end_child: false,
                        // Gives the divider a visible, hoverable handle (see style.css).
                        add_css_class: "pane-split",

                        // `#[wrap(Some)]` passes the widget as `Some(widget)`, since the setter
                        // takes an `Option` (GTK allows clearing a child with `None`).
                        #[wrap(Some)]
                        set_start_child = model.local_pane.widget(),

                        #[wrap(Some)]
                        set_end_child = &gtk::Label {
                            set_label: "Not connected",
                            add_css_class: "dim-label",
                        },
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
        }
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
        let menu_model = build_menu(&themes);

        let start_dir = config
            .last_local_dir()
            .and_then(local_fs::nearest_listable_dir)
            .map(PathBuf::from)
            .or_else(std::env::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        tracing::info!("opening local pane at {}", start_dir.display());
        let model = Self {
            // `.forward()` converts each pane output into a message for this component.
            local_pane: FilePane::builder().launch(start_dir).forward(
                sender.input_sender(),
                |output| match output {
                    FilePaneOutput::DirectoryChanged(dir) => AppMsg::LocalDirChanged(dir),
                },
            ),
            config,
            config_path,
            logger,
        };
        let widgets = view_output!();
        resize_grips::add_corner_grips(&root, &widgets.overlay);
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
                self.config.set_theme(choice);
                self.save_config();
            }
            AppMsg::LogLevelSelected(level) => {
                self.logger.set_level(level);
                self.config.set_log_level(self.logger.level());
                self.save_config();
            }
            AppMsg::LocalDirChanged(dir) => {
                tracing::debug!("local pane now shows {}", dir.display());
                // Only kept in memory here; written to disk once, in `shutdown`.
                self.config.set_last_local_dir(dir);
            }
        }
    }

    /// Runs when the application quits (window closed or File > Quit).
    fn shutdown(&mut self, _widgets: &mut Self::Widgets, _output: relm4::Sender<Self::Output>) {
        self.save_config();
        tracing::info!("GuiSCP shutting down");
    }
}

impl App {
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
    let quit = RelmAction::<QuitAction>::new_stateless(|_| relm4::main_application().quit());

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
    group.add_action(quit);
    group.add_action(open_log);
    group.add_action(theme_action);
    group.add_action(level_action);
    group.register_for_widget(window);
    relm4::main_application().set_accelerators_for_action::<QuitAction>(&["<primary>q"]);
}

/// Build the menu bar model: File, View (themes) and Tools (logging).
///
/// Radio lists sit in sections rather than nested submenus, so every menu opens as a
/// single popup.
///
/// # Arguments
///
/// * `themes` - Installed theme names, listed after "System default"
///
/// # Returns
///
/// The menu model for a [`gtk::PopoverMenuBar`]
fn build_menu(themes: &[String]) -> gio::Menu {
    let file_menu = gio::Menu::new();
    file_menu.append_item(&RelmAction::<QuitAction>::to_menu_item("_Quit"));

    let theme_section = gio::Menu::new();
    let system_target = ThemeChoice::System.action_target().to_owned();
    theme_section.append_item(&RelmAction::<ThemeAction>::to_menu_item_with_target_value(
        SYSTEM_THEME_LABEL,
        &system_target,
    ));
    for name in themes {
        theme_section.append_item(&RelmAction::<ThemeAction>::to_menu_item_with_target_value(
            name, name,
        ));
    }
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
