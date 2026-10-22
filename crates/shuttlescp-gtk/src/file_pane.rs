//! A file explorer pane: refresh button, disk drop-down, path label and a sortable column
//! view whose first row, "..", goes up one folder. Right-clicking offers to copy, move,
//! rename or delete the selected items, or to create a folder. Items can be dragged onto
//! a folder in the same pane or into the other pane (see [`drag`]); dropping them asks
//! whether to copy or move. Dragging with the right button selects a range of rows.

mod drag;

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::iter;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use relm4::gtk::prelude::*;
use relm4::gtk::{gdk, gio, glib, graphene};
use relm4::prelude::*;
use relm4::typed_view::OrdFn;
use relm4::typed_view::column::{RelmColumn, TypedColumnView};

use shuttlescp_core::disks::{self, Disk};
use shuttlescp_core::entry::{EntryKind, FileEntry};
use shuttlescp_core::format::{format_modified, format_size};
use shuttlescp_core::local_fs;
use shuttlescp_core::path::SidePath;
use shuttlescp_core::remote::Remote;
use shuttlescp_core::transfer::{self, JobKind, PARTIAL_SUFFIX, Side};

use crate::dialog::{self, Dialog};
use crate::disk_picker::DiskPicker;
use crate::name_dialog::{self, NamePrompt};

/// Up-arrow picture drawn over the folder icon of the ".." row, embedded at compile time.
const UP_ARROW_SVG: &[u8] = include_bytes!("icons/up-arrow.svg");
/// Displayed size of the up arrow, in pixels (the folder icon under it is 16px).
const UP_ARROW_SIZE: i32 = 10;
/// Theme icon for an ordinary folder.
const FOLDER_ICON: &str = "folder";
/// Theme icons the desktop's file manager uses for the XDG special folders.
const SPECIAL_FOLDER_ICONS: [(glib::UserDirectory, &str); 8] = [
    (glib::UserDirectory::Desktop, "user-desktop"),
    (glib::UserDirectory::Documents, "folder-documents"),
    (glib::UserDirectory::Downloads, "folder-download"),
    (glib::UserDirectory::Music, "folder-music"),
    (glib::UserDirectory::Pictures, "folder-pictures"),
    (glib::UserDirectory::PublicShare, "folder-publicshare"),
    (glib::UserDirectory::Templates, "folder-templates"),
    (glib::UserDirectory::Videos, "folder-videos"),
];
/// Theme icon for the user's home folder.
const HOME_FOLDER_ICON: &str = "user-home";
/// Name of the pane's action group: its actions are "pane.copy" and so on.
const ACTION_GROUP: &str = "pane";
/// Action a right-click on a row triggers, with the row's position and the click's
/// coordinates in the column view.
const CONTEXT_MENU_ACTION: &str = "context-menu";
/// CSS class marking the cells of the file list, to tell a right-click on a row from
/// one on the empty space below the rows.
const CELL_CLASS: &str = "pane-cell";
/// Name of the drop menu's actions: "pane.drop-copy" and so on.
const DROP_COPY_ACTION: &str = "drop-copy";
const DROP_MOVE_ACTION: &str = "drop-move";
const DROP_CANCEL_ACTION: &str = "drop-cancel";
/// CSS class of the text of an item a running transfer uses (see style.css).
const IN_USE_CLASS: &str = "in-transfer";
/// Name suggested for a new folder.
const NEW_FOLDER_NAME: &str = "New Folder";

/// CSS class of the parts of a row that drag its files: the icon and the name in the
/// Name column. Pressing anywhere else in the list starts a selection box instead.
const DRAG_HANDLE_CLASS: &str = "drag-handle";
/// How far (in pixels) the pointer must move with the button held before a selection
/// box starts (less is a click).
const BOX_SELECT_THRESHOLD: f64 = 4.0;
/// While a selection box is drawn, the list scrolls when the pointer is this close to
/// its top or bottom edge (or beyond it); the column titles are within the top margin.
const AUTOSCROLL_EDGE: f64 = 28.0;
/// How often the list scrolls while the pointer stays near an edge.
const AUTOSCROLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(30);
/// The most it scrolls per step, in pixels (the further past the edge, the faster).
const AUTOSCROLL_MAX_STEP: f64 = 40.0;

thread_local! {
    // The list item of each cell, by the cell's address, so a point in the list can be
    // traced to its row (GTK does not say which row a widget shows).
    static CELL_ITEMS: RefCell<HashMap<usize, glib::WeakRef<gtk::ListItem>>> =
        RefCell::default();
    // Decoded once and shared by every ".." row. `thread_local!` because GTK objects live
    // on the UI thread only.
    static UP_ARROW: Option<gdk::Texture> =
        gdk::Texture::from_bytes(&glib::Bytes::from_static(UP_ARROW_SVG))
            .inspect_err(|error| tracing::warn!("cannot load up-arrow icon: {error}"))
            .ok();
}

/// Messages that drive a [`FilePane`].
#[derive(Debug)]
pub enum FilePaneMsg {
    /// Re-read the current directory.
    Refresh,
    /// A row was double-clicked or had Enter pressed on it (position in the sorted view).
    Activate(u32),
    /// The user picked the disk at this position in the disk drop-down.
    DiskSelected(u32),
    /// A disk was mounted, unmounted or renamed.
    DisksChanged,
    /// A row (or the empty space below the rows) was right-clicked: show the context
    /// menu at (`x`, `y`) in the column view.
    ContextMenu {
        /// Position of the row in the sorted view; `None` for the empty space.
        position: Option<u32>,
        /// Click position in the column view.
        x: f64,
        /// Click position in the column view.
        y: f64,
    },
    /// Copy, move or delete the selected items (from the context menu or a key).
    Request(JobKind),
    /// Whether there is somewhere to copy to (for the local pane: a connected server).
    PeerAvailable(bool),
    /// Ask for a name and create a folder in the current folder.
    NewFolder,
    /// Ask for a new name for the selected item.
    Rename,
    /// Create a folder with this name in the current folder.
    CreateFolder(String),
    /// Rename an item.
    RenameTo {
        /// The item.
        path: SidePath,
        /// Its new name.
        name: String,
    },
    /// Items were dragged and dropped on this pane: ask whether to copy or move them.
    Dropped(Dropped),
    /// The answer from the drop menu: copy or move the dropped items, or (`None`) leave
    /// them.
    DropChosen(Option<JobKind>),
    /// These files and folders (and everything inside them) are used by running
    /// transfers: show them as busy until the next update.
    InUse(Vec<SidePath>),
}

/// Files and folders dropped on a pane.
#[derive(Debug)]
pub struct Dropped {
    /// The dragged files and folders.
    sources: Vec<SidePath>,
    /// They were dragged from this pane, not from the other one.
    from_this_pane: bool,
    /// The folder they were dropped on; `None` for the folder the pane shows.
    dest_dir: Option<SidePath>,
    /// Drop position in the column view, where the menu opens.
    x: f64,
    /// Drop position in the column view.
    y: f64,
}

/// A drop waiting for the drop menu's answer.
#[derive(Debug)]
struct PendingDrop {
    sources: Vec<SidePath>,
    from_this_pane: bool,
    dest_dir: SidePath,
}

/// What a [`FilePane`] starts with.
#[derive(Debug)]
pub struct FilePaneInit {
    /// Directory shown first.
    pub start_dir: SidePath,
    /// Last directory shown on each disk in earlier sessions, keyed by mount point.
    pub last_dir_on_disk: BTreeMap<PathBuf, PathBuf>,
    /// Browse this server instead of the local disks.
    pub remote: Option<Remote>,
}

/// Notifications a [`FilePane`] sends to its parent.
#[derive(Debug)]
pub enum FilePaneOutput {
    /// The pane now shows `dir`, which lies on the disk mounted at `disk_root`
    /// (`None` if no known disk holds it).
    DirectoryChanged {
        /// Directory now shown.
        dir: SidePath,
        /// Mount point of the disk holding `dir`.
        disk_root: Option<PathBuf>,
    },
    /// The user asked to copy, move or delete these items (full paths).
    Request {
        /// What to do.
        kind: JobKind,
        /// The selected files and folders.
        paths: Vec<SidePath>,
    },
    /// Items were dropped on this pane, and the user chose to copy or move them.
    Dropped {
        /// Copy or move.
        kind: JobKind,
        /// The dropped files and folders (full paths).
        paths: Vec<SidePath>,
        /// They come from this pane, so they stay on its side; otherwise they come
        /// from the other pane.
        from_this_pane: bool,
        /// The folder of this pane they go into.
        dest_dir: SidePath,
    },
}

