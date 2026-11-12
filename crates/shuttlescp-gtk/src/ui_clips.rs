//! Development aid: record the short clips of the user guide (`docs/help`). Debug builds
//! only.
//!
//! Started by `scripts/record-help-clips.sh`, which sets [`ENV_VAR`] to a folder, picks a
//! scene with [`SCENE_VAR`] and names throwaway servers in [`SITES_VAR`]. The app logs in,
//! then plays the scene as a user would: a drawn pointer glides to a file, drags it to the
//! other pane and picks Copy Here from the pane's own drop menu. Meanwhile the window is
//! saved as GTK draws it (with the pointer and any open menu) eight times a second, and
//! `frames.txt` lists the pictures with how long each shows; the script turns them into a
//! GIF.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use relm4::gtk::prelude::*;
use relm4::gtk::{self, gdk, glib, graphene};
use secrecy::SecretString;

use shuttlescp_core::path::{RemotePath, SidePath};
use shuttlescp_core::remote::{self, ConnectError, Credentials, Remote};
use shuttlescp_core::sites::{self, Site};
use shuttlescp_core::tr;
use shuttlescp_core::transfer::Side;

use crate::app::{App, AppMsg};
use crate::file_pane::clip;
use crate::login_dialog::Resume;

/// Environment variable naming the folder the pictures go into.
pub const ENV_VAR: &str = "SHUTTLESCP_UI_CLIPS";
/// Application ID used while recording, so it runs beside an open ShuttleSCP.
pub const APP_ID: &str = "io.github.dsalvado.ShuttleSCP.Clips";
/// The scene to play: [`UPLOAD_DOWNLOAD`] or [`SITE_TO_SITE`].
const SCENE_VAR: &str = "SHUTTLESCP_UI_CLIPS_SCENE";
/// The servers, `alias=user:password@host:port/folder`, separated by commas: the first
/// one for the upload and download, both for site to site.
const SITES_VAR: &str = "SHUTTLESCP_UI_CLIPS_SITES";
/// A file dragged to the server, then one downloaded.
const UPLOAD_DOWNLOAD: &str = "upload-download";
/// A folder dragged from one server to the other.
const SITE_TO_SITE: &str = "site-to-site";
/// The folder the local pane shows.
const LOCAL_DIR: &str = "/home/alex/Documents";
/// Time between pictures.
const FRAME: Duration = Duration::from_millis(125);
/// How long the last picture shows, in hundredths of a second.
const LAST_FRAME_HOLD: u128 = 200;
/// The longest wait for the app (a listing, a transfer) before giving up.
const PATIENCE: Duration = Duration::from_secs(90);
/// The pointer's outline, from its tip.
const ARROW: [(f64, f64); 7] = [
    (0.0, 0.0),
    (0.0, 17.0),
    (4.2, 13.2),
    (7.0, 19.5),
    (9.6, 18.4),
    (6.8, 12.4),
    (12.2, 12.4),
];

/// What is drawn over the window.
struct Stage {
    /// Where the pointer's tip is, in the window.
    pointer: (f64, f64),
    /// A picture dragged along with the pointer, and where the pointer holds it.
    ghost: Option<(gtk::Widget, (f64, f64))>,
    /// A click shows as a ring around the pointer until then.
    click_until: Option<Instant>,
}

type SharedStage = Rc<RefCell<Stage>>;

/// A server named in [`SITES_VAR`].
#[derive(Clone)]
struct Server {
    site: Site,
    password: String,
    /// The folder its pane shows.
    folder: String,
}

