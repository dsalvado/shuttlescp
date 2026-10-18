//! A file explorer pane: refresh button, disk drop-down, path label and a sortable column
//! view whose first row, "..", goes up one folder.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::iter;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use relm4::gtk::prelude::*;
use relm4::gtk::{gdk, gio, glib};
use relm4::prelude::*;
use relm4::typed_view::OrdFn;
use relm4::typed_view::column::{RelmColumn, TypedColumnView};

use crate::disk_picker::DiskPicker;
use crate::disks;
use crate::local_fs::{self, EntryKind, FileEntry};
use crate::remote::Remote;

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

thread_local! {
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
}

/// What a [`FilePane`] starts with.
#[derive(Debug)]
pub struct FilePaneInit {
    /// Directory shown first.
    pub start_dir: PathBuf,
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
        dir: PathBuf,
        /// Mount point of the disk holding `dir`.
        disk_root: Option<PathBuf>,
    },
}

/// Result of a background directory listing, delivered back to the UI thread.
#[derive(Debug)]
pub struct Listing {
    dir: PathBuf,
    /// The entries, or a message for the status line.
    result: Result<Vec<FileEntry>, String>,
}

/// One line of the file list.
#[derive(Debug)]
enum Row {
    /// The ".." line that goes up one folder; always shown first.
    Parent,
    /// A file or folder inside the current directory.
    Entry {
        entry: FileEntry,
        /// Theme icon for special folders such as Documents; `None` for everything else.
        special_icon: Option<&'static str>,
    },
}

/// File explorer component, for the local disks or a remote server.
pub struct FilePane {
    /// Directory whose contents are currently displayed.
    current_dir: PathBuf,
    /// Most recently requested directory; listings for anything else are stale.
    requested_dir: PathBuf,
    entries: TypedColumnView<Row, gtk::SingleSelection>,
    status: String,
    disk_picker: DiskPicker,
    /// Last directory shown on each disk, keyed by the disk's mount point, so returning
    /// to a disk resumes where the user left it.
    last_dir_on_disk: BTreeMap<PathBuf, PathBuf>,
    /// The server this pane browses; `None` for the local disks.
    remote: Option<Remote>,
}

impl FilePane {
    /// Start listing `dir` in the background so a slow disk or network never freezes
    /// the UI: on a worker thread for local folders, as an async SFTP request for remote.
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory to list
    /// * `sender` - Component sender used to spawn the work and receive its result
    fn request_listing(&mut self, dir: PathBuf, sender: &ComponentSender<Self>) {
        tracing::debug!("listing {}", dir.display());
        self.requested_dir = dir.clone();
        self.status = String::from("Loading...");
        match self.remote.clone() {
            Some(remote) => sender.oneshot_command(async move {
                Listing {
                    result: remote.list_dir(&dir).await,
                    dir,
                }
            }),
            // `move` transfers ownership of `dir` into the closure, which runs on another
            // thread.
            None => sender.spawn_oneshot_command(move || Listing {
                result: local_fs::list_dir(&dir).map_err(|error| error.to_string()),
                dir,
            }),
        }
    }

    /// Replace the list contents with a fresh listing of `dir`.
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory the entries belong to
    /// * `entries` - Its contents, as returned by [`local_fs::list_dir`]
    fn show_entries(&mut self, dir: &Path, entries: Vec<FileEntry>) {
        // Special folder icons describe this computer's folders, not the server's.
        let special_folders = if self.remote.is_some() {
            Vec::new()
        } else {
            special_folder_icons(dir)
        };
        self.entries.clear();
        if dir.parent().is_some() {
            self.entries.append(Row::Parent);
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
                    entry,
                    special_icon,
                }
            }));
    }
}