/// Results of background work, delivered back to the UI thread.
#[derive(Debug)]
pub enum PaneCommand {
    /// A directory listing.
    Listing(Listing),
    /// A folder was created or an item renamed: what happened, or what went wrong.
    Operation(Result<String, String>),
    /// The server's drives, for the drive drop-down.
    Drives(Vec<Disk>),
}

/// Result of a background directory listing.
#[derive(Debug)]
pub struct Listing {
    dir: SidePath,
    /// The entries, or a message for the status line.
    result: Result<Vec<FileEntry>, String>,
}

/// Items running transfers use, drawn in red until they finish. Shared by a pane and
/// its rows, so the visible cells can be restyled without rebuilding the list.
#[derive(Default)]
struct InUseMarks {
    /// Paths in use; everything inside them is in use too.
    paths: RefCell<Vec<SidePath>>,
    /// The text labels of the cells on screen and the path each shows, by label.
    labels: RefCell<HashMap<usize, (glib::WeakRef<gtk::Label>, SidePath)>>,
}

impl InUseMarks {
    /// Whether a transfer uses `path` (or `path` is the partial copy of one in use).
    fn is_in_use(&self, path: &SidePath) -> bool {
        let copy = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(PARTIAL_SUFFIX))
            .map(|name| path.with_file_name(name));
        let path = copy.as_ref().unwrap_or(path);
        self.paths
            .borrow()
            .iter()
            .any(|used| path.starts_with(used))
    }

    /// A cell's label now shows `path` (`None` for the ".." row): style it, and restyle
    /// it when the paths in use change.
    fn bind(&self, label: &gtk::Label, path: Option<&SidePath>) {
        let key = label.as_ptr() as usize;
        match path {
            Some(path) => {
                self.style(label, path);
                self.labels
                    .borrow_mut()
                    .insert(key, (label.downgrade(), path.clone()));
            }
            None => {
                label.remove_css_class(IN_USE_CLASS);
                self.labels.borrow_mut().remove(&key);
            }
        }
    }

    /// Replace the paths in use and restyle the cells on screen.
    fn set(&self, paths: Vec<SidePath>) {
        self.paths.replace(paths);
        self.labels
            .borrow_mut()
            .retain(|_, (label, path)| match label.upgrade() {
                Some(label) => {
                    self.style(&label, path);
                    true
                }
                None => false,
            });
    }

    fn style(&self, label: &gtk::Label, path: &SidePath) {
        if self.is_in_use(path) {
            label.add_css_class(IN_USE_CLASS);
        } else {
            label.remove_css_class(IN_USE_CLASS);
        }
    }
}

// Written by hand: the widgets inside have nothing useful to show.
impl std::fmt::Debug for InUseMarks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InUseMarks")
            .field("paths", &self.paths.borrow())
            .finish_non_exhaustive()
    }
}

/// One line of the file list.
#[derive(Debug)]
enum Row {
    /// The ".." line that goes up one folder; always shown first.
    Parent {
        /// The folder it goes up to.
        path: SidePath,
    },
    /// A file or folder inside the current directory.
    Entry {
        entry: FileEntry,
        /// Its full path.
        path: SidePath,
        /// What running transfers use, to draw this row in red while it is in use.
        in_use: Rc<InUseMarks>,
        /// Theme icon for special folders such as Documents; `None` for everything else.
        special_icon: Option<&'static str>,
    },
}

/// File explorer component, for the local disks or a remote server.
pub struct FilePane {
    /// Directory whose contents are currently displayed.
    current_dir: SidePath,
    /// Most recently requested directory; listings for anything else are stale.
    requested_dir: SidePath,
    entries: TypedColumnView<Row, gtk::MultiSelection>,
    status: String,
    disk_picker: DiskPicker,
    /// Last directory shown on each disk, keyed by the disk's mount point, so returning
    /// to a disk resumes where the user left it.
    last_dir_on_disk: BTreeMap<PathBuf, PathBuf>,
    /// The server this pane browses; `None` for the local disks.
    remote: Option<Remote>,
    /// Right-click menu of the selected items.
    context_menu: gtk::PopoverMenu,
    /// Menu shown after a drop: copy or move?
    drop_menu: gtk::PopoverMenu,
    /// The drop the drop menu is about.
    pending_drop: Option<PendingDrop>,
    /// The menu's actions, enabled to suit the selection before it opens.
    actions: PaneActions,
    /// Whether there is somewhere to copy to (for the local pane: a connected server).
    peer_available: bool,
    /// What the last operation did, shown with the next listing's item count.
    notice: Option<String>,
    /// Items running transfers use; shared with the rows.
    in_use: Rc<InUseMarks>,
}

/// The actions of the right-click menu.
struct PaneActions {
    copy: gio::SimpleAction,
    move_: gio::SimpleAction,
    delete: gio::SimpleAction,
    rename: gio::SimpleAction,
}

impl FilePane {
    /// Start listing `dir` in the background so a slow disk or network never freezes
    /// the UI: on a worker thread for local folders, as an async SFTP request for remote.
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory to list
    /// * `sender` - Component sender used to spawn the work and receive its result
    fn request_listing(&mut self, dir: SidePath, sender: &ComponentSender<Self>) {
        tracing::debug!("listing {dir}");
        self.requested_dir = dir.clone();
        self.status = String::from("Loading...");
        match (&dir, self.remote.clone()) {
            (SidePath::Remote(remote_dir), Some(remote)) => {
                let remote_dir = remote_dir.clone();
                sender.oneshot_command(async move {
                    PaneCommand::Listing(Listing {
                        result: remote.list_dir(&remote_dir).await,
                        dir,
                    })
                });
            }
            // `move` transfers ownership of `dir` into the closure, which runs on another
            // thread.
            (SidePath::Local(local_dir), None) => {
                let local_dir = local_dir.clone();
                sender.spawn_oneshot_command(move || {
                    PaneCommand::Listing(Listing {
                        result: local_fs::list_dir(&local_dir).map_err(|error| error.to_string()),
                        dir,
                    })
                });
            }
            // A pane only ever browses its own side.
            _ => tracing::error!("{dir} is not on this pane's side"),
        }
    }

    /// Enable the menu's actions to suit the selection: copying needs a selection and
    /// somewhere to copy to, renaming exactly one selected item.
    fn update_actions(&self) {
        let selected = self.selected_paths().len();
        self.actions
            .copy
            .set_enabled(self.peer_available && selected > 0);
        self.actions
            .move_
            .set_enabled(self.peer_available && selected > 0);
        self.actions.delete.set_enabled(selected > 0);
        self.actions.rename.set_enabled(selected == 1);
    }

    /// Ask for a name over the pane's window, then send the message `on_name` makes.
    fn ask_name(
        root: &gtk::Box,
        wording: NamePrompt<'_>,
        sender: &ComponentSender<Self>,
        on_name: impl Fn(String) -> FilePaneMsg + 'static,
    ) {
        let Some(window) = root.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let sender = sender.clone();
        name_dialog::ask(&window, wording, move |name| sender.input(on_name(name)));
    }