/// Play the scene named in [`SCENE_VAR`], record it, then quit.
///
/// # Arguments
///
/// * `main` - The main window
/// * `sender` - Logs in, as the Login dialog would
pub fn start(main: &gtk::ApplicationWindow, sender: relm4::ComponentSender<App>) {
    let dir = PathBuf::from(std::env::var_os(ENV_VAR).unwrap_or_default());
    let scene = std::env::var(SCENE_VAR).unwrap_or_else(|_| String::from(UPLOAD_DOWNLOAD));
    let servers = std::env::var(SITES_VAR).unwrap_or_default();
    let servers: Vec<Server> = servers.split(',').filter_map(parse_server).collect();
    let main = main.clone().upcast::<gtk::Window>();
    glib::MainContext::default().spawn_local(async move {
        match play(&main, &dir, &scene, &servers, &sender).await {
            Ok(frames) => tracing::info!("UI clips: {frames} pictures in {}", dir.display()),
            Err(error) => tracing::error!("UI clips: {error}"),
        }
        relm4::main_application().quit();
    });
}

/// A server from `alias=user:password@host:port/folder`.
fn parse_server(text: &str) -> Option<Server> {
    let (alias, rest) = text.trim().split_once('=')?;
    let (login, address) = rest.rsplit_once('@')?;
    let (user, password) = login.split_once(':')?;
    let (host_port, folder) = address.split_once('/')?;
    let (host, port) = host_port.split_once(':')?;
    let site = Site {
        id: format!("clip-{alias}"),
        protocol: sites::Protocol::Sftp,
        host: host.to_owned(),
        port: port.parse().ok()?,
        user: user.to_owned(),
        password_saved: false,
        ftp: sites::FtpOptions::default(),
        ssh: sites::SshAuth {
            key_file: None,
            use_agent: false,
            passphrase_saved: false,
        },
        files_at_once: sites::DEFAULT_FILES_AT_ONCE,
        alias: alias.to_owned(),
        description: String::new(),
        folder: String::new(),
        verify: sites::SiteVerify::default(),
    };
    Some(Server {
        site,
        password: password.to_owned(),
        folder: format!("/{folder}"),
    })
}

/// Log in, open the scene's session, play it while recording; the number of pictures.
async fn play(
    main: &gtk::Window,
    dir: &Path,
    scene: &str,
    servers: &[Server],
    sender: &relm4::ComponentSender<App>,
) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let web = servers
        .first()
        .ok_or(format!("{SITES_VAR} names no server"))?;
    let remote_dir = |server: &Server| SidePath::Remote(RemotePath::new(server.folder.clone()));
    // The files each pane must show before the scene starts.
    let (left_file, right_file) = match scene {
        UPLOAD_DOWNLOAD => {
            let remote = log_in(web).await?;
            sender.input(AppMsg::Connected {
                site: web.site.clone(),
                remote,
                resume: Some(Resume {
                    keys: Vec::new(),
                    folders: vec![
                        (Side::Local, SidePath::Local(PathBuf::from(LOCAL_DIR))),
                        (Side::Remote, remote_dir(web)),
                    ],
                }),
            });
            ("report.pdf", "logo.png")
        }
        SITE_TO_SITE => {
            let backup = servers
                .get(1)
                .ok_or(format!("{SITES_VAR} names one server"))?;
            let left = (web.site.clone(), log_in(web).await?);
            let right = (backup.site.clone(), log_in(backup).await?);
            sender.input(AppMsg::ConnectedPair {
                left,
                right,
                resume: Some(Resume {
                    keys: Vec::new(),
                    folders: vec![
                        (Side::Local, remote_dir(web)),
                        (Side::Remote, remote_dir(backup)),
                    ],
                }),
            });
            ("images", "2025")
        }
        other => return Err(format!("no scene called {other}")),
    };
    let (left, right) = wait_for("the listings", || {
        let (left, right) = panes(main)?;
        (label_in(&left, left_file).is_some() && label_in(&right, right_file).is_some())
            .then_some((left, right))
    })
    .await?;
    if scene == SITE_TO_SITE {
        // The site-to-site session opened in a tab of its own: close the empty one the
        // app started with.
        close_first_session(main);
    }
    // Time for the window to settle (the status bar, the panes' titles).
    pause(1000).await;
    let stage = Rc::new(RefCell::new(Stage {
        pointer: (
            f64::from(main.width()) * 0.5,
            f64::from(main.height()) * 0.55,
        ),
        ghost: None,
        click_until: None,
    }));
    let recording = Recording::start(main, dir, &stage);
    pause(700).await;
    let completed_tab = if scene == UPLOAD_DOWNLOAD {
        let upload = [SidePath::Local(Path::new(LOCAL_DIR).join(left_file))];
        drag_and_copy(
            main,
            &stage,
            (&left, left_file),
            &right,
            &upload,
            &remote_dir(web),
        )
        .await?;
        wait_for_completed(main, 1).await?;
        pause(1200).await;
        right_click_and_copy(main, &stage, &right, right_file).await?;
        wait_for_completed(main, 2).await?
    } else {
        let backup = &servers[1];
        let folder = [SidePath::Remote(RemotePath::new(format!(
            "{}/{left_file}",
            web.folder
        )))];
        drag_and_copy(
            main,
            &stage,
            (&left, left_file),
            &right,
            &folder,
            &remote_dir(backup),
        )
        .await?;
        wait_for_completed(main, 1).await?
    };
    // The finished transfers; the newest one opened, to show its files marked verified.
    pause(900).await;
    let completed = open_tab(main, &stage, &completed_tab).await?;
    pause(700).await;
    let expander = wait_for("a finished transfer", || {
        descendants::<gtk::Button>(&completed)
            .into_iter()
            .find(|button| {
                button.is_mapped() && button.icon_name().as_deref() == Some("pan-end-symbolic")
            })
    })
    .await?;
    let bounds = expander
        .compute_bounds(main)
        .ok_or("the transfer is not in the window")?;
    let centre = (
        f64::from(bounds.x() + bounds.width() * 0.5),
        f64::from(bounds.y() + bounds.height() * 0.5),
    );
    glide(&stage, centre, 800).await;
    click(&stage).await;
    expander.emit_clicked();
    pause(3000).await;
    recording.finish(dir)
}

