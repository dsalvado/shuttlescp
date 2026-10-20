//! File access on one side of a transfer: this computer or the server.

use std::ffi::OsString;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::SystemTime;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite};

use crate::remote::{Remote, RemoteMeta};

/// What a walk needs to know about a file or folder.
#[derive(Debug, Clone)]
pub(super) struct Node {
    pub(super) is_dir: bool,
    pub(super) size: u64,
    pub(super) modified: Option<SystemTime>,
}

/// File access on one side of a transfer.
#[derive(Clone)]
pub(super) enum Endpoint {
    Local,
    Remote(Remote),
}

/// A file being read, local or remote.
pub(super) type Reader = Pin<Box<dyn AsyncRead + Send>>;
/// A file being written, local or remote.
pub(super) type Writer = Pin<Box<dyn AsyncWrite + Send>>;

impl Endpoint {
    /// The endpoint for files on the server if `remote` is given, otherwise local.
    pub(super) fn of(remote: Option<&Remote>) -> Self {
        remote.map_or(Self::Local, |remote| Self::Remote(remote.clone()))
    }

    /// Details of `path`; `follow` describes what a symbolic link points to instead of
    /// the link itself.
    pub(super) async fn stat(&self, path: &Path, follow: bool) -> Result<Node, String> {
        match self {
            Self::Local => {
                let metadata = if follow {
                    tokio::fs::metadata(path).await
                } else {
                    tokio::fs::symlink_metadata(path).await
                }
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                Ok(Node {
                    is_dir: metadata.is_dir(),
                    size: metadata.len(),
                    modified: metadata.modified().ok(),
                })
            }
            Self::Remote(remote) => {
                let metadata = if follow {
                    remote.stat(path).await?
                } else {
                    remote.lstat(path).await?
                };
                Ok(remote_node(&metadata))
            }
        }
    }

    /// Whether anything (even a broken symbolic link) is at `path`.
    pub(super) async fn exists(&self, path: &Path) -> bool {
        self.stat(path, false).await.is_ok()
    }

    /// The entries of a folder; see [`Endpoint::stat`] for `follow`. A link that cannot
    /// be followed (e.g. broken) is listed as itself.
    pub(super) async fn children(
        &self,
        dir: &Path,
        follow: bool,
    ) -> Result<Vec<(OsString, Node)>, String> {
        let mut children = Vec::new();
        match self {
            Self::Local => {
                let unreadable = |error| format!("cannot open {}: {error}", dir.display());
                let mut entries = tokio::fs::read_dir(dir).await.map_err(unreadable)?;
                while let Some(entry) = entries.next_entry().await.map_err(unreadable)? {
                    let path = entry.path();
                    let mut node = self.stat(&path, false).await?;
                    if follow && entry.file_type().await.is_ok_and(|kind| kind.is_symlink()) {
                        node = self.stat(&path, true).await.unwrap_or(node);
                    }
                    children.push((entry.file_name(), node));
                }
            }
            Self::Remote(remote) => {
                for (name, metadata) in remote.read_dir_raw(dir).await? {
                    let mut node = remote_node(&metadata);
                    if follow && metadata.is_symlink {
                        node = self.stat(&dir.join(&name), true).await.unwrap_or(node);
                    }
                    children.push((OsString::from(name), node));
                }
            }
        }
        Ok(children)
    }

    /// Open a file for reading, starting `offset` bytes in.
    pub(super) async fn open_at(&self, path: &Path, offset: u64) -> Result<Reader, String> {
        let seek_failed = |error| format!("cannot read {}: {error}", path.display());
        Ok(match self {
            Self::Local => {
                let mut file = tokio::fs::File::open(path)
                    .await
                    .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
                if offset > 0 {
                    file.seek(SeekFrom::Start(offset))
                        .await
                        .map_err(seek_failed)?;
                }
                Box::pin(file)
            }
            Self::Remote(remote) => remote.open_at(path, offset).await?,
        })
    }