    /// Open a local file with the system's default app for its type, as double-clicking
    /// it in the file manager would (xdg-open on Linux, Finder on macOS, Explorer on
    /// Windows). If that fails, e.g. no app handles the type, the user is told.
    fn open_local_file(root: &gtk::Box, file: &Path) {
        let uri = gio::File::for_path(file).uri();
        tracing::info!("opening {} with its default app", file.display());
        // The launch context lets the desktop show the app starting and focus it.
        let context = root.display().app_launch_context();
        let (root, file) = (root.clone(), file.to_path_buf());
        gio::AppInfo::launch_default_for_uri_async(
            &uri,
            Some(&context),
            None::<&gio::Cancellable>,
            move |result| {
                let Err(error) = result else {
                    return;
                };
                tracing::warn!("cannot open {}: {error}", file.display());
                let Some(window) = root.root().and_downcast::<gtk::Window>() else {
                    return;
                };
                let name = file.file_name().map_or_else(
                    || file.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                let dialog = Dialog::new(&window, "Open File");
                dialog
                    .content
                    .append(&dialog::text(&format!("Cannot open {name}"), &["heading"]));
                dialog.content.append(&dialog::text(
                    &format!("{}.", error.message().trim_end_matches('.')),
                    &[],
                ));
                let close = dialog.add_close_button("Close");
                dialog.window.present();
                close.grab_focus();
            },
        );
    }

    /// Full paths of the selected files and folders ("..", if selected, is left out).
    fn selected_paths(&self) -> Vec<SidePath> {
        let selection = &self.entries.selection_model;
        (0..selection.n_items())
            .filter(|&position| selection.is_selected(position))
            .filter_map(|position| self.entries.get_visible(position))
            .filter_map(|item| match &*item.borrow() {
                Row::Parent { .. } => None,
                Row::Entry { path, .. } => Some(path.clone()),
            })
            .collect()
    }

    /// Replace the list contents with a fresh listing of `dir`.
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory the entries belong to
    /// * `entries` - Its contents, as returned by [`local_fs::list_dir`]
    fn show_entries(&mut self, dir: &SidePath, entries: Vec<FileEntry>) {
        // Special folder icons describe this computer's folders, not the server's.
        let special_folders = dir.as_local().map(special_folder_icons).unwrap_or_default();
        self.entries.clear();
        if let Some(parent) = dir.parent() {
            self.entries.append(Row::Parent { path: parent });
        }
        self.entries
            .extend_from_iter(entries.into_iter().map(|entry| {
                let special_icon = match entry.kind() {
                    EntryKind::Directory => special_folders
                        .iter()
                        .find(|(name, _)| name == entry.name())
                        .map(|&(_, icon)| icon),
                    EntryKind::File => None,
                };
                Row::Entry {
                    path: dir.join(entry.name()),
                    in_use: Rc::clone(&self.in_use),
                    entry,
                    special_icon,
                }
            }));
    }

    /// Select the rows showing these paths again (after the list was rebuilt).
    fn reselect(&self, paths: &[SidePath]) {
        if paths.is_empty() {
            return;
        }
        let selection = &self.entries.selection_model;
        for position in 0..selection.n_items() {
            let selected =
                self.entries
                    .get_visible(position)
                    .is_some_and(|item| match &*item.borrow() {
                        Row::Entry { path, .. } => paths.contains(path),
                        Row::Parent { .. } => false,
                    });
            if selected {
                selection.select_item(position, false);
            }
        }
    }

    /// Open `menu` at (`x`, `y`) in the column view.
    fn popup_at(&self, menu: &gtk::PopoverMenu, root: &gtk::Box, x: f64, y: f64) {
        let point = self
            .entries
            .view
            .compute_point(root, &graphene::Point::new(x as f32, y as f32))
            .unwrap_or_else(|| graphene::Point::new(x as f32, y as f32));
        menu.set_pointing_to(Some(&gdk::Rectangle::new(
            point.x() as i32,
            point.y() as i32,
            1,
            1,
        )));
        menu.popup();
    }
}

// `#[relm4::component(pub)]` expands the `view!` block below into widget construction code
// and a struct holding the widgets, roughly like a declarative template.
#[relm4::component(pub)]
impl Component for FilePane {
    type Init = FilePaneInit;
    type Input = FilePaneMsg;
    type Output = FilePaneOutput;
    type CommandOutput = PaneCommand;

    view! {
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_spacing: 2,
            set_margin_all: 2,

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 2,
                // Sized down in style.css.
                add_css_class: "compact-toolbar",

                gtk::Button {
                    set_has_frame: false,
                    set_icon_name: "view-refresh-symbolic",
                    set_tooltip_text: Some("Refresh"),
                    connect_clicked => FilePaneMsg::Refresh,
                },
                // `#[local_ref]` embeds a widget created in `init` instead of building a new one.
                #[local_ref]
                disk_dropdown -> gtk::DropDown {},
                // Current directory. A plain label: it cannot be clicked, focused or edited.
                gtk::Label {
                    set_hexpand: true,
                    set_xalign: 0.0,
                    // Long paths lose their start, keeping the current folder name visible.
                    set_ellipsize: gtk::pango::EllipsizeMode::Start,
                    add_css_class: "path-label",
                    // `#[watch]` re-evaluates the property after every update.
                    #[watch]
                    set_label: &model.current_dir.to_string(),
                    #[watch]
                    set_tooltip_text: Some(&model.current_dir.to_string()),
                },
            },

            // The list, with the selection box drawn over it.
            gtk::Overlay {
                set_vexpand: true,
                add_overlay: &selection_box,

                #[wrap(Some)]
                set_child = &gtk::ScrolledWindow {
                    set_vexpand: true,

                    #[local_ref]
                    entries_view -> gtk::ColumnView {
                        // GTK themes style "data-table" lists with short rows.
                        add_css_class: "data-table",
                        // Lines between columns, drawn dotted by style.css.
                        set_show_column_separators: true,
                        connect_activate[sender] => move |_, position| {
                            sender.input(FilePaneMsg::Activate(position));
                        },
                    },
                },
            },

