//! Browsable disks (mounted filesystems), on this computer or on the server. How local
//! disks are discovered depends on the platform and front end (the GTK front end asks
//! GIO, like the file manager does); a server's are found by [`crate::remote`]. This
//! module holds what is common: the disk record and which disk holds a folder.

use crate::path::SidePath;

/// A browsable disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    name: String,
    root: SidePath,
    icon: String,
}

impl Disk {
    /// Create a disk entry.
    ///
    /// # Arguments
    ///
    /// * `name` - Label shown to the user
    /// * `root` - Mount point (or drive, such as `C:\`)
    /// * `icon` - Icon reference in a form the front end understands (for GTK, a
    ///   serialized `GIcon` such as a theme icon name)
    pub fn new(name: String, root: SidePath, icon: String) -> Self {
        Self { name, root, icon }
    }

    /// Label shown to the user, e.g. "/" or a USB drive's volume name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Mount point of the disk.
    pub fn root(&self) -> &SidePath {
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
pub fn disk_index(disks: &[Disk], dir: &SidePath) -> Option<usize> {
    disks
        .iter()
        .enumerate()
        // `starts_with` compares whole components, so /media/usb does not match
        // /media/usb2.
        .filter(|(_, disk)| dir.starts_with(&disk.root))
        .max_by_key(|(_, disk)| depth(&disk.root))
        .map(|(index, _)| index)
}

/// How many folders deep `path` is.
fn depth(path: &SidePath) -> usize {
    match path {
        SidePath::Local(path) => path.components().count(),
        SidePath::Remote(path) => path
            .as_str()
            .split('/')
            .filter(|part| !part.is_empty())
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::path::RemotePath;

    fn local(path: &str) -> SidePath {
        SidePath::Local(PathBuf::from(path))
    }

    fn disk(root: &str) -> Disk {
        Disk::new(root.to_owned(), local(root), String::from("drive-harddisk"))
    }

    #[test]
    fn disk_index_prefers_the_deepest_mount() {
        // Arrange
        let disks = [disk("/"), disk("/media/me/usb"), disk("/media/me/usb2")];

        // Act + Assert
        assert_eq!(disk_index(&disks, &local("/home/me")), Some(0));
        assert_eq!(disk_index(&disks, &local("/media/me/usb/photos")), Some(1));
        assert_eq!(disk_index(&disks, &local("/media/me/usb2")), Some(2));
        assert_eq!(disk_index(&disks, &local("/media/me")), Some(0));
    }

    #[test]
    fn disk_index_is_none_without_a_matching_disk() {
        let disks = [disk("/media/me/usb")];
        assert_eq!(disk_index(&disks, &local("/home")), None);
    }

    #[test]
    fn disk_index_finds_server_drives() {
        let drive = |root: &str| {
            Disk::new(
                root.to_owned(),
                SidePath::Remote(RemotePath::new(root)),
                String::from("drive-harddisk"),
            )
        };
        let disks = [drive("/"), drive("/mnt/data")];
        let dir = |path: &str| SidePath::Remote(RemotePath::new(path));

        assert_eq!(disk_index(&disks, &dir("/mnt/data/photos")), Some(1));
        assert_eq!(disk_index(&disks, &dir("/home")), Some(0));
        assert_eq!(disk_index(&disks, &local("/mnt/data")), None);
    }
}