// `#[relm4::component(pub)]` expands the `view!` block below into widget construction code
// and a struct holding the widgets, roughly like a declarative template.
#[relm4::component(pub)]
impl Component for FilePane {
    type Init = FilePaneInit;
    type Input = FilePaneMsg;
    type Output = FilePaneOutput;
    type CommandOutput = Listing;

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
                disk_dropdown -> gtk::DropDown {
                    // Local disks mean nothing when browsing a server.
                    set_visible: model.remote.is_none(),
                },
                // Current directory. A plain label: it cannot be clicked, focused or edited.
                gtk::Label {
                    set_hexpand: true,
                    set_xalign: 0.0,
                    // Long paths lose their start, keeping the current folder name visible.
                    set_ellipsize: gtk::pango::EllipsizeMode::Start,
                    add_css_class: "path-label",
                    // `#[watch]` re-evaluates the property after every update.
                    #[watch]
                    set_label: &model.current_dir.to_string_lossy(),
                    #[watch]
                    set_tooltip_text: Some(&model.current_dir.to_string_lossy()),
                },
            },

            gtk::ScrolledWindow {
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
        let mut entries = TypedColumnView::<Row, gtk::SingleSelection>::new();
        entries.append_column::<NameColumn>();
        entries.append_column::<SizeColumn>();
        entries.append_column::<ModifiedColumn>();
        pin_parent_row(&entries);

        // Each callback owns its own sender clone, since both outlive this function.
        let select_sender = sender.clone();
        let mounts_sender = sender.clone();
        let disk_picker = DiskPicker::new(
            move |position| select_sender.input(FilePaneMsg::DiskSelected(position)),
            move || mounts_sender.input(FilePaneMsg::DisksChanged),
        );
        disk_picker.show_dir(&start_dir);

        let mut model = Self {
            current_dir: start_dir.clone(),
            requested_dir: start_dir.clone(),
            entries,
            status: String::new(),
            disk_picker,
            last_dir_on_disk,
            remote,
        };
        model.request_listing(start_dir, &sender);

        let entries_view = &model.entries.view;
        let disk_dropdown = model.disk_picker.widget();
        // `view_output!()` is generated by the macro; it builds the widget tree declared above.
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            FilePaneMsg::Refresh => self.request_listing(self.current_dir.clone(), &sender),
            FilePaneMsg::Activate(position) => {
                // Build the target first: `item.borrow()` holds a borrow that must end
                // before `self` is mutably borrowed by `request_listing`.
                let target =
                    self.entries
                        .get_visible(position)
                        .and_then(|item| match &*item.borrow() {
                            Row::Parent => self.current_dir.parent().map(Path::to_path_buf),
                            Row::Entry { entry, .. } => match entry.kind() {
                                EntryKind::Directory => Some(self.current_dir.join(entry.name())),
                                EntryKind::File => None,
                            },
                        });
                if let Some(dir) = target {
                    self.request_listing(dir, &sender);
                }
            }
            FilePaneMsg::DiskSelected(position) => {
                let Some(root) = usize::try_from(position)
                    .ok()
                    .and_then(|index| self.disk_picker.disks().get(index))
                    .map(|disk| disk.root().to_path_buf())
                else {
                    return;
                };
                // Resume at the last folder seen on this disk if it (or a parent on the
                // same disk) still exists, otherwise start at the disk's top.
                let target = self
                    .last_dir_on_disk
                    .get(&root)
                    .and_then(|dir| local_fs::nearest_listable_dir(dir))
                    .filter(|dir| dir.starts_with(&root))
                    .map_or(root.clone(), Path::to_path_buf);
                tracing::info!(
                    "switching to disk {} at {}",
                    root.display(),
                    target.display()
                );
                self.request_listing(target, &sender);
            }
            FilePaneMsg::DisksChanged if self.remote.is_some() => {}
            FilePaneMsg::DisksChanged => {
                self.disk_picker.reload();
                self.disk_picker.show_dir(&self.current_dir);
                // If the disk being shown was removed, fall back to the nearest folder that
                // still exists (ultimately "/").
                if let Some(fallback) = local_fs::nearest_listable_dir(&self.current_dir)
                    && fallback != self.current_dir
                {
                    tracing::info!(
                        "{} is gone; showing {}",
                        self.current_dir.display(),
                        fallback.display()
                    );
                    self.request_listing(fallback.to_path_buf(), &sender);
                }
            }
        }
    }

    fn update_cmd(
        &mut self,
        listing: Self::CommandOutput,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        // A newer navigation superseded this listing before it finished.
        if listing.dir != self.requested_dir {
            tracing::trace!("dropping stale listing of {}", listing.dir.display());
            return;
        }
        match listing.result {
            Ok(entries) => {
                self.status = format!("{} items", entries.len());
                self.show_entries(&listing.dir, entries);
                let disk_root = if self.remote.is_some() {
                    None
                } else {
                    disks::disk_index(self.disk_picker.disks(), &listing.dir)
                        .and_then(|index| self.disk_picker.disks().get(index))
                        .map(|disk| disk.root().to_path_buf())
                };
                if let Some(root) = &disk_root {
                    self.last_dir_on_disk
                        .insert(root.clone(), listing.dir.clone());
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

/// Keep the ".." row first whatever column and direction the list is sorted by.
///
/// The column sorters alone cannot do this: GTK reverses their result for descending
/// order, which would move ".." to the bottom. This wraps them in a sorter that settles
/// ".." itself and only asks the column sorter about ordinary rows.
///
/// # Arguments
///
/// * `entries` - The list view whose sort model gets the wrapping sorter
fn pin_parent_row(entries: &TypedColumnView<Row, gtk::SingleSelection>) {
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
        .is_some_and(|row| matches!(*row, Row::Parent))
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
        (Row::Parent, Row::Parent) => Ordering::Equal,
        (Row::Parent, Row::Entry { .. }) => Ordering::Less,
        (Row::Entry { .. }, Row::Parent) => Ordering::Greater,
        (Row::Entry { entry: a, .. }, Row::Entry { entry: b, .. }) => compare(a, b),
    }
}

/// Size order: directories first, then smallest to largest, then by name.
fn by_size(a: &FileEntry, b: &FileEntry) -> Ordering {
    a.kind()
        .cmp(&b.kind())
        .then_with(|| a.size().cmp(&b.size()))
        .then_with(|| a.listing_order(b))
}

/// Date order: oldest to newest, then by name.
fn by_modified(a: &FileEntry, b: &FileEntry) -> Ordering {
    a.modified()
        .cmp(&b.modified())
        .then_with(|| a.listing_order(b))
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

    fn setup(_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
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
        root.append(&icon_stack);
        root.append(&label);
        (root, (icon, arrow, label))
    }

    fn bind(item: &mut Self::Item, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        let (icon, arrow, label) = widgets;
        arrow.set_visible(matches!(item, Row::Parent));
        match item {
            Row::Parent => {
                icon.set_icon_name(Some(FOLDER_ICON));
                label.set_label("..");
            }
            Row::Entry {
                entry,
                special_icon,
            } => {
                match entry.kind() {
                    EntryKind::Directory => {
                        icon.set_icon_name(Some(special_icon.unwrap_or(FOLDER_ICON)));
                    }
                    EntryKind::File => icon.set_from_gicon(&file_icon(entry)),
                }
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

    fn setup(_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        (text_cell(1.0), ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, label: &mut Self::Root) {
        match item {
            Row::Entry { entry, .. } if entry.kind() == EntryKind::File => {
                label.set_label(&format_size(entry.size()));
            }
            Row::Parent | Row::Entry { .. } => label.set_label(""),
        }
    }

    fn sort_fn() -> OrdFn<Self::Item> {
        Some(Box::new(|a, b| compare_rows(a, b, by_size)))
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

    fn setup(_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        (text_cell(0.0), ())
    }

    fn bind(item: &mut Self::Item, _widgets: &mut Self::Widgets, label: &mut Self::Root) {
        let text = match item {
            Row::Parent => String::new(),
            Row::Entry { entry, .. } => entry.modified().map(format_modified).unwrap_or_default(),
        };
        label.set_label(&text);
    }

    fn sort_fn() -> OrdFn<Self::Item> {
        Some(Box::new(|a, b| compare_rows(a, b, by_modified)))
    }
}

/// Format a byte count with binary units, e.g. `1536` becomes `"1.5 KiB"`.
///
/// # Arguments
///
/// * `bytes` - Size in bytes
///
/// # Returns
///
/// Whole bytes below 1 KiB, otherwise one decimal place in the largest fitting unit
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    // `as f64` is an explicit numeric cast; Rust never converts number types implicitly.
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Format a timestamp as `YYYY-MM-DD HH:MM` in the local time zone.
///
/// # Arguments
///
/// * `time` - Timestamp to format
///
/// # Returns
///
/// The formatted time, or an empty string for times GLib cannot represent
fn format_modified(time: SystemTime) -> String {
    let seconds = match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).ok(),
        // Pre-1970 timestamps come back as an error holding the distance before the epoch.
        Err(before) => i64::try_from(before.duration().as_secs()).ok().map(|s| -s),
    };
    seconds
        .and_then(|seconds| glib::DateTime::from_unix_local(seconds).ok())
        .and_then(|date| date.format("%Y-%m-%d %H:%M").ok())
        .map(String::from)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn format_size_uses_bytes_below_one_kibibyte() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn format_size_scales_to_largest_unit() {
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(5 * 1024 * 1024 * 1024), "5.0 GiB");
        assert_eq!(format_size(u64::MAX), "16.0 EiB");
    }

    #[test]
    fn format_modified_produces_date_and_minutes() {
        // Arrange: noon UTC keeps the date stable in every time zone within +-11h.
        let time =
            UNIX_EPOCH + Duration::from_secs(1_700_000_000 - 1_700_000_000 % 86_400 + 43_200);

        // Act
        let text = format_modified(time);

        // Assert
        assert!(text.starts_with("2023-11-14 "), "unexpected: {text}");
        assert_eq!(text.len(), "2023-11-14 12:00".len());
    }

    #[test]
    fn compare_rows_puts_parent_first() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        std::fs::write(temp.path().join("a.txt"), b"").expect("write file");
        let entry = local_fs::list_dir(temp.path())
            .expect("list temp dir")
            .remove(0);
        let row = Row::Entry {
            entry,
            special_icon: None,
        };

        // Act + Assert
        assert_eq!(compare_rows(&Row::Parent, &row, by_size), Ordering::Less);
        assert_eq!(compare_rows(&row, &Row::Parent, by_size), Ordering::Greater);
        assert_eq!(
            compare_rows(&Row::Parent, &Row::Parent, by_size),
            Ordering::Equal
        );
    }
}
