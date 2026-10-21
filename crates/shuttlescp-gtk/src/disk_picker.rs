//! Drop-down listing the disks: this machine's, kept up to date as drives are
//! (un)mounted, or a server's, given by the pane once it has asked the server.

use std::path::Path;

use relm4::gtk;
use relm4::gtk::prelude::*;
use relm4::gtk::{gio, glib};

use shuttlescp_core::disks::{self, Disk};
use shuttlescp_core::path::SidePath;

/// Display name of the root filesystem entry: its path, as Linux users know it.
const ROOT_DISK_NAME: &str = "/";
/// Theme icon of the root filesystem entry.
const ROOT_DISK_ICON: &str = "drive-harddisk";

/// Disk drop-down plus the data behind it.
pub struct DiskPicker {
    dropdown: gtk::DropDown,
    /// Display model: one `BoxedAnyObject` holding a [`Disk`] per row.
    store: gio::ListStore,
    /// Same disks as `store`, for lookups without going through GObject boxing.
    disks: Vec<Disk>,
    /// The "selected" handler; blocked while the selection is changed from code, so only
    /// real user choices reach `on_select`.
    selected_handler: glib::SignalHandlerId,
    /// Source of mount events for this machine's disks; kept alive so its signal
    /// handlers stay connected. `None` for a server's disks.
    monitor: Option<gio::VolumeMonitor>,
}

impl DiskPicker {
    /// Build the picker and start watching for mounts.
    ///
    /// # Arguments
    ///
    /// * `on_select` - Called with the row position when the user picks a disk
    /// * `on_mounts_changed` - Called when a disk is mounted, unmounted or renamed
    ///
    /// # Returns
    ///
    /// The picker, filled with the current disks and nothing selected yet
    pub fn new(
        on_select: impl Fn(u32) + 'static,
        on_mounts_changed: impl Fn() + Clone + 'static,
    ) -> Self {
        let mut picker = Self::for_server(on_select);
        let monitor = gio::VolumeMonitor::get();
        // Each signal needs its own copy of the callback, hence the `Clone` bound.
        let on_added = on_mounts_changed.clone();
        monitor.connect_mount_added(move |_, _| on_added());
        let on_changed = on_mounts_changed.clone();
        monitor.connect_mount_changed(move |_, _| on_changed());
        monitor.connect_mount_removed(move |_, _| on_mounts_changed());
        picker.monitor = Some(monitor);
        picker.reload();
        picker
    }

    /// Build an empty picker for a server's disks; fill it with
    /// [`DiskPicker::set_disks`].
    ///
    /// # Arguments
    ///
    /// * `on_select` - Called with the row position when the user picks a disk
    pub fn for_server(on_select: impl Fn(u32) + 'static) -> Self {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let dropdown = gtk::DropDown::new(Some(store.clone()), None::<gtk::Expression>);
        dropdown.set_factory(Some(&disk_row_factory()));
        dropdown.set_tooltip_text(Some("Disk"));
        let selected_handler =
            dropdown.connect_selected_notify(move |dropdown| on_select(dropdown.selected()));
        Self {
            dropdown,
            store,
            disks: Vec::new(),
            selected_handler,
            monitor: None,
        }
    }

    /// The drop-down widget, for placing in a layout.
    pub fn widget(&self) -> &gtk::DropDown {
        &self.dropdown
    }

    /// Disks in the order shown.
    pub fn disks(&self) -> &[Disk] {
        &self.disks
    }

    /// Re-read the list of this machine's mounted disks (nothing for a server's).
    /// Leaves nothing selected; follow with [`DiskPicker::show_dir`].
    pub fn reload(&mut self) {
        let Some(monitor) = &self.monitor else {
            return;
        };
        let disks = list_disks(monitor);
        tracing::debug!("found {} disks", disks.len());
        self.set_disks(disks);
    }

    /// Show these disks. Leaves nothing selected; follow with [`DiskPicker::show_dir`].
    ///
    /// # Arguments
    ///
    /// * `disks` - Disks in display order
    pub fn set_disks(&mut self, disks: Vec<Disk>) {
        self.disks = disks;
        let rows: Vec<glib::BoxedAnyObject> = self
            .disks
            .iter()
            .map(|disk| glib::BoxedAnyObject::new(disk.clone()))
            .collect();
        self.dropdown.block_signal(&self.selected_handler);
        self.store.splice(0, self.store.n_items(), &rows);
        self.dropdown.unblock_signal(&self.selected_handler);
    }

