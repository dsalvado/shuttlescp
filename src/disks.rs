//! The disks (mounted filesystems) a user can browse, as the desktop's file manager sees
//! them: the root filesystem plus every user-visible mount such as USB drives.

use std::path::{Path, PathBuf};

use relm4::gtk::gio;
use relm4::gtk::prelude::*;

/// Display name of the root filesystem entry: its path, as Linux users know it.
const ROOT_DISK_NAME: &str = "/";
/// Theme icon of the root filesystem entry.
const ROOT_DISK_ICON: &str = "drive-harddisk";

/// A browsable disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Disk {
    name: String,
    root: PathBuf,
    /// `gio::Icon` is a reference-counted GObject, so cloning it is cheap.
    icon: gio::Icon,
}

impl Disk {
    /// Create a disk entry.
    ///
    /// # Arguments
    ///
    /// * `name` - Label shown to the user
    /// * `root` - Mount point
    /// * `icon` - Icon shown next to the label
    pub fn new(name: String, root: PathBuf, icon: gio::Icon) -> Self {
        Self { name, root, icon }
    }

    /// Label shown to the user, e.g. "/" or a USB drive's volume name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Mount point of the disk.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Icon the desktop uses for this disk.
    pub fn icon(&self) -> &gio::Icon {
        &self.icon
    }
}

/// List the root filesystem followed by every user-visible local mount.
///
/// # Arguments
///
/// * `monitor` - The GIO volume monitor (the file manager's source of mounts)
///
/// # Returns
///
/// Disks in display order; mounts without a local path (e.g. network shares) are skipped
pub fn list_disks(monitor: &gio::VolumeMonitor) -> Vec<Disk> {
    let root = Path::new("/");
    let mounts = monitor.mounts();
    // `with_capacity` reserves room for every disk up front, avoiding regrowth.
    let mut disks = Vec::with_capacity(mounts.len() + 1);
    disks.push(Disk::new(
        ROOT_DISK_NAME.to_owned(),
        root.to_path_buf(),
        gio::ThemedIcon::new(ROOT_DISK_ICON).upcast(),
    ));
    disks.extend(mounts.iter().filter_map(|mount| {
        // `?` on an `Option` returns `None` from the closure when there is no local path.
        let path = mount.root().path()?;
        (path != root).then(|| Disk::new(mount.name().into(), path, mount.icon()))
    }));
    disks
}

/// Find the disk holding `dir`: the one whose mount point is the longest prefix of it.
///
/// # Arguments
///
/// * `disks` - Disks to search
/// * `dir` - An absolute directory path
///
/// # Returns
///
/// Index into `disks`, or `None` if no disk contains `dir`
pub fn disk_index(disks: &[Disk], dir: &Path) -> Option<usize> {
    disks
        .iter()
        .enumerate()
        // `Path::starts_with` compares whole components, so /media/usb does not match
        // /media/usb2.
        .filter(|(_, disk)| dir.starts_with(&disk.root))
        .max_by_key(|(_, disk)| disk.root.components().count())
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(root: &str) -> Disk {
        Disk::new(
            root.to_owned(),
            PathBuf::from(root),
            gio::ThemedIcon::new(ROOT_DISK_ICON).upcast(),
        )
    }

    #[test]
    fn disk_index_prefers_the_deepest_mount() {
        // Arrange
        let disks = [disk("/"), disk("/media/me/usb"), disk("/media/me/usb2")];

        // Act + Assert
        assert_eq!(disk_index(&disks, Path::new("/home/me")), Some(0));
        assert_eq!(
            disk_index(&disks, Path::new("/media/me/usb/photos")),
            Some(1)
        );
        assert_eq!(disk_index(&disks, Path::new("/media/me/usb2")), Some(2));
        assert_eq!(disk_index(&disks, Path::new("/media/me")), Some(0));
    }

    #[test]
    fn disk_index_is_none_without_a_matching_disk() {
        let disks = [disk("/media/me/usb")];
        assert_eq!(disk_index(&disks, Path::new("/home")), None);
    }
}