/// Close the first session tab, with its close button.
fn close_first_session(main: &gtk::Window) {
    let tab = descendants::<gtk::Notebook>(main.upcast_ref())
        .into_iter()
        .find(|notebook| notebook.has_css_class("session-tabs"))
        .and_then(|notebook| {
            let first = notebook.nth_page(Some(0))?;
            notebook.tab_label(&first)
        });
    let close = tab.and_then(|tab| descendants::<gtk::Button>(&tab).into_iter().next());
    if let Some(close) = close {
        close.emit_clicked();
    }
}

/// Log in to `server` with its password, trusting its host key (`known_hosts` belongs
/// to the recording's throwaway home).
async fn log_in(server: &Server) -> Result<Remote, String> {
    let Server { site, password, .. } = server.clone();
    relm4::spawn(async move {
        let credentials = || Credentials {
            password: Some(SecretString::from(password.as_str())),
            key_passphrase: None,
        };
        let first = remote::connect(&site, credentials(), None).await;
        let logged_in = match first {
            Err(ConnectError::UnknownHostKey { key, .. }) => {
                remote::connect(&site, credentials(), Some(*key)).await
            }
            other => other,
        };
        logged_in.map_err(|error| format!("cannot log in to {}: {error}", site.host))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Drag `name` from the `from` pane to the `to` pane and choose Copy Here.
///
/// # Arguments
///
/// * `main` - The main window
/// * `stage` - The pointer
/// * `from` - The pane the file is in, and its name
/// * `to` - The pane it is dropped on
/// * `sources` - Its full path, as the drop carries it
/// * `dest_dir` - The folder the `to` pane shows
async fn drag_and_copy(
    main: &gtk::Window,
    stage: &SharedStage,
    (from, name): (&gtk::ColumnView, &str),
    to: &gtk::ColumnView,
    sources: &[SidePath],
    dest_dir: &SidePath,
) -> Result<(), String> {
    let label = label_in(from, name).ok_or(format!("no {name}"))?;
    glide(stage, grip_of(&label, main)?, 900).await;
    if let Some(position) = clip::position_of(from, name)
        && let Some(selection) = from.model()
    {
        selection.select_item(position, true);
    }
    click(stage).await;
    // The picture dragged along is the name and its icon, held where the pointer took it.
    let cell = label.parent().unwrap_or_else(|| label.clone().upcast());
    let origin = cell
        .compute_point(main, &graphene::Point::zero())
        .ok_or("the file is not in the window")?;
    let pointer = stage.borrow().pointer;
    stage.borrow_mut().ghost = Some((
        cell,
        (
            pointer.0 - f64::from(origin.x()),
            pointer.1 - f64::from(origin.y()),
        ),
    ));
    let bounds = to
        .compute_bounds(main)
        .ok_or("the other pane is not in the window")?;
    let target = (
        f64::from(bounds.x() + bounds.width() * 0.4),
        f64::from(bounds.y() + bounds.height() * 0.62),
    );
    glide(stage, target, 1300).await;
    stage.borrow_mut().ghost = None;
    let (x, y) = in_widget(main, to, target)?;
    activate(
        to,
        clip::DROP,
        Some(&clip::drop_variant(sources, dest_dir, x, y)),
    )?;
    pick_first_item(main, stage, to, clip::DROP_COPY).await
}

/// Right-click `name` in `pane` and choose its first item (Upload, Download, Copy to).
async fn right_click_and_copy(
    main: &gtk::Window,
    stage: &SharedStage,
    pane: &gtk::ColumnView,
    name: &str,
) -> Result<(), String> {
    let label = label_in(pane, name).ok_or(format!("no {name}"))?;
    let grip = grip_of(&label, main)?;
    glide(stage, grip, 900).await;
    click(stage).await;
    let position = clip::position_of(pane, name).ok_or(format!("{name} is not listed"))?;
    let (x, y) = in_widget(main, pane, grip)?;
    activate(
        pane,
        clip::CONTEXT_MENU,
        Some(&(position, x, y).to_variant()),
    )?;
    pick_first_item(main, stage, pane, clip::COPY).await
}

/// Once a pane's menu is open, move to its first item and choose it with `action`.
async fn pick_first_item(
    main: &gtk::Window,
    stage: &SharedStage,
    pane: &gtk::ColumnView,
    action: &str,
) -> Result<(), String> {
    let menu = wait_for("the menu", || open_menus(main).into_iter().next()).await?;
    pause(450).await;
    let item = descendants::<gtk::Label>(menu.upcast_ref())
        .into_iter()
        .find(|label| !label.label().is_empty())
        .ok_or("the menu has no items")?;
    let offset = menu_offset(main, &menu).ok_or("the menu is not placed")?;
    let bounds = item
        .compute_bounds(&menu)
        .ok_or("the menu item is not in the menu")?;
    // Just past the item's text, so the pointer does not hide it.
    let target = (
        offset.0 + f64::from(bounds.x() + bounds.width() + 8.0),
        offset.1 + f64::from(bounds.y() + bounds.height() * 0.5),
    );
    glide(stage, target, 600).await;
    click(stage).await;
    activate(pane, action, None)?;
    menu.popdown();
    Ok(())
}

/// Activate one of a pane's actions.
fn activate(
    pane: &gtk::ColumnView,
    action: &str,
    parameter: Option<&glib::Variant>,
) -> Result<(), String> {
    WidgetExt::activate_action(pane, action, parameter)
        .map_err(|error| format!("cannot activate {action}: {error}"))
}

/// Wait until the Completed tab counts `count` transfers; the tab's title.
async fn wait_for_completed(main: &gtk::Window, count: usize) -> Result<gtk::Label, String> {
    let title = format!("{} ({count})", tr!("list-completed"));
    wait_for("the transfer", || {
        descendants::<gtk::Label>(main.upcast_ref())
            .into_iter()
            .find(|label| label.is_mapped() && label.label() == title)
    })
    .await
}

/// Click a notebook tab, given its title; the page it shows.
async fn open_tab(
    main: &gtk::Window,
    stage: &SharedStage,
    title: &gtk::Label,
) -> Result<gtk::Widget, String> {
    let bounds = title
        .compute_bounds(main)
        .ok_or("the tab is not in the window")?;
    let centre = (
        f64::from(bounds.x() + bounds.width() * 0.5),
        f64::from(bounds.y() + bounds.height() * 0.55),
    );
    glide(stage, centre, 900).await;
    click(stage).await;
    let notebook = title
        .ancestor(gtk::Notebook::static_type())
        .and_downcast::<gtk::Notebook>()
        .ok_or("the tab is in no notebook")?;
    let page = (0..notebook.n_pages())
        .find(|&page| {
            notebook
                .nth_page(Some(page))
                .and_then(|child| notebook.tab_label(&child))
                .is_some_and(|tab| {
                    title.is_ancestor(&tab) || tab == *title.upcast_ref::<gtk::Widget>()
                })
        })
        .ok_or("no page has that tab")?;
    notebook.set_current_page(Some(page));
    notebook
        .nth_page(Some(page))
        .ok_or_else(|| String::from("the page is gone"))
}

/// Check `ready` every tenth of a second until it gives something, for at most
/// [`PATIENCE`].
async fn wait_for<T>(what: &str, mut ready: impl FnMut() -> Option<T>) -> Result<T, String> {
    let start = Instant::now();
    loop {
        if let Some(found) = ready() {
            return Ok(found);
        }
        if start.elapsed() > PATIENCE {
            return Err(format!("gave up waiting for {what}"));
        }
        pause(100).await;
    }
}

async fn pause(milliseconds: u64) {
    glib::timeout_future(Duration::from_millis(milliseconds)).await;
}

/// Move the pointer to `to` in `milliseconds`, slow at both ends.
async fn glide(stage: &SharedStage, to: (f64, f64), milliseconds: u64) {
    let from = stage.borrow().pointer;
    let start = Instant::now();
    let length = Duration::from_millis(milliseconds).as_secs_f64();
    loop {
        let t = (start.elapsed().as_secs_f64() / length).min(1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        stage.borrow_mut().pointer = (
            from.0 + (to.0 - from.0) * eased,
            from.1 + (to.1 - from.1) * eased,
        );
        if t >= 1.0 {
            return;
        }
        pause(16).await;
    }
}

/// Show a click, and wait a moment as a user would.
async fn click(stage: &SharedStage) {
    stage.borrow_mut().click_until = Some(Instant::now() + Duration::from_millis(350));
    pause(400).await;
}

/// The two file lists of the session shown, left first.
fn panes(main: &gtk::Window) -> Option<(gtk::ColumnView, gtk::ColumnView)> {
    let mut views: Vec<(f32, gtk::ColumnView)> = descendants::<gtk::ColumnView>(main.upcast_ref())
        .into_iter()
        .filter(|view| view.is_mapped())
        .filter_map(|view| Some((view.compute_bounds(main)?.x(), view)))
        .collect();
    views.sort_by(|a, b| a.0.total_cmp(&b.0));
    match <[_; 2]>::try_from(views) {
        Ok([(_, left), (_, right)]) => Some((left, right)),
        Err(_) => None,
    }
}

/// The label showing `name` in a file list, if its row is drawn.
fn label_in(view: &gtk::ColumnView, name: &str) -> Option<gtk::Label> {
    descendants::<gtk::Label>(view.upcast_ref())
        .into_iter()
        .find(|label| label.is_mapped() && label.label() == name)
}

/// Where the pointer takes a file's name: a little into it.
fn grip_of(label: &gtk::Label, main: &gtk::Window) -> Result<(f64, f64), String> {
    let bounds = label
        .compute_bounds(main)
        .ok_or("the file is not in the window")?;
    Ok((
        f64::from(bounds.x() + bounds.width().min(70.0) * 0.5),
        f64::from(bounds.y() + bounds.height() * 0.55),
    ))
}

/// A point of the window in `widget`'s coordinates.
fn in_widget(
    main: &gtk::Window,
    widget: &impl IsA<gtk::Widget>,
    (x, y): (f64, f64),
) -> Result<(f64, f64), String> {
    let point = main
        .compute_point(widget, &graphene::Point::new(x as f32, y as f32))
        .ok_or("the point is not in the pane")?;
    Ok((f64::from(point.x()), f64::from(point.y())))
}

/// All widgets of type `T` inside `widget`, depth first.
fn descendants<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Vec<T> {
    let mut found = Vec::new();
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(own) = current.downcast_ref::<T>() {
            found.push(own.clone());
        }
        found.extend(descendants::<T>(&current));
        child = current.next_sibling();
    }
    found
}

/// The panes' menus that are open.
fn open_menus(main: &gtk::Window) -> Vec<gtk::PopoverMenu> {
    descendants::<gtk::PopoverMenu>(main.upcast_ref())
        .into_iter()
        .filter(|menu| menu.is_visible() && menu.has_css_class("pane-menu"))
        .collect()
}

/// Where a menu is drawn, in the window. A menu is its own surface, placed by the
/// windowing system next to the window's.
fn menu_offset(main: &gtk::Window, menu: &gtk::PopoverMenu) -> Option<(f64, f64)> {
    let popup = menu.surface()?.downcast::<gdk::Popup>().ok()?;
    let (menu_x, menu_y) = menu.surface_transform();
    let (main_x, main_y) = main.surface_transform();
    Some((
        f64::from(popup.position_x()) + menu_x - main_x,
        f64::from(popup.position_y()) + menu_y - main_y,
    ))
}

/// The pictures being taken.
struct Recording {
    /// Each picture's file name and when it was taken.
    frames: Rc<RefCell<Vec<(String, Instant)>>>,
    source: glib::SourceId,
}

impl Recording {
    /// Take a picture every [`FRAME`] until [`Recording::finish`].
    fn start(main: &gtk::Window, dir: &Path, stage: &SharedStage) -> Self {
        let frames = Rc::new(RefCell::new(Vec::new()));
        let failed = Cell::new(false);
        let source = {
            let (main, dir, stage, frames) = (
                main.clone(),
                dir.to_owned(),
                Rc::clone(stage),
                Rc::clone(&frames),
            );
            glib::timeout_add_local(FRAME, move || {
                // Drawn as when it has the focus: the desktop usually hands the focus back
                // to the window the recording was started from.
                main.unset_state_flags(gtk::StateFlags::BACKDROP);
                let name = format!("frame-{:04}.png", frames.borrow().len());
                let saved = match draw(&main, &stage.borrow()) {
                    // Now and then the renderer gives an empty picture: skip it (the one
                    // before shows a little longer).
                    Some(texture) if is_blank(&texture) => return glib::ControlFlow::Continue,
                    Some(texture) => texture
                        .save_to_png(dir.join(&name))
                        .map_err(|error| error.to_string()),
                    None => Err(String::from("nothing drawn")),
                };
                match saved {
                    Ok(()) => frames.borrow_mut().push((name, Instant::now())),
                    // Once is enough to know.
                    Err(error) if !failed.replace(true) => {
                        tracing::warn!("UI clips: cannot save {name}: {error}");
                    }
                    Err(_) => {}
                }
                glib::ControlFlow::Continue
            })
        };
        Self { frames, source }
    }

    /// Stop, and write `frames.txt`: each picture and how long it shows, in hundredths
    /// of a second. The number of pictures.
    fn finish(self, dir: &Path) -> Result<usize, String> {
        self.source.remove();
        let frames = self.frames.borrow();
        let list: String = frames
            .iter()
            .enumerate()
            .map(|(index, (name, taken))| {
                let shows = frames.get(index + 1).map_or(LAST_FRAME_HOLD, |(_, next)| {
                    next.duration_since(*taken).as_millis() / 10
                });
                format!("{name} {shows}\n")
            })
            .collect();
        let path = dir.join("frames.txt");
        std::fs::write(&path, list).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(frames.len())
    }
}

/// The window as GTK draws it, with its open menus, the dragged picture and the pointer.
fn draw(main: &gtk::Window, stage: &Stage) -> Option<gdk::Texture> {
    let (width, height) = (main.width() as f32, main.height() as f32);
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(main)).snapshot(&snapshot, width.into(), height.into());
    // A menu is drawn by its own renderer (the window's draws nothing of it), then
    // pasted in as a picture.
    for menu in open_menus(main) {
        if let (Some((x, y)), Some(picture)) = (menu_offset(main, &menu), render(menu.upcast_ref()))
        {
            let bounds = graphene::Rect::new(
                x as f32,
                y as f32,
                picture.width() as f32,
                picture.height() as f32,
            );
            snapshot.append_texture(&picture, &bounds);
        }
    }
    if let Some((ghost, (held_x, held_y))) = &stage.ghost {
        let (x, y) = stage.pointer;
        paint_at(&snapshot, ghost, (x - held_x, y - held_y), 0.7);
    }
    let clicking = stage
        .click_until
        .is_some_and(|until| Instant::now() < until);
    draw_pointer(&snapshot, stage.pointer, clicking);
    let node = snapshot.to_node()?;
    let renderer = main.renderer()?;
    let bounds = graphene::Rect::new(0.0, 0.0, width, height);
    Some(renderer.render_texture(&node, Some(&bounds)))
}

/// Whether the window is missing from a picture (menus and the pointer may be there):
/// judged by a row of its title bar, which nothing covers but the pointer.
fn is_blank(texture: &gdk::Texture) -> bool {
    let stride = texture.width() as usize * 4;
    let mut pixels = vec![0; stride * texture.height() as usize];
    texture.download(&mut pixels, stride);
    let row = pixels.chunks_exact(stride).nth(6).unwrap_or_default();
    let transparent = row
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] == 0)
        .count();
    transparent > row.len() / 8
}

