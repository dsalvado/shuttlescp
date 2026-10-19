//! Browsable disks (mounted filesystems). How disks are discovered depends on the
//! platform and front end (the GTK front end asks GIO, like the file manager does);
//! this module holds what is common: the disk record and which disk holds a folder.

use std::path::{Path, PathBuf};

/// A browsable disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    name: String,
    root: PathBuf,
    icon: String,
}

impl Disk {
    /// Create a disk entry.
    ///
    /// # Arguments
    ///
    /// * `name` - Label shown to the user
    /// * `root` - Mount point
    /// * `icon` - Icon reference in a form the front end understands (for GTK, a
    ///   serialized `GIcon` such as a theme icon name)
    pub fn new(name: String, root: PathBuf, icon: String) -> Self {
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

    /// Icon reference given to [`Disk::new`].
    pub fn icon(&self) -> &str {
        &self.icon
    }
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
            String::from("drive-harddisk"),
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