            gtk::Label {
                set_xalign: 0.0,
                set_margin_start: 4,
                // Without ellipsizing, a long error message would raise the window's
                // minimum width and stop it from being resized smaller.
                set_ellipsize: gtk::pango::EllipsizeMode::End,
                #[watch]
                set_label: &model.status,
                #[watch]
                set_tooltip_text: Some(&model.status),
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let FilePaneInit {
            start_dir,
            last_dir_on_disk,
            remote,
        } = init;
        let mut entries = TypedColumnView::<Row, gtk::MultiSelection>::new();
        entries.append_column::<NameColumn>();
        entries.append_column::<SizeColumn>();
        entries.append_column::<ModifiedColumn>();
        pin_parent_row(&entries);

        // Each callback owns its own sender clone, since both outlive this function.
        let select_sender = sender.clone();
        let on_select = move |position| select_sender.input(FilePaneMsg::DiskSelected(position));
        let disk_picker = match &remote {
            // A server's drives arrive once it has been asked (see below).
            Some(_) => DiskPicker::for_server(on_select),
            None => {
                let mounts_sender = sender.clone();
                DiskPicker::new(on_select, move || {
                    mounts_sender.input(FilePaneMsg::DisksChanged);
                })
            }
        };
        disk_picker.show_dir(&start_dir);
        if let Some(remote) = remote.clone() {
            sender.oneshot_command(async move { PaneCommand::Drives(remote.drives().await) });
        }

        let side = if remote.is_some() {
            Side::Remote
        } else {
            Side::Local
        };
        let (context_menu, drop_menu, actions) = context_menu(&root, side, &sender);
        let keys = gtk::EventControllerKey::new();
        {
            let sender = sender.clone();
            keys.connect_key_pressed(move |_, key, _, _| {
                let message = match key {
                    gdk::Key::Delete => FilePaneMsg::Request(JobKind::Delete),
                    gdk::Key::F2 => FilePaneMsg::Rename,
                    _ => return glib::Propagation::Proceed,
                };
                sender.input(message);
                glib::Propagation::Stop
            });
        }
        entries.view.add_controller(keys);
        attach_empty_space_click(&entries.view, &sender);
        // The selection box, drawn over the list while the user drags one out.
        let selection_box = gtk::Box::builder()
            .css_classes(["selection-box"])
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .can_target(false)
            .visible(false)
            .build();
        attach_box_select(&entries.view, &selection_box);
        {
            let sender = sender.clone();
            drag::attach_list_drop(&entries.view, move |dropped| {
                sender.input(FilePaneMsg::Dropped(dropped));
            });
        }

        let mut model = Self {
            current_dir: start_dir.clone(),
            requested_dir: start_dir.clone(),
            entries,
            status: String::new(),
            disk_picker,
            last_dir_on_disk,
            remote,
            context_menu,
            drop_menu,
            pending_drop: None,
            in_use: Rc::default(),
            actions,
            // The local pane has nowhere to copy to until a server is connected.
            peer_available: side == Side::Remote,
            notice: None,
        };
        model.request_listing(start_dir, &sender);

        let entries_view = &model.entries.view;
        let disk_dropdown = model.disk_picker.widget();
        // `view_output!()` is generated by the macro; it builds the widget tree declared above.
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match message {
            FilePaneMsg::Refresh => self.request_listing(self.current_dir.clone(), &sender),
            FilePaneMsg::Activate(position) => {
                // Build the target first: `item.borrow()` holds a borrow that must end
                // before `self` is mutably borrowed by `request_listing`.
                let target = self
                    .entries
                    .get_visible(position)
                    .map(|item| match &*item.borrow() {
                        Row::Parent { path } => (path.clone(), EntryKind::Directory),
                        Row::Entry { entry, path, .. } => (path.clone(), entry.kind()),
                    });
                match target {
                    Some((dir, EntryKind::Directory)) => self.request_listing(dir, &sender),
                    // A local file opens in the app the system uses for it; a server's
                    // file would need downloading first, so nothing happens there.
                    Some((SidePath::Local(file), EntryKind::File)) => {
                        Self::open_local_file(root, &file);
                    }
                    Some((SidePath::Remote(_), EntryKind::File)) | None => {}
                }
            }
            FilePaneMsg::DiskSelected(position) => {
                let Some(root) = usize::try_from(position)
                    .ok()
                    .and_then(|index| self.disk_picker.disks().get(index))
                    .map(|disk| disk.root().clone())
                else {
                    return;
                };
                // Resume at the last folder seen on this disk if it (or a parent on the
                // same disk) still exists, otherwise start at the disk's top. A server's
                // drive opens at its top.
                let target = match &root {
                    SidePath::Remote(_) => root.clone(),
                    SidePath::Local(local_root) => self
                        .last_dir_on_disk
                        .get(local_root)
                        .and_then(|dir| local_fs::nearest_listable_dir(dir))
                        .filter(|dir| dir.starts_with(local_root))
                        .map_or(root.clone(), |dir| SidePath::Local(dir.to_path_buf())),
                };
                tracing::info!("switching to disk {root} at {target}");
                self.request_listing(target, &sender);
            }
            FilePaneMsg::ContextMenu { position, x, y } => {
                let selection = &self.entries.selection_model;
                match position {
                    Some(position) => {
                        let is_entry = self
                            .entries
                            .get_visible(position)
                            .is_some_and(|item| matches!(&*item.borrow(), Row::Entry { .. }));
                        if !is_entry {
                            return;
                        }
                        // Right-clicking outside the selection selects just that row, as
                        // file managers do; inside it, the whole selection is kept.
                        if !selection.is_selected(position) {
                            selection.select_item(position, true);
                        }
                    }
                    // The empty space below the rows: a menu for the folder itself.
                    None => {
                        selection.unselect_all();
                    }
                }
                self.update_actions();
                self.popup_at(&self.context_menu, root, x, y);
            }
            FilePaneMsg::Dropped(dropped) => {
                let dest_dir = dropped.dest_dir.unwrap_or_else(|| self.current_dir.clone());
                if dropped.from_this_pane
                    && let Err(error) = transfer::check_same_side(&dropped.sources, &dest_dir)
                {
                    tracing::debug!("drop refused: {error}");
                    self.status = error;
                    return;
                }
                tracing::debug!("{} items dropped on {dest_dir}", dropped.sources.len());
                self.pending_drop = Some(PendingDrop {
                    sources: dropped.sources,
                    from_this_pane: dropped.from_this_pane,
                    dest_dir,
                });
                self.popup_at(&self.drop_menu, root, dropped.x, dropped.y);
            }
            FilePaneMsg::InUse(paths) => self.in_use.set(paths),
            FilePaneMsg::DropChosen(kind) => {
                let Some(drop) = self.pending_drop.take() else {
                    return;
                };
                let Some(kind) = kind else {
                    tracing::debug!("drop cancelled");
                    return;
                };
                let _ = sender.output(FilePaneOutput::Dropped {
                    kind,
                    paths: drop.sources,
                    from_this_pane: drop.from_this_pane,
                    dest_dir: drop.dest_dir,
                });
            }
            FilePaneMsg::Request(kind) => {
                let paths = self.selected_paths();
                let allowed = kind == JobKind::Delete || self.peer_available;
                if paths.is_empty() || !allowed {
                    return;
                }
                tracing::debug!("{kind:?} requested for {} items", paths.len());
                let _ = sender.output(FilePaneOutput::Request { kind, paths });
            }
            FilePaneMsg::PeerAvailable(available) => self.peer_available = available,
            FilePaneMsg::NewFolder => Self::ask_name(
                root,
                NamePrompt {
                    title: "New Folder",
                    prompt: &format!("Create a folder in {}:", self.current_dir),
                    initial: NEW_FOLDER_NAME,
                    confirm_label: "Create",
                    side: self.current_dir.side(),
                },
                &sender,
                FilePaneMsg::CreateFolder,
            ),
            FilePaneMsg::Rename => {
                let paths = self.selected_paths();
                let [path] = paths.as_slice() else {
                    return;
                };
                let path = path.clone();
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Self::ask_name(
                    root,
                    NamePrompt {
                        title: "Rename",
                        prompt: &format!("New name for {name}:"),
                        initial: &name,
                        confirm_label: "Rename",
                        side: path.side(),
                    },
                    &sender,
                    move |new_name| FilePaneMsg::RenameTo {
                        path: path.clone(),
                        name: new_name,
                    },
                );
            }
            FilePaneMsg::CreateFolder(name) => {
                let (remote, dir) = (self.remote.clone(), self.current_dir.clone());
                sender.oneshot_command(async move {
                    PaneCommand::Operation(
                        transfer::create_folder(remote.as_ref(), &dir, &name)
                            .await
                            .map(|_| format!("Created folder {name}")),
                    )
                });
            }
            FilePaneMsg::RenameTo { path, name } => {
                let remote = self.remote.clone();
                sender.oneshot_command(async move {
                    PaneCommand::Operation(
                        transfer::rename(remote.as_ref(), &path, &name)
                            .await
                            .map(|_| format!("Renamed to {name}")),
                    )
                });
            }
            FilePaneMsg::DisksChanged if self.remote.is_some() => {}
            FilePaneMsg::DisksChanged => {
                self.disk_picker.reload();
                self.disk_picker.show_dir(&self.current_dir);
                // If the disk being shown was removed, fall back to the nearest folder that
                // still exists (ultimately "/").
                let fallback = self
                    .current_dir
                    .as_local()
                    .and_then(local_fs::nearest_listable_dir)
                    .map(|dir| SidePath::Local(dir.to_path_buf()));
                if let Some(fallback) = fallback
                    && fallback != self.current_dir
                {
                    tracing::info!("{} is gone; showing {fallback}", self.current_dir);
                    self.request_listing(fallback, &sender);
                }
            }
        }
    }