/// A widget with its own surface (a menu), as its renderer draws it.
fn render(widget: &gtk::Widget) -> Option<gdk::Texture> {
    let (width, height) = (widget.width() as f32, widget.height() as f32);
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(widget)).snapshot(&snapshot, width.into(), height.into());
    let node = snapshot.to_node()?;
    let renderer = widget.native()?.renderer()?;
    let bounds = graphene::Rect::new(0.0, 0.0, width, height);
    Some(renderer.render_texture(&node, Some(&bounds)))
}

/// Draw `widget` with its top left corner at `at`.
fn paint_at(snapshot: &gtk::Snapshot, widget: &gtk::Widget, (x, y): (f64, f64), opacity: f64) {
    snapshot.save();
    snapshot.translate(&graphene::Point::new(x as f32, y as f32));
    snapshot.push_opacity(opacity);
    gtk::WidgetPaintable::new(Some(widget)).snapshot(
        snapshot,
        widget.width().into(),
        widget.height().into(),
    );
    snapshot.pop();
    snapshot.restore();
}

/// The pointer: a white arrow with a dark outline, its tip at (`x`, `y`); a click adds a
/// blue ring.
fn draw_pointer(snapshot: &gtk::Snapshot, (x, y): (f64, f64), clicking: bool) {
    let bounds = graphene::Rect::new(x as f32 - 16.0, y as f32 - 16.0, 40.0, 44.0);
    let cairo = snapshot.append_cairo(&bounds);
    if clicking {
        cairo.arc(x, y, 12.0, 0.0, std::f64::consts::TAU);
        // The guide's accent blue (#245edc), see docs/help/style.css.
        cairo.set_source_rgba(0.141, 0.369, 0.863, 0.35);
        let _ = cairo.fill();
    }
    for (index, (dx, dy)) in ARROW.into_iter().enumerate() {
        if index == 0 {
            cairo.move_to(x + dx, y + dy);
        } else {
            cairo.line_to(x + dx, y + dy);
        }
    }
    cairo.close_path();
    cairo.set_source_rgb(1.0, 1.0, 1.0);
    let _ = cairo.fill_preserve();
    cairo.set_source_rgb(0.192, 0.176, 0.165);
    cairo.set_line_width(1.3);
    let _ = cairo.stroke();
}