    /// Open a file for writing from `offset` on: 0 creates it (or empties it); more
    /// keeps its first `offset` bytes and continues after them.
    pub(super) async fn open_write_at(&self, path: &Path, offset: u64) -> Result<Writer, String> {
        let failed = |error| format!("cannot write {}: {error}", path.display());
        Ok(match self {
            Self::Local => {
                let mut file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(offset == 0)
                    .open(path)
                    .await
                    .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
                if offset > 0 {
                    // Drops anything past `offset`, in case the file is longer.
                    file.set_len(offset).await.map_err(failed)?;
                    file.seek(SeekFrom::Start(offset)).await.map_err(failed)?;
                }
                Box::pin(file)
            }
            Self::Remote(remote) => remote.open_write_at(path, offset).await?,
        })
    }

    /// Read `len` bytes starting at `offset`.
    pub(super) async fn read_range(
        &self,
        path: &Path,
        offset: u64,
        len: usize,
    ) -> Result<Vec<u8>, String> {
        let mut reader = self.open_at(path, offset).await?;
        let mut bytes = vec![0; len];
        reader
            .read_exact(&mut bytes)
            .await
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        Ok(bytes)
    }

    /// Create a folder; an existing folder is fine.
    pub(super) async fn create_dir(&self, path: &Path) -> Result<(), String> {
        match self {
            Self::Local => match tokio::fs::create_dir(path).await {
                Err(error)
                    if !tokio::fs::metadata(path)
                        .await
                        .is_ok_and(|meta| meta.is_dir()) =>
                {
                    Err(format!("cannot create folder {}: {error}", path.display()))
                }
                _ => Ok(()),
            },
            Self::Remote(remote) => remote.create_dir(path).await,
        }
    }

    /// Delete a file (or a symbolic link itself).
    pub(super) async fn remove_file(&self, path: &Path) -> Result<(), String> {
        match self {
            Self::Local => tokio::fs::remove_file(path)
                .await
                .map_err(|error| format!("cannot delete {}: {error}", path.display())),
            Self::Remote(remote) => remote.remove_file(path).await,
        }
    }

    /// Delete an empty folder.
    pub(super) async fn remove_dir(&self, path: &Path) -> Result<(), String> {
        match self {
            Self::Local => tokio::fs::remove_dir(path)
                .await
                .map_err(|error| format!("cannot delete folder {}: {error}", path.display())),
            Self::Remote(remote) => remote.remove_dir(path).await,
        }
    }

    /// Rename (or move within the same side) `from` to `to`. Callers make sure `to`
    /// does not exist: servers differ on whether renaming onto a file replaces it.
    pub(super) async fn rename(&self, from: &Path, to: &Path) -> Result<(), String> {
        match self {
            Self::Local => tokio::fs::rename(from, to).await.map_err(|error| {
                format!(
                    "cannot rename {} to {}: {error}",
                    from.display(),
                    to.display()
                )
            }),
            Self::Remote(remote) => remote.rename(from, to).await,
        }
    }

    /// Set a file's modification time.
    pub(super) async fn set_modified(&self, path: &Path, time: SystemTime) -> Result<(), String> {
        match self {
            Self::Local => {
                let path = path.to_path_buf();
                tokio::task::spawn_blocking(move || {
                    std::fs::File::options()
                        .write(true)
                        .open(&path)
                        .and_then(|file| file.set_modified(time))
                        .map_err(|error| {
                            format!("cannot set the time of {}: {error}", path.display())
                        })
                })
                .await
                .map_err(|error| error.to_string())?
            }
            Self::Remote(remote) => remote.set_modified(path, time).await,
        }
    }
}

/// Where a file is written while being copied: its name plus [`super::PARTIAL_SUFFIX`].
pub(super) fn partial_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().map(OsString::from).unwrap_or_default();
    name.push(super::PARTIAL_SUFFIX);
    dest.with_file_name(name)
}

/// A [`Node`] from what the server reports.
fn remote_node(metadata: &RemoteMeta) -> Node {
    Node {
        is_dir: metadata.is_dir,
        size: metadata.size,
        modified: metadata.modified,
    }
}