    fn update_cmd(
        &mut self,
        command: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        let listing = match command {
            PaneCommand::Listing(listing) => listing,
            PaneCommand::Operation(Ok(notice)) => {
                self.notice = Some(notice);
                self.request_listing(self.current_dir.clone(), &sender);
                return;
            }
            PaneCommand::Operation(Err(error)) => {
                tracing::warn!("{error}");
                self.status = error;
                return;
            }
            PaneCommand::Drives(drives) => {
                self.disk_picker.set_disks(drives);
                self.disk_picker.show_dir(&self.current_dir);
                return;
            }
        };
        // A newer navigation superseded this listing before it finished.
        if listing.dir != self.requested_dir {
            tracing::trace!("dropping stale listing of {}", listing.dir);
            return;
        }
        match listing.result {
            Ok(entries) => {
                let count = match entries.len() {
                    1 => String::from("1 item"),
                    count => format!("{count} items"),
                };
                self.status = match self.notice.take() {
                    Some(notice) => format!("{notice} · {count}"),
                    None => count,
                };
                // A refresh of the same folder keeps the selection and scroll position.
                let kept = (listing.dir == self.current_dir).then(|| {
                    (
                        self.selected_paths(),
                        self.entries.view.vadjustment().map(|scroll| scroll.value()),
                    )
                });
                self.show_entries(&listing.dir, entries);
                if let Some((selected, scrolled)) = kept {
                    self.reselect(&selected);
                    if let (Some(value), Some(scroll)) = (scrolled, self.entries.view.vadjustment())
                    {
                        // After the next layout, once the list has its new height.
                        glib::idle_add_local_once(move || scroll.set_value(value));
                    }
                }
                // Only local disks remember their last folder.
                let disk_root = disks::disk_index(self.disk_picker.disks(), &listing.dir)
                    .and_then(|index| self.disk_picker.disks().get(index))
                    .and_then(|disk| disk.root().as_local())
                    .map(Path::to_path_buf);
                if let (Some(root), SidePath::Local(dir)) = (&disk_root, &listing.dir) {
                    self.last_dir_on_disk.insert(root.clone(), dir.clone());
                }
                // Sending fails only when the parent is already gone, so there is no one
                // left to notify; `let _ =` explicitly discards that result.
                let _ = sender.output(FilePaneOutput::DirectoryChanged {
                    dir: listing.dir.clone(),
                    disk_root,
                });
                self.current_dir = listing.dir;
            }
            Err(error) => {
                tracing::warn!("{error}");
                self.status = error;
                self.requested_dir = self.current_dir.clone();
            }
        }
        // Also undoes a disk choice whose listing failed.
        self.disk_picker.show_dir(&self.current_dir);
    }
}

/// Build the right-click menu and the actions behind it, attached to the pane.
///
/// # Arguments
///
/// * `root` - The pane's root widget, which owns the actions and the menu
/// * `side` - Which side the pane shows, for the wording
/// * `sender` - Receives the chosen action
///
/// # Returns
///
/// The right-click menu, the menu shown after a drop, and the right-click menu's
/// actions (to enable or disable)
fn context_menu(
    root: &gtk::Box,
    side: Side,
    sender: &ComponentSender<FilePane>,
) -> (gtk::PopoverMenu, gtk::PopoverMenu, PaneActions) {
    let group = gio::SimpleActionGroup::new();
    let add = |name: &str, message: fn() -> FilePaneMsg| {
        let action = gio::SimpleAction::new(name, None);
        let sender = sender.clone();
        action.connect_activate(move |_, _| sender.input(message()));
        group.add_action(&action);
        action
    };
    let actions = PaneActions {
        copy: add("copy", || FilePaneMsg::Request(JobKind::Copy)),
        move_: add("move", || FilePaneMsg::Request(JobKind::Move)),
        delete: add("delete", || FilePaneMsg::Request(JobKind::Delete)),
        rename: add("rename", || FilePaneMsg::Rename),
    };
    add("new-folder", || FilePaneMsg::NewFolder);
    add("refresh", || FilePaneMsg::Refresh);
    add(DROP_COPY_ACTION, || {
        FilePaneMsg::DropChosen(Some(JobKind::Copy))
    });
    add(DROP_MOVE_ACTION, || {
        FilePaneMsg::DropChosen(Some(JobKind::Move))
    });
    add(DROP_CANCEL_ACTION, || FilePaneMsg::DropChosen(None));

    // Triggered by the right-click handlers of the cells (see `attach_context_click`).
    let menu_action = gio::SimpleAction::new(
        CONTEXT_MENU_ACTION,
        Some(&<(u32, f64, f64)>::static_variant_type()),
    );
    {
        let sender = sender.clone();
        menu_action.connect_activate(move |_, parameter| {
            if let Some((position, x, y)) =
                parameter.and_then(|value| value.get::<(u32, f64, f64)>())
            {
                sender.input(FilePaneMsg::ContextMenu {
                    position: Some(position),
                    x,
                    y,
                });
            }
        });
    }
    group.add_action(&menu_action);

    // Triggered by the drop targets of folder rows (see `drag::attach_folder_drop`).
    let drop_action = gio::SimpleAction::new(
        drag::DROP_ACTION,
        Some(&<drag::DropVariant>::static_variant_type()),
    );
    {
        let sender = sender.clone();
        drop_action.connect_activate(move |_, parameter| {
            if let Some(dropped) = parameter.and_then(drag::dropped_from) {
                sender.input(FilePaneMsg::Dropped(dropped));
            }
        });
    }
    group.add_action(&drop_action);
    root.insert_action_group(ACTION_GROUP, Some(&group));

    let (copy_label, move_label) = match side {
        Side::Local => ("_Upload", "_Move"),
        Side::Remote => ("_Download", "_Move"),
    };
    let transfer_section = gio::Menu::new();
    transfer_section.append(Some(copy_label), Some("pane.copy"));
    transfer_section.append(Some(move_label), Some("pane.move"));
    let edit_section = gio::Menu::new();
    edit_section.append(Some("_New Folder…"), Some("pane.new-folder"));
    edit_section.append(Some("_Rename…"), Some("pane.rename"));
    let delete_section = gio::Menu::new();
    delete_section.append(Some("_Delete"), Some("pane.delete"));
    let menu = gio::Menu::new();
    menu.append_section(None, &transfer_section);
    menu.append_section(None, &edit_section);
    let refresh_section = gio::Menu::new();
    refresh_section.append(Some("Re_fresh"), Some("pane.refresh"));
    menu.append_section(None, &refresh_section);
    // The one destructive item comes last, away from the others.
    menu.append_section(None, &delete_section);

    let choice_section = gio::Menu::new();
    choice_section.append(
        Some("_Copy Here"),
        Some(&format!("{ACTION_GROUP}.{DROP_COPY_ACTION}")),
    );
    choice_section.append(
        Some("_Move Here"),
        Some(&format!("{ACTION_GROUP}.{DROP_MOVE_ACTION}")),
    );
    let cancel_section = gio::Menu::new();
    cancel_section.append(
        Some("C_ancel"),
        Some(&format!("{ACTION_GROUP}.{DROP_CANCEL_ACTION}")),
    );
    let drop_menu = gio::Menu::new();
    drop_menu.append_section(None, &choice_section);
    drop_menu.append_section(None, &cancel_section);

    (
        pane_popover(root, &menu),
        pane_popover(root, &drop_menu),
        actions,
    )
}

/// A pop-up menu of the pane, showing `menu`.
///
/// # Arguments
///
/// * `root` - The pane's root widget, which owns the menu
/// * `menu` - The menu's items
fn pane_popover(root: &gtk::Box, menu: &gio::Menu) -> gtk::PopoverMenu {
    let popover = gtk::PopoverMenu::from_model(Some(menu));
    popover.add_css_class("pane-menu");
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover.set_parent(root);
    // A popover is not a regular child: it must be detached before its parent goes.
    {
        let popover = popover.clone();
        root.connect_destroy(move |_| popover.unparent());
    }
    popover
}

/// Make right-clicking the empty space below the rows open the context menu (with
/// nothing selected, so it offers New Folder). Clicks on rows are left to their cells.
///
/// # Arguments
///
/// * `view` - The file list
/// * `sender` - Receives the request for the menu
fn attach_empty_space_click(view: &gtk::ColumnView, sender: &ComponentSender<FilePane>) {
    let click = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .build();
    let sender = sender.clone();
    click.connect_released(move |gesture, _, x, y| {
        let Some(view) = gesture.widget() else {
            return;
        };
        // Walk up from the widget under the pointer: a cell means a row was clicked
        // (its own handler opens the menu); a column title means the header was.
        let mut widget = view.pick(x, y, gtk::PickFlags::DEFAULT);
        while let Some(current) = widget {
            if current.has_css_class(CELL_CLASS) || current.type_().name().contains("Title") {
                return;
            }
            if current == view {
                break;
            }
            widget = current.parent();
        }
        sender.input(FilePaneMsg::ContextMenu {
            position: None,
            x,
            y,
        });
    });
    view.add_controller(click);
}

/// Give a cell its mouse handling: right-click for the context menu, dragging (from
/// the Name column's icon and name), and (on folder rows) taking drops.
///
/// # Arguments
///
/// * `cell` - The cell's root widget
/// * `item` - The list item the cell belongs to
/// * `draggable` - Whether its drag handles drag the files (the Name column)
fn attach_cell_handlers(cell: &impl IsA<gtk::Widget>, item: &gtk::ListItem, draggable: bool) {
    let key = cell.as_ptr() as usize;
    CELL_ITEMS.with(|items| items.borrow_mut().insert(key, item.downgrade()));
    cell.connect_destroy(move |_| {
        CELL_ITEMS.with(|items| items.borrow_mut().remove(&key));
    });
    attach_context_click(cell, item);
    if draggable {
        drag::attach_drag_source(cell, item);
    }
    drag::attach_folder_drop(cell, item);
}

/// Make right-clicking a cell open the pane's context menu for its row.
///
/// The column view does not report which row was right-clicked, so each cell listens
/// itself and triggers the pane's context menu action with its row position. The menu
/// opens on release.
///
/// # Arguments
///
/// * `cell` - The cell's root widget
/// * `item` - The list item the cell belongs to (it knows the row's position)
fn attach_context_click(cell: &impl IsA<gtk::Widget>, item: &gtk::ListItem) {
    let click = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .build();
    cell.add_css_class(CELL_CLASS);
    let item = item.downgrade();
    let cell_widget = cell.clone().upcast::<gtk::Widget>();
    click.connect_released(move |_, _, x, y| {
        let (Some(item), Some(view)) = (
            item.upgrade(),
            cell_widget.ancestor(gtk::ColumnView::static_type()),
        ) else {
            return;
        };
        let point = cell_widget
            .compute_point(&view, &graphene::Point::new(x as f32, y as f32))
            .unwrap_or_else(|| graphene::Point::new(x as f32, y as f32));
        let parameter = (item.position(), f64::from(point.x()), f64::from(point.y()));
        let _ = cell_widget.activate_action(
            &format!("{ACTION_GROUP}.{CONTEXT_MENU_ACTION}"),
            Some(&parameter.to_variant()),
        );
    });
    cell.add_controller(click);
}

/// A selection box being dragged out (see [`attach_box_select`]).
struct BoxSelect {
    /// Where the press was, in the list's coordinates; its height is kept as a
    /// position in the whole (scrolled) list, so the box stretches as the list scrolls.
    start_x: f64,
    start_list_y: f64,
    /// The row pressed on; `None` for the empty space below the rows.
    start_row: Option<u32>,
    /// Where the pointer is now, in the list's coordinates.
    pointer: (f64, f64),
    /// The selection to add to (Ctrl held), or an empty one.
    base: gtk::Bitset,
    /// Ctrl (Cmd on macOS) was held: the box adds to the selection.
    adding: bool,
    /// The pointer has moved far enough for this to be a box, not a click.
    active: bool,
    /// Scrolls the list while the pointer is near an edge.
    autoscroll: Option<glib::SourceId>,
}

/// Select files with a box, as file managers do: pressing the left button anywhere in
/// the list except on a file's icon or name (which drags the files instead) and moving
/// draws a box, and every row it touches is selected. Ctrl (Cmd on macOS) adds them to
/// the selection. Near the top or bottom edge the list scrolls on. A plain click on the
/// empty space below the rows clears the selection.
///
/// # Arguments
///
/// * `view` - The file list
/// * `band` - The box drawn over the list (inside an overlay around the list)
fn attach_box_select(view: &gtk::ColumnView, band: &gtk::Box) {
    let drag = gtk::GestureDrag::builder()
        .button(gdk::BUTTON_PRIMARY)
        .build();
    let state: Rc<RefCell<Option<BoxSelect>>> = Rc::default();
    {
        let state = Rc::clone(&state);
        drag.connect_drag_begin(move |gesture, x, y| {
            let Some(view) = gesture.widget().and_downcast::<gtk::ColumnView>() else {
                return;
            };
            // On a file's icon or name the press drags files; on a column title it sorts.
            if starts_elsewhere(&view, x, y) {
                gesture.set_state(gtk::EventSequenceState::Denied);
                return;
            }
            let Some(selection) = view.model() else {
                return;
            };
            let adding = gesture
                .current_event_state()
                .intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::META_MASK);
            let scrolled = view
                .vadjustment()
                .map_or(0.0, |adjustment| adjustment.value());
            *state.borrow_mut() = Some(BoxSelect {
                start_x: x,
                start_list_y: y + scrolled,
                start_row: row_at_height(&view, y),
                pointer: (x, y),
                base: if adding {
                    selection.selection()
                } else {
                    gtk::Bitset::new_empty()
                },
                adding,
                active: false,
                autoscroll: None,
            });
        });
    }
    {
        let (state, band) = (Rc::clone(&state), band.clone());
        drag.connect_drag_update(move |gesture, dx, dy| {
            let Some(view) = gesture.widget().and_downcast::<gtk::ColumnView>() else {
                return;
            };
            let start_autoscroll = {
                let mut state = state.borrow_mut();
                let Some(current) = state.as_mut() else {
                    return;
                };
                let Some((x, y)) = gesture.start_point() else {
                    return;
                };
                current.pointer = (x + dx, y + dy);
                if !current.active && dx.hypot(dy) < BOX_SELECT_THRESHOLD {
                    return;
                }
                // From here on the press is ours: the row under it does not also take
                // it as a click.
                let starting = !current.active;
                current.active = true;
                starting
            };
            if start_autoscroll {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                let timer = autoscroll(&view, &band, &state);
                if let Some(current) = state.borrow_mut().as_mut() {
                    current.autoscroll = Some(timer);
                }
            }
            if let Some(current) = state.borrow().as_ref() {
                update_box(&view, &band, current);
            }
        });
    }
    let finish = {
        let (state, band) = (Rc::clone(&state), band.clone());
        move |gesture: &gtk::GestureDrag| {
            band.set_visible(false);
            let Some(finished) = state.borrow_mut().take() else {
                return;
            };
            if let Some(timer) = finished.autoscroll {
                timer.remove();
            }
            // A click (no box) on the empty space below the rows clears the selection.
            if !finished.active
                && !finished.adding
                && finished.start_row.is_none()
                && let Some(selection) = gesture
                    .widget()
                    .and_downcast::<gtk::ColumnView>()
                    .and_then(|view| view.model())
            {
                selection.unselect_all();
            }
        }
    };
    {
        let finish = finish.clone();
        drag.connect_drag_end(move |gesture, _, _| finish(gesture));
    }
    drag.connect_cancel(move |gesture, _| {
        if let Some(gesture) = gesture.downcast_ref::<gtk::GestureDrag>() {
            finish(gesture);
        }
    });
    view.add_controller(drag);
}

