//! Development aid: save a picture of every window and dialog, to review the look of the
//! whole app at once. Debug builds only.
//!
//! Started by setting [`ENV_VAR`] to a folder: the app opens each window in turn, saves
//! it there as a PNG (rendered by GTK itself, so it works on Wayland, where screenshot
//! tools are usually blocked), closes it and finally quits. Run it with scratch
//! `XDG_CONFIG_HOME` / `XDG_STATE_HOME` folders to keep real settings untouched.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use relm4::gtk::prelude::*;
use relm4::gtk::{self, glib, graphene};

use shuttlescp_core::transfer::{FileInfo, Side};

use crate::confirm_dialog::{self, Confirmation};
use crate::conflict_dialog;
use crate::name_dialog::{self, NamePrompt};
use crate::passphrase_dialog::{self, PassphrasePurpose};
use crate::update_dialog;

/// Environment variable naming the folder the pictures go into.
pub const ENV_VAR: &str = "SHUTTLESCP_UI_GALLERY";
/// Application ID used while taking the pictures.
pub const APP_ID: &str = "io.github.dsalvado.ShuttleSCP.Gallery";
/// Time for a window to open and draw before its picture is taken.
const SETTLE: Duration = Duration::from_millis(800);

/// Something to take a picture of: its file name, and how to open it.
type Step = (&'static str, Box<dyn Fn(&gtk::Window)>);

/// Take the pictures, then quit.
///
/// # Arguments
///
/// * `main` - The main window
/// * `dir` - Folder the pictures go into
/// * `open_login` - Opens the Login dialog
/// * `open_sessions` - Opens a second session tab, with the first one busy
/// * `focus_first` - Brings the first session tab forward again
pub fn run(
    main: &gtk::ApplicationWindow,
    dir: PathBuf,
    open_login: impl Fn() + 'static,
    open_sessions: impl Fn() + 'static,
    focus_first: impl Fn() + 'static,
) {
    if let Err(error) = std::fs::create_dir_all(&dir) {
        tracing::error!("UI gallery: cannot create {}: {error}", dir.display());
        return;
    }
    let ago = |hours: u64| SystemTime::now() - Duration::from_secs(hours * 3600);
    let steps: Vec<Step> = vec![
        ("main-window", Box::new(|_| {})),
        (
            "menu-view",
            Box::new(|window| {
                if let Some(menu) = theme_menu(window) {
                    menu.popup();
                }
            }),
        ),
        ("sessions", Box::new(move |_| open_sessions())),
        ("sessions-first", Box::new(move |_| focus_first())),
        ("login", Box::new(move |_| open_login())),
        (
            "about",
            Box::new(|window| {
                let _ = WidgetExt::activate_action(window, "win.about", None);
            }),
        ),
        (
            "confirm",
            Box::new(|window| {
                confirm_dialog::ask(
                    window,
                    Confirmation {
                        title: "Delete",
                        question: "Delete 3 items from the server?",
                        detail: "Folders are deleted with everything in them. This cannot be \
                                 undone.",
                        confirm_label: "Delete",
                    },
                    |_| {},
                );
            }),
        ),
        (
            "name",
            Box::new(|window| {
                name_dialog::ask(
                    window,
                    NamePrompt {
                        title: "Rename",
                        prompt: "New name for report.pdf:",
                        initial: "report.pdf",
                        confirm_label: "Rename",
                        side: Side::Local,
                    },
                    |_| {},
                );
            }),
        ),
        (
            "conflict",
            Box::new(move |window| {
                conflict_dialog::ask(
                    window,
                    "report.pdf",
                    "/home/me/Documents",
                    FileInfo {
                        size: 1_204_224,
                        modified: Some(ago(30)),
                    },
                    FileInfo {
                        size: 1_318_400,
                        modified: Some(ago(1)),
                    },
                    |_| {},
                );
            }),
        ),
        (
            "passphrase-export",
            Box::new(|window| {
                passphrase_dialog::ask(window, PassphrasePurpose::Export, |_| {});
            }),
        ),
        (
            "update-available",
            Box::new(|window| {
                let repository = env!("CARGO_PKG_REPOSITORY");
                update_dialog::offer(
                    window,
                    shuttlescp_core::update::Manifest {
                        version: String::from("1.0.0"),
                        published: String::from("2026-11-02T09:00:00Z"),
                        notes_url: format!("{repository}/releases/tag/v1.0.0"),
                        appimage: shuttlescp_core::update::Asset {
                            name: String::from("ShuttleSCP-1.0.0-x86_64.AppImage"),
                            url: format!("{repository}/releases/download/v1.0.0/x.AppImage"),
                            size: 0,
                            sha256: String::new(),
                        },
                        macos: Some(shuttlescp_core::update::Asset {
                            name: String::from("ShuttleSCP-1.0.0-macos-arm64.dmg"),
                            url: format!("{repository}/releases/download/v1.0.0/x.dmg"),
                            size: 0,
                            sha256: String::new(),
                        }),
                    },
                    |_| {},
                );
            }),
        ),
        (
            "update-error",
            Box::new(|window| {
                update_dialog::report_error(window, "this build cannot check for updates");
            }),
        ),
        (
            "key-passphrase",
            Box::new(|window| {
                crate::key_passphrase_dialog::ask(
                    window,
                    std::path::Path::new("/home/me/.ssh/id_ed25519"),
                    false,
                    |_| {},
                );
            }),
        ),
        (
            "passphrase-import",
            Box::new(|window| {
                passphrase_dialog::ask(window, PassphrasePurpose::Import, |_| {});
            }),
        ),
    ];
    tracing::info!(
        "UI gallery: saving {} pictures in {}",
        steps.len(),
        dir.display()
    );
    let main = main.clone().upcast::<gtk::Window>();
    let next = Rc::new(Cell::new(0));
    let steps = Rc::new(steps);
    glib::timeout_add_local(SETTLE, move || {
        let index = next.get();
        // Each tick photographs the window opened by the previous tick, then opens the
        // next one.
        if index > 0 {
            let (name, _) = &steps[index - 1];
            let path = dir.join(format!("{index:02}-{name}.png"));
            if name.starts_with("menu") {
                // A menu is its own surface: picture it, then close it.
                if let Some(menu) = theme_menu(&main) {
                    save_png(menu.upcast_ref(), &path);
                    menu.popdown();
                }
            } else {
                let window = if name.starts_with("main") || name.starts_with("sessions") {
                    // These steps picture the main window itself.
                    main.clone()
                } else {
                    newest_dialog(&main).unwrap_or_else(|| main.clone())
                };
                save_png(window.upcast_ref(), &path);
            }
        }
        close_dialogs(&main);
        let Some((_, open)) = steps.get(index) else {
            tracing::info!("UI gallery: done");
            relm4::main_application().quit();
            return glib::ControlFlow::Break;
        };
        open(&main);
        next.set(index + 1);
        glib::ControlFlow::Continue
    });
}

/// The most recently opened window other than the main one, if one is showing.
fn newest_dialog(main: &gtk::Window) -> Option<gtk::Window> {
    let toplevels = gtk::Window::toplevels();
    (0..toplevels.n_items())
        .rev()
        .filter_map(|position| toplevels.item(position).and_downcast::<gtk::Window>())
        .find(|window| window != main && window.is_visible())
}

/// Close every window but the main one.
fn close_dialogs(main: &gtk::Window) {
    while let Some(window) = newest_dialog(main) {
        window.close();
        if window.is_visible() {
            window.set_visible(false);
        }
    }
}

/// The View menu: the menu holding the theme list.
fn theme_menu(main: &gtk::Window) -> Option<gtk::Popover> {
    fn find(widget: &gtk::Widget) -> Option<gtk::Widget> {
        if widget.has_css_class("theme-list") {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = find(&current) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }
    find(main.upcast_ref())?
        .ancestor(gtk::Popover::static_type())
        .and_downcast()
}

/// Render a window or menu as GTK draws it and save it as a PNG.
fn save_png(window: &gtk::Widget, path: &std::path::Path) {
    let (width, height) = (window.width(), window.height());
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
    let (Some(node), Some(renderer)) = (
        snapshot.to_node(),
        window.native().and_then(|native| native.renderer()),
    ) else {
        tracing::warn!("UI gallery: nothing drawn for {}", path.display());
        return;
    };
    let bounds = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
    let texture = renderer.render_texture(&node, Some(&bounds));
    match texture.save_to_png(path) {
        Ok(()) => tracing::info!("UI gallery: saved {}", path.display()),
        Err(error) => tracing::warn!("UI gallery: cannot save {}: {error}", path.display()),
    }
}
