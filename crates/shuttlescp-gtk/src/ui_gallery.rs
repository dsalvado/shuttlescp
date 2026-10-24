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
/// Optional: two servers, `user@host,user@host`, logged in to with the SSH agent's keys,
/// to picture a site-to-site session (the "sessions-site-to-site" step). Their host keys
/// must already be in `~/.ssh/known_hosts`.
pub const PAIR_VAR: &str = "SHUTTLESCP_UI_GALLERY_PAIR";
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
    let open_login = Rc::new(open_login);
    let open_login_again = Rc::clone(&open_login);
    let open_login_unfinished = Rc::clone(&open_login);
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
        // Shows whatever tab is in front: the site-to-site one, if `PAIR_VAR` opened one
        // (it comes forward once both servers are connected).
        (
            "sessions-site-to-site",
            Box::new(|window| show_site_to_site_tab(window.upcast_ref())),
        ),
        (
            "login-site-to-site",
            Box::new(move |window| {
                open_login_again();
                // The dialog opens on the next main-loop turns; then bring its Site to
                // site tab forward (the "login" step after this one shows Direct).
                show_login_tab(window, "site-to-site");
            }),
        ),
        (
            "login",
            Box::new(move |window| {
                open_login();
                // Back to the Direct tab (the dialog keeps the last one shown).
                show_login_tab(window, "direct");
            }),
        ),
        (
            "login-unfinished",
            Box::new(move |window| {
                open_login_unfinished();
                // Its first transfer picked (details shown) and ticked.
                show_login_tab(window, "unfinished");
            }),
        ),
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
                update_dialog::offer(window, sample_manifest(), |_| {});
            }),
        ),
        (
            "update-restart",
            Box::new(|window| {
                update_dialog::ask_restart(window, "1.0.0", 2, |_| {});
            }),
        ),
        (
            "quit",
            Box::new(|window| {
                crate::quit_dialog::ask(window, 1, 1, |_| {});
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

/// Once the Login dialog has opened (it does on the next main-loop turn), bring the tab
/// named `tab` forward; on the Site to site tab, also pick a site in each list so their
/// summaries show.
fn show_login_tab(main: &gtk::Window, tab: &'static str) {
    let main = main.clone();
    glib::idle_add_local_once(move || {
        let Some(dialog) = newest_dialog(&main) else {
            return;
        };
        let Some(stack) = find_stack(dialog.upcast_ref()) else {
            return;
        };
        stack.set_visible_child_name(tab);
        let Some(page) = stack.child_by_name(tab) else {
            return;
        };
        if tab == "site-to-site" {
            for (list, row) in site_lists(&page).iter().zip([0, 2]) {
                if let Some(row) = list.row_at_index(row) {
                    row.activate();
                }
            }
        }
        if tab == "unfinished"
            && let Some(list) = first_descendant::<gtk::ListBox>(&page)
            && let Some(row) = list.row_at_index(0)
        {
            list.select_row(Some(&row));
            if let Some(check) = first_descendant::<gtk::CheckButton>(row.upcast_ref()) {
                check.set_active(true);
            }
        }
    });
}

/// The first widget of type `T` inside `widget` (depth first), if any.
fn first_descendant<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Option<T> {
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = current
            .clone()
            .downcast::<T>()
            .ok()
            .or_else(|| first_descendant(&current))
        {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

/// The site lists inside `widget`, in order.
fn site_lists(widget: &gtk::Widget) -> Vec<gtk::ListBox> {
    let mut found = Vec::new();
    if let Some(list) = widget.downcast_ref::<gtk::ListBox>()
        && list.has_css_class("site-list")
    {
        found.push(list.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        found.extend(site_lists(&current));
        child = current.next_sibling();
    }
    found
}

/// The first stack inside `widget` (depth first), e.g. the Login dialog's tabs.
fn find_stack(widget: &gtk::Widget) -> Option<gtk::Stack> {
    if let Some(stack) = widget.downcast_ref::<gtk::Stack>() {
        return Some(stack.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(stack) = find_stack(&current) {
            return Some(stack);
        }
        child = current.next_sibling();
    }
    None
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

/// A made-up release 1.0.0, to picture the update dialog and the status bar's notice.
pub fn sample_manifest() -> shuttlescp_core::update::Manifest {
    let repository = env!("CARGO_PKG_REPOSITORY");
    let asset = |name: &str, file: &str| shuttlescp_core::update::Asset {
        name: String::from(name),
        url: format!("{repository}/releases/download/v1.0.0/{file}"),
        size: 0,
        sha256: String::new(),
    };
    shuttlescp_core::update::Manifest {
        version: String::from("1.0.0"),
        published: String::from("2026-11-02T09:00:00Z"),
        notes_url: format!("{repository}/releases/tag/v1.0.0"),
        appimage: asset("ShuttleSCP-1.0.0-x86_64.AppImage", "x.AppImage"),
        macos: Some(asset("ShuttleSCP-1.0.0-macos-arm64.dmg", "x.dmg")),
        windows: Some(asset("ShuttleSCP-1.0.0-windows-x64-setup.exe", "x.exe")),
    }
}

/// Log in to the two servers named in [`PAIR_VAR`] and open a site-to-site session.
pub fn connect_pair(pair: &str, sender: relm4::ComponentSender<crate::app::App>) {
    let sites: Vec<shuttlescp_core::sites::Site> = pair
        .split(',')
        .filter_map(|target| target.trim().split_once('@'))
        .map(|(user, host)| shuttlescp_core::sites::Site {
            id: format!("gallery-{host}"),
            protocol: shuttlescp_core::sites::Protocol::Sftp,
            host: host.to_owned(),
            port: 22,
            user: user.to_owned(),
            password_saved: false,
            ftp: shuttlescp_core::sites::FtpOptions::default(),
            ssh: shuttlescp_core::sites::SshAuth {
                key_file: None,
                use_agent: true,
                passphrase_saved: false,
            },
        })
        .collect();
    let [left, right] = <[_; 2]>::try_from(sites)
        .unwrap_or_else(|_| panic!("{PAIR_VAR} must name two servers, user@host,user@host"));
    relm4::spawn(async move {
        let open = |site: shuttlescp_core::sites::Site| async move {
            let credentials = shuttlescp_core::remote::Credentials::default();
            match shuttlescp_core::remote::connect(&site, credentials, None).await {
                Ok(remote) => Some((site, remote)),
                Err(error) => {
                    tracing::error!("UI gallery: cannot log in to {}: {error}", site.host);
                    None
                }
            }
        };
        if let (Some(left), Some(right)) = (open(left).await, open(right).await) {
            sender.input(crate::app::AppMsg::ConnectedPair {
                left,
                right,
                resume: None,
            });
        }
    });
}

/// Bring the site-to-site session's tab (its title has "⇄") forward, if there is one.
fn show_site_to_site_tab(widget: &gtk::Widget) {
    if let Some(notebook) = widget.downcast_ref::<gtk::Notebook>()
        && notebook.has_css_class("session-tabs")
    {
        for page in 0..notebook.n_pages() {
            let is_pair = notebook
                .nth_page(Some(page))
                .and_then(|child| notebook.tab_label(&child))
                .is_some_and(|label| has_text(&label, '\u{21c4}'));
            if is_pair {
                notebook.set_current_page(Some(page));
                return;
            }
        }
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        show_site_to_site_tab(&current);
        child = current.next_sibling();
    }
}

/// Whether a label inside `widget` contains `wanted`.
fn has_text(widget: &gtk::Widget, wanted: char) -> bool {
    if let Some(label) = widget.downcast_ref::<gtk::Label>()
        && label.label().contains(wanted)
    {
        return true;
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if has_text(&current, wanted) {
            return true;
        }
        child = current.next_sibling();
    }
    false
}