/// Whether a press at (`x`, `y`) in the file list is for something other than a
/// selection box: a file's icon or name (dragging files) or a column title (sorting).
fn starts_elsewhere(view: &gtk::ColumnView, x: f64, y: f64) -> bool {
    let mut widget = view.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(current) = widget {
        if current.has_css_class(DRAG_HANDLE_CLASS) || current.type_().name().contains("Title") {
            return true;
        }
        if current == *view.upcast_ref::<gtk::Widget>() {
            break;
        }
        widget = current.parent();
    }
    false
}

/// Select the rows the box touches, and draw the box.
fn update_box(view: &gtk::ColumnView, band: &gtk::Box, current: &BoxSelect) {
    let Some(selection) = view.model() else {
        return;
    };
    let rows = selection.n_items();
    let scrolled = view
        .vadjustment()
        .map_or(0.0, |adjustment| adjustment.value());
    let start_y = current.start_list_y - scrolled;
    let (x, y) = current.pointer;
    // The row under the pointer. Above the rows (over the column titles, or past the
    // top), the first row on screen; below them, none.
    let row = row_at_height(view, y).or_else(|| (y < start_y).then(|| first_row_from(view, y)));
    // Starting below the rows and moving up selects from the last row up.
    let range = match (current.start_row, row) {
        (None, None) => None,
        (Some(row), None) | (None, Some(row)) => Some((row, rows.saturating_sub(1))),
        (Some(start), Some(row)) => Some((start.min(row), start.max(row))),
    };
    let selected = current.base.copy();
    if let Some((low, high)) = range
        && rows > 0
    {
        selected.add_range_closed(low, high.min(rows - 1));
    }
    selection.set_selection(&selected, &gtk::Bitset::new_range(0, rows));

    // The box, in the overlay's coordinates, kept within the list.
    let Some(overlay) = band.parent() else {
        return;
    };
    let height = f64::from(view.height());
    let (top, bottom) = (start_y.min(y).max(0.0), start_y.max(y).min(height));
    let (left, right) = (current.start_x.min(x).max(0.0), current.start_x.max(x));
    let Some(corner) = view.compute_point(&overlay, &graphene::Point::new(left as f32, top as f32))
    else {
        return;
    };
    band.set_margin_start(corner.x().max(0.0) as i32);
    band.set_margin_top(corner.y().max(0.0) as i32);
    band.set_size_request(
        (right.min(f64::from(view.width())) - left).max(1.0) as i32,
        (bottom - top).max(1.0) as i32,
    );
    band.set_visible(true);
}

