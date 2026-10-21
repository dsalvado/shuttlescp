//! The drives of a server, for the remote pane's drive drop-down.
//!
//! Neither SFTP nor FTP can ask a server for its disks, so they are found from what
//! the server shows, depending on its system:
//! - Linux and other Unix servers: the mounted filesystems in `/proc/mounts`, keeping
//!   disks (`/dev/...` devices) and network shares, not system filesystems.
//! - macOS servers: the folders in `/Volumes`.
//! - Windows servers (OpenSSH): the drive letters (`C:`, `D:`, ...) listed in `/`.
//!
//! The root `/` always comes first; on an FTP server it is often all there is.

use tokio::io::AsyncReadExt;

use super::Remote;
use crate::disks::Disk;
use crate::path::{RemotePath, SidePath};

/// Icon (a theme icon name, see [`Disk::new`]) of a disk.
const DISK_ICON: &str = "drive-harddisk";
/// Icon of a removable disk (mounted under /media or /run/media).
const REMOVABLE_ICON: &str = "drive-removable-media";
/// Icon of a network share.
const NETWORK_ICON: &str = "network-server";
/// Largest mount table read, in bytes (a busy server's is a few kB).
const MOUNTS_LIMIT: u64 = 1024 * 1024;
/// Filesystem types of network shares.
const NETWORK_TYPES: [&str; 7] = ["nfs", "nfs4", "cifs", "smb3", "smbfs", "fuse.sshfs", "9p"];
/// Disk filesystem types that are not browsable data: snap packages and the like.
const SKIPPED_TYPES: [&str; 1] = ["squashfs"];

impl Remote {
    /// The server's drives: `/` first, then the others found (see the module notes).
    /// Anything that cannot be read is skipped, so there is always at least `/`.
    pub async fn drives(&self) -> Vec<Disk> {
        let mut drives = vec![Disk::new(
            String::from("/"),
            SidePath::Remote(RemotePath::root()),
            String::from(DISK_ICON),
        )];
        let found = match self.read_small_file(&RemotePath::new("/proc/mounts")).await {
            Ok(table) => self.existing_dirs(mounted_drives(&table)).await,
            Err(_) => Vec::new(),
        };
        let found = if found.is_empty() {
            self.mac_volumes().await
        } else {
            found
        };
        let found = if found.is_empty() {
            self.windows_drives().await
        } else {
            found
        };
        drives.extend(found);
        tracing::debug!("found {} drives on {}", drives.len(), self.host());
        drives
    }

    /// A small text file from the server, read whole.
    async fn read_small_file(&self, path: &RemotePath) -> Result<String, String> {
        let mut text = String::new();
        self.open_at(path, 0)
            .await?
            .take(MOUNTS_LIMIT)
            .read_to_string(&mut text)
            .await
            .map_err(|error| error.to_string())?;
        Ok(text)
    }

    /// The drives that are folders: a mount table also lists files bound over files.
    async fn existing_dirs(&self, drives: Vec<Disk>) -> Vec<Disk> {
        let mut dirs = Vec::with_capacity(drives.len());
        for drive in drives {
            let Some(root) = drive.root().as_remote() else {
                continue;
            };
            if self.stat(root).await.is_ok_and(|meta| meta.is_dir) {
                dirs.push(drive);
            }
        }
        dirs
    }

    /// macOS: the disks in /Volumes (the startup disk's entry there is a link to /).
    async fn mac_volumes(&self) -> Vec<Disk> {
        let volumes = RemotePath::new("/Volumes");
        let Ok(entries) = self.read_dir_raw(&volumes).await else {
            return Vec::new();
        };
        let mut drives: Vec<Disk> = entries
            .into_iter()
            .filter(|(_, meta)| meta.is_dir && !meta.is_symlink)
            .map(|(name, _)| {
                Disk::new(
                    name.clone(),
                    SidePath::Remote(volumes.join(&name)),
                    String::from(DISK_ICON),
                )
            })
            .collect();
        drives.sort_by(|a, b| a.name().cmp(b.name()));
        drives
    }

    /// Windows: the drive letters the server lists in /.
    async fn windows_drives(&self) -> Vec<Disk> {
        let Ok(entries) = self.read_dir_raw(&RemotePath::root()).await else {
            return Vec::new();
        };
        let mut drives: Vec<Disk> = entries
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| is_drive_letter(name))
            .map(|name| {
                Disk::new(
                    name.clone(),
                    SidePath::Remote(RemotePath::root().join(&name)),
                    String::from(DISK_ICON),
                )
            })
            .collect();
        drives.sort_by(|a, b| a.name().cmp(b.name()));
        drives
    }
}