    /// Select the disk holding `dir`, without reporting it as a user choice.
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory now shown in the pane
    pub fn show_dir(&self, dir: &SidePath) {
        let position = disks::disk_index(&self.disks, dir)
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(gtk::INVALID_LIST_POSITION);
        self.dropdown.block_signal(&self.selected_handler);
        self.dropdown.set_selected(position);
        self.dropdown.unblock_signal(&self.selected_handler);
    }
}

/// Factory drawing each drop-down row as the disk's icon followed by its name.
fn disk_row_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.append(&gtk::Image::new());
        row.append(&gtk::Label::new(None));
        item.set_child(Some(&row));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(boxed) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        let Ok(disk) = boxed.try_borrow::<Disk>() else {
            return;
        };
        let Some(row) = item.child() else {
            return;
        };
        if let Some(icon) = row.first_child().and_downcast::<gtk::Image>() {
            // The core crate keeps the icon as text; turn it back into a GIO icon.
            match gio::Icon::for_string(disk.icon()) {
                Ok(gicon) => icon.set_from_gicon(&gicon),
                Err(_) => icon.set_icon_name(Some(ROOT_DISK_ICON)),
            }
        }
        if let Some(label) = row.last_child().and_downcast::<gtk::Label>() {
            label.set_label(disk.name());
            label.set_tooltip_text(Some(&disk.root().to_string()));
        }
    });
    factory
}

/// List the root filesystem followed by every user-visible local mount, as the desktop's
/// file manager shows them.
///
/// # Arguments
///
/// * `monitor` - The GIO volume monitor (the file manager's source of mounts)
///
/// # Returns
///
/// Disks in display order; mounts without a local path (e.g. network shares) are skipped
fn list_disks(monitor: &gio::VolumeMonitor) -> Vec<Disk> {
    let root = Path::new("/");
    let mounts = monitor.mounts();
    // `with_capacity` reserves room for every disk up front, avoiding regrowth.
    let mut found = Vec::with_capacity(mounts.len() + 1);
    // Windows has no single root: GIO lists every drive (C:, D:, ...) itself.
    if cfg!(unix) {
        found.push(Disk::new(
            ROOT_DISK_NAME.to_owned(),
            SidePath::Local(root.to_path_buf()),
            ROOT_DISK_ICON.to_owned(),
        ));
    }
    found.extend(mounts.iter().filter_map(|mount| {
        // `?` on an `Option` returns `None` from the closure when there is no local path.
        let path = mount.root().path()?;
        // The icon is kept as GIO's text form, so the core crate needs no GIO types.
        let icon = IconExt::to_string(&mount.icon())
            .filter(|text| icon_loadable(text))
            .map_or_else(|| ROOT_DISK_ICON.to_owned(), |text| text.to_string());
        (path != root).then(|| Disk::new(mount.name().into(), SidePath::Local(path), icon))
    }));
    found
}

/// Whether GTK can show a disk's icon, given in GIO's text form. On Windows GIO names
/// icons stored inside system files, such as `C:\Windows\system32\imageres.dll,-36`
/// (icon 36 of that file); read back, that text is a file that does not exist.
fn icon_loadable(text: &str) -> bool {
    gio::Icon::for_string(text).is_ok_and(|icon| {
        icon.downcast_ref::<gio::FileIcon>()
            .is_none_or(|icon| icon.file().path().is_some_and(|path| path.is_file()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_inside_system_files_are_not_loadable() {
        let temp = tempfile::tempdir().expect("temp dir");
        let picture = temp.path().join("disk.png");
        std::fs::write(&picture, b"").expect("write");
        let missing = format!("{},-36", temp.path().join("imageres.dll").display());

        assert!(icon_loadable(ROOT_DISK_ICON), "a theme icon");
        assert!(
            icon_loadable(&picture.display().to_string()),
            "an icon file"
        );
        assert!(!icon_loadable(&missing), "icon 36 of a system file");
    }
}