/// While a selection box is drawn, scroll the list when the pointer is near its top or
/// bottom edge, stretching the box over the rows that come into view.
fn autoscroll(
    view: &gtk::ColumnView,
    band: &gtk::Box,
    state: &Rc<RefCell<Option<BoxSelect>>>,
) -> glib::SourceId {
    let (view, band, state) = (view.downgrade(), band.downgrade(), Rc::downgrade(state));
    glib::timeout_add_local(AUTOSCROLL_INTERVAL, move || {
        let (Some(view), Some(band), Some(state)) =
            (view.upgrade(), band.upgrade(), state.upgrade())
        else {
            return glib::ControlFlow::Break;
        };
        let state = state.borrow();
        let Some(current) = state.as_ref() else {
            return glib::ControlFlow::Break;
        };
        let (_, y) = current.pointer;
        let height = f64::from(view.height());
        let step = if y < AUTOSCROLL_EDGE {
            -(AUTOSCROLL_EDGE - y)
        } else if y > height - AUTOSCROLL_EDGE {
            y - (height - AUTOSCROLL_EDGE)
        } else {
            return glib::ControlFlow::Continue;
        };
        if let Some(adjustment) = view.vadjustment() {
            let before = adjustment.value();
            adjustment.set_value(before + step.clamp(-AUTOSCROLL_MAX_STEP, AUTOSCROLL_MAX_STEP));
            if adjustment.value() != before {
                update_box(&view, &band, current);
            }
        }
        glib::ControlFlow::Continue
    })
}

/// The row at height `y` in the file list, whatever the pointer's side position (a
/// box can start right of the columns).
fn row_at_height(view: &gtk::ColumnView, y: f64) -> Option<u32> {
    // Inside the Name column, which always comes first; a little in from the edge.
    row_at(view, 24.0, y)
}

/// The first row at or below height `y` in the file list (the top one on screen when
/// `y` is above the rows), or 0 if none is found.
fn first_row_from(view: &gtk::ColumnView, y: f64) -> u32 {
    let height = f64::from(view.height());
    let mut probe = y.max(0.0);
    while probe < height {
        if let Some(row) = row_at_height(view, probe) {
            return row;
        }
        probe += 4.0;
    }
    0
}

/// The position of the row at (`x`, `y`) in the file list, if there is one.
fn row_at(view: &gtk::ColumnView, x: f64, y: f64) -> Option<u32> {
    let mut widget = view.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(current) = widget {
        let key = current.as_ptr() as usize;
        let item =
            CELL_ITEMS.with(|items| items.borrow().get(&key).and_then(glib::WeakRef::upgrade));
        if let Some(item) = item {
            return Some(item.position());
        }
        if current == *view.upcast_ref::<gtk::Widget>() {
            return None;
        }
        widget = current.parent();
    }
    None
}

/// Keep the ".." row first whatever column and direction the list is sorted by.
///
/// The column sorters alone cannot do this: GTK reverses their result for descending
/// order, which would move ".." to the bottom. This wraps them in a sorter that settles
/// ".." itself and only asks the column sorter about ordinary rows.
///
/// # Arguments
///
/// * `entries` - The list view whose sort model gets the wrapping sorter
fn pin_parent_row(entries: &TypedColumnView<Row, gtk::MultiSelection>) {
    // relm4 places a `gtk::SortListModel` directly under the selection model.
    let Some(sort_model) = entries
        .selection_model
        .model()
        .and_downcast::<gtk::SortListModel>()
    else {
        tracing::warn!("file list has no sort model; '..' will sort like other rows");
        return;
    };
    let Some(column_sorter) = entries.view.sorter() else {
        return;
    };

    let pinned = gtk::CustomSorter::new({
        // The closure keeps its own reference to the column sorter (a cheap refcount bump).
        let column_sorter = column_sorter.clone();
        move |a, b| match (is_parent_row(a), is_parent_row(b)) {
            (true, true) => gtk::Ordering::Equal,
            (true, false) => gtk::Ordering::Smaller,
            (false, true) => gtk::Ordering::Larger,
            (false, false) => column_sorter.compare(a, b),
        }
    });
    // Re-sort whenever a column header is clicked. A weak reference avoids a reference
    // cycle between the two sorters.
    let pinned_weak = pinned.downgrade();
    column_sorter.connect_changed(move |_, change| {
        if let Some(pinned) = pinned_weak.upgrade() {
            pinned.changed(change);
        }
    });
    sort_model.set_sorter(Some(&pinned));
}

/// Whether a list model item is the ".." row.
///
/// # Arguments
///
/// * `item` - Item from the list model (relm4 stores rows in `glib::BoxedAnyObject`s)
fn is_parent_row(item: &glib::Object) -> bool {
    item.downcast_ref::<glib::BoxedAnyObject>()
        .and_then(|boxed| boxed.try_borrow::<Row>().ok())
        .is_some_and(|row| matches!(*row, Row::Parent { .. }))
}

/// Find which entries of `dir` are special folders (Documents, Downloads, home, ...).
///
/// # Arguments
///
/// * `dir` - Directory being listed
///
/// # Returns
///
/// `(folder name, theme icon)` pairs for special folders located directly in `dir`
fn special_folder_icons(dir: &Path) -> Vec<(OsString, &'static str)> {
    let home = glib::home_dir();
    SPECIAL_FOLDER_ICONS
        .iter()
        .filter_map(|&(kind, icon)| glib::user_special_dir(kind).map(|path| (path, icon)))
        // An unset XDG folder points at the home folder itself; that is not special.
        .filter(|(path, _)| path != &home)
        .chain(iter::once((home.clone(), HOME_FOLDER_ICON)))
        .filter(|(path, _)| path.parent() == Some(dir))
        .filter_map(|(path, icon)| path.file_name().map(|name| (name.to_os_string(), icon)))
        .collect()
}

/// Icon the desktop uses for a file, chosen from its name like the file manager does.
///
/// # Arguments
///
/// * `entry` - A file (not a directory)
fn file_icon(entry: &FileEntry) -> gio::Icon {
    let (content_type, _uncertain) = gio::content_type_guess(Some(entry.name()), None);
    gio::content_type_get_icon(&content_type)
}

/// Show a file or folder's icon: the one the system's file manager shows (see
/// [`system_icon`]), or else the icon theme's.
///
/// # Arguments
///
/// * `icon` - The row's image
/// * `entry` - The file or folder
/// * `path` - Its full path
/// * `special_icon` - Theme icon of a special folder such as Documents, if it is one
fn show_entry_icon(
    icon: &gtk::Image,
    entry: &FileEntry,
    path: &SidePath,
    special_icon: Option<&'static str>,
) {
    if let Some(system) = system_icon(entry, path, special_icon) {
        icon.set_paintable(Some(&system));
        return;
    }
    match entry.kind() {
        EntryKind::Directory => icon.set_icon_name(Some(special_icon.unwrap_or(FOLDER_ICON))),
        EntryKind::File => icon.set_from_gicon(&file_icon(entry)),
    }
}

/// The icon Finder shows for a file or folder (see [`crate::mac_icons`]). Special
/// folders, which are on this Mac, are looked up by path.
#[cfg(target_os = "macos")]
fn system_icon(
    entry: &FileEntry,
    path: &SidePath,
    special_icon: Option<&'static str>,
) -> Option<gdk::Texture> {
    use crate::mac_icons;
    match entry.kind() {
        EntryKind::Directory if special_icon.is_some() => {
            path.as_local().and_then(mac_icons::local_folder)
        }
        EntryKind::Directory => mac_icons::folder(),
        EntryKind::File => mac_icons::file(entry.name()),
    }
}

/// The icon File Explorer shows for a file or folder (see [`crate::win_icons`]).
/// Special folders, which are on this computer, are looked up by path.
#[cfg(windows)]
fn system_icon(
    entry: &FileEntry,
    path: &SidePath,
    special_icon: Option<&'static str>,
) -> Option<gdk::Texture> {
    use crate::win_icons;
    match entry.kind() {
        EntryKind::Directory if special_icon.is_some() => {
            path.as_local().and_then(win_icons::local_folder)
        }
        EntryKind::Directory => win_icons::folder(),
        EntryKind::File => win_icons::file(entry.name(), path.as_local()),
    }
}