/// The browsable drives in a Linux mount table (`/proc/mounts`), without `/`, sorted by
/// mount point; a mount point listed twice is kept once.
///
/// # Arguments
///
/// * `table` - The table: one mount per line, "device mount-point type options ..."
fn mounted_drives(table: &str) -> Vec<Disk> {
    let mut drives: Vec<Disk> = Vec::new();
    for line in table.lines() {
        let mut fields = line.split_whitespace();
        let (Some(device), Some(mount_point), Some(fs_type)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let icon = if NETWORK_TYPES.contains(&fs_type) {
            NETWORK_ICON
        } else if device.starts_with("/dev/") && !SKIPPED_TYPES.contains(&fs_type) {
            if mount_point.starts_with("/media/") || mount_point.starts_with("/run/media/") {
                REMOVABLE_ICON
            } else {
                DISK_ICON
            }
        } else {
            continue;
        };
        let mount_point = unescape_mount_point(mount_point);
        if mount_point == "/" || drives.iter().any(|drive| drive.name() == mount_point) {
            continue;
        }
        drives.push(Disk::new(
            mount_point.clone(),
            SidePath::Remote(RemotePath::new(mount_point)),
            String::from(icon),
        ));
    }
    drives.sort_by(|a, b| a.root().cmp(b.root()));
    drives
}

/// A mount point as written in the mount table, where spaces, tabs, newlines and
/// backslashes are octal escapes ("\040" for a space).
fn unescape_mount_point(text: &str) -> String {
    let mut unescaped = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('\\') {
        unescaped.push_str(&rest[..start]);
        let code = rest.get(start + 1..start + 4);
        match code.and_then(|digits| u8::from_str_radix(digits, 8).ok()) {
            Some(byte) => {
                unescaped.push(char::from(byte));
                rest = &rest[start + 4..];
            }
            None => {
                unescaped.push('\\');
                rest = &rest[start + 1..];
            }
        }
    }
    unescaped.push_str(rest);
    unescaped
}

/// Whether a name in a Windows server's / is a drive, such as "C:".
fn is_drive_letter(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(
        (characters.next(), characters.next(), characters.next()),
        (Some(letter), Some(':'), None) if letter.is_ascii_alphabetic()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounted_drives_keeps_disks_and_shares_only() {
        // Arrange: a typical Ubuntu server with a data disk, a USB stick, an NFS share
        // and the usual system and snap filesystems.
        let table = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0
/dev/sda2 / ext4 rw,relatime 0 0
tmpfs /run tmpfs rw,nosuid,nodev,size=1631368k,mode=755 0 0
/dev/loop0 /snap/core22/1380 squashfs ro,nodev,relatime 0 0
/dev/sda1 /boot/efi vfat rw,relatime 0 0
/dev/sdb1 /mnt/data xfs rw,relatime 0 0
/dev/sdc1 /media/me/USB\\040Stick vfat rw,nosuid,nodev 0 0
nas:/export /srv/nas nfs4 rw,relatime 0 0
overlay /var/lib/docker/overlay2/abc/merged overlay rw,relatime 0 0
/dev/sdb1 /mnt/data xfs rw,relatime 0 0
";

        // Act
        let drives = mounted_drives(table);

        // Assert
        let found: Vec<(&str, &str)> = drives
            .iter()
            .map(|drive| (drive.name(), drive.icon()))
            .collect();
        assert_eq!(
            found,
            [
                ("/boot/efi", DISK_ICON),
                ("/media/me/USB Stick", REMOVABLE_ICON),
                ("/mnt/data", DISK_ICON),
                ("/srv/nas", NETWORK_ICON),
            ]
        );
    }

    #[test]
    fn mount_points_are_unescaped() {
        assert_eq!(unescape_mount_point("/mnt/My\\040Disk"), "/mnt/My Disk");
        assert_eq!(unescape_mount_point("/mnt/tab\\011x"), "/mnt/tab\tx");
        assert_eq!(
            unescape_mount_point("/mnt/back\\134slash"),
            "/mnt/back\\slash"
        );
        // A backslash without three octal digits stays as it is.
        assert_eq!(unescape_mount_point("/mnt/odd\\x"), "/mnt/odd\\x");
    }

    #[test]
    fn drive_letters_are_letter_and_colon() {
        assert!(is_drive_letter("C:"));
        assert!(is_drive_letter("d:"));
        assert!(!is_drive_letter("C"));
        assert!(!is_drive_letter("CD:"));
        assert!(!is_drive_letter("1:"));
        assert!(!is_drive_letter("Users"));
    }
}