/// Linux: the icon theme's icons are the desktop's own, so there is no other source.
#[cfg(not(any(target_os = "macos", windows)))]
fn system_icon(
    _entry: &FileEntry,
    _path: &SidePath,
    _special_icon: Option<&'static str>,
) -> Option<gdk::Texture> {
    None
}

/// The system file manager's icon for an ordinary folder; `None` on Linux.
fn system_folder_icon() -> Option<gdk::Texture> {
    #[cfg(target_os = "macos")]
    return crate::mac_icons::folder();
    #[cfg(windows)]
    return crate::win_icons::folder();
    #[cfg(not(any(target_os = "macos", windows)))]
    None
}

/// Draw a cell's text in red while a running transfer uses its row's item.
fn bind_in_use(row: &Row, label: &gtk::Label) {
    match row {
        Row::Entry { path, in_use, .. } => in_use.bind(label, Some(path)),
        // The ".." row is never in use; a recycled label may still carry the style.
        Row::Parent { .. } => label.remove_css_class(IN_USE_CLASS),
    }
}

/// Create the label used by the plain text columns.
///
/// # Arguments
///
/// * `xalign` - Horizontal text alignment, 0.0 (left) to 1.0 (right)
fn text_cell(xalign: f32) -> gtk::Label {
    gtk::Label::builder()
        .xalign(xalign)
        .margin_start(6)
        .margin_end(6)
        .build()
}

/// Order two rows, keeping ".." before everything and using `compare` for the rest.
///
/// # Arguments
///
/// * `a` - First row
/// * `b` - Second row
/// * `compare` - Ordering for two ordinary entries
fn compare_rows(a: &Row, b: &Row, compare: fn(&FileEntry, &FileEntry) -> Ordering) -> Ordering {
    match (a, b) {
        (Row::Parent { .. }, Row::Parent { .. }) => Ordering::Equal,
        (Row::Parent { .. }, Row::Entry { .. }) => Ordering::Less,
        (Row::Entry { .. }, Row::Parent { .. }) => Ordering::Greater,
        (Row::Entry { entry: a, .. }, Row::Entry { entry: b, .. }) => compare(a, b),
    }
}

/// Name column: the desktop's icon for the item plus its name. Sorts directories first.
struct NameColumn;

impl RelmColumn for NameColumn {
    type Root = gtk::Box;
    /// The item icon, the up arrow shown over it on the ".." row, and the name.
    type Widgets = (gtk::Image, gtk::Image, gtk::Label);
    type Item = Row;

    const COLUMN_NAME: &'static str = "Name";
    const ENABLE_RESIZE: bool = true;
    const ENABLE_EXPAND: bool = true;

    fn setup(item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let icon = gtk::Image::new();
        let arrow = UP_ARROW.with(|texture| match texture {
            Some(texture) => gtk::Image::from_paintable(Some(texture)),
            None => gtk::Image::from_icon_name("go-up-symbolic"),
        });
        arrow.set_pixel_size(UP_ARROW_SIZE);
        arrow.set_halign(gtk::Align::Center);
        arrow.set_valign(gtk::Align::End);
        let icon_stack = gtk::Overlay::new();
        icon_stack.set_child(Some(&icon));
        icon_stack.add_overlay(&arrow);

        let label = text_cell(0.0);
        label.set_margin_start(0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

        let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        // Pressing the icon or the name drags the files; the rest of the cell starts a
        // selection box.
        icon_stack.add_css_class(DRAG_HANDLE_CLASS);
        label.add_css_class(DRAG_HANDLE_CLASS);
        root.append(&icon_stack);
        root.append(&label);
        attach_cell_handlers(&root, item, true);
        (root, (icon, arrow, label))
    }

    fn bind(item: &mut Self::Item, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        let (icon, arrow, label) = widgets;
        bind_in_use(item, label);
        arrow.set_visible(matches!(item, Row::Parent { .. }));
        match item {
            Row::Parent { .. } => {
                match system_folder_icon() {
                    Some(folder) => icon.set_paintable(Some(&folder)),
                    None => icon.set_icon_name(Some(FOLDER_ICON)),
                }
                label.set_label("..");
            }
            Row::Entry {
                entry,
                path,
                special_icon,
                ..
            } => {
                show_entry_icon(icon, entry, path, *special_icon);
                label.set_label(&entry.display_name());
            }
        }
    }

    fn sort_fn() -> OrdFn<Self::Item> {
        Some(Box::new(|a, b| {
            compare_rows(a, b, FileEntry::listing_order)
        }))
    }
}

/// Size column: human readable size, blank for directories and "..".
struct SizeColumn;

impl RelmColumn for SizeColumn {
    type Root = gtk::Label;
    type Widgets = ();
    type Item = Row;

    const COLUMN_NAME: &'static str = "Size";
    const ENABLE_RESIZE: bool = true;

    fn setup(item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let cell = text_cell(1.0);
        attach_cell_handlers(&cell, item, false);
        (cell, ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, label: &mut Self::Root) {
        bind_in_use(item, label);
        match item {
            Row::Entry { entry, .. } if entry.kind() == EntryKind::File => {
                label.set_label(&format_size(entry.size()));
            }
            Row::Parent { .. } | Row::Entry { .. } => label.set_label(""),
        }
    }

    fn sort_fn() -> OrdFn<Self::Item> {
        Some(Box::new(|a, b| compare_rows(a, b, FileEntry::size_order)))
    }
}

/// Modified column: local date and time of the last modification.
struct ModifiedColumn;

impl RelmColumn for ModifiedColumn {
    type Root = gtk::Label;
    type Widgets = ();
    type Item = Row;

    const COLUMN_NAME: &'static str = "Modified";
    const ENABLE_RESIZE: bool = true;

    fn setup(item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        let cell = text_cell(0.0);
        attach_cell_handlers(&cell, item, false);
        (cell, ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, label: &mut Self::Root) {
        bind_in_use(item, label);
        let text = match item {
            Row::Parent { .. } => String::new(),
            Row::Entry { entry, .. } => entry.modified().map(format_modified).unwrap_or_default(),
        };
        label.set_label(&text);
    }

    fn sort_fn() -> OrdFn<Self::Item> {
        Some(Box::new(|a, b| {
            compare_rows(a, b, FileEntry::modified_order)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shuttlescp_core::path::RemotePath;

    #[test]
    fn compare_rows_puts_parent_first() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        std::fs::write(temp.path().join("a.txt"), b"").expect("write file");
        let entry = local_fs::list_dir(temp.path())
            .expect("list temp dir")
            .remove(0);
        let row = Row::Entry {
            in_use: Rc::default(),
            path: SidePath::Local(temp.path().join("a.txt")),
            entry,
            special_icon: None,
        };
        let parent = || Row::Parent {
            path: SidePath::Local(PathBuf::from("/")),
        };

        // Act + Assert
        assert_eq!(
            compare_rows(&parent(), &row, FileEntry::size_order),
            Ordering::Less
        );
        assert_eq!(
            compare_rows(&row, &parent(), FileEntry::size_order),
            Ordering::Greater
        );
        assert_eq!(
            compare_rows(&parent(), &parent(), FileEntry::size_order),
            Ordering::Equal
        );
    }

    #[test]
    fn items_inside_a_folder_in_use_are_in_use() {
        let server = |path: &str| SidePath::Remote(RemotePath::new(path));
        let marks = InUseMarks::default();
        marks.set(vec![server("/srv/photos"), server("/srv/a.txt")]);

        assert!(marks.is_in_use(&server("/srv/photos")));
        assert!(marks.is_in_use(&server("/srv/photos/2026/beach.jpg")));
        assert!(marks.is_in_use(&server("/srv/a.txt")));
        assert!(!marks.is_in_use(&server("/srv/a.txt.bak")));
        assert!(!marks.is_in_use(&server("/srv")));
        assert!(
            marks.is_in_use(&server("/srv/a.txt.filepart")),
            "a partial copy is in use"
        );
        assert!(
            !marks.is_in_use(&SidePath::Local(PathBuf::from("/srv/a.txt"))),
            "the same path on this computer is another file"
        );
    }
}
