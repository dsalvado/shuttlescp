//! File access on one side of a transfer: this computer or the server.

use std::ffi::OsString;
use std::io::SeekFrom;
use std::path::Path;
use std::pin::Pin;
use std::time::SystemTime;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite};

use crate::path::{RemotePath, SidePath};
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

/// A path together with the access it needs (see [`Endpoint::target`]).
enum Target<'a> {
    Local(&'a Path),
    Remote(&'a Remote, &'a RemotePath),
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

    /// Pair `path` with this endpoint's access to its side.
    ///
    /// # Errors
    ///
    /// Returns a message if `path` is on the other side (a mistake in the caller)
    fn target<'a>(&'a self, path: &'a SidePath) -> Result<Target<'a>, String> {
        match (self, path) {
            (Self::Local, SidePath::Local(path)) => Ok(Target::Local(path)),
            (Self::Remote(remote), SidePath::Remote(path)) => Ok(Target::Remote(remote, path)),
            (Self::Local, SidePath::Remote(_)) => Err(format!("{path} is not on this computer")),
            (Self::Remote(_), SidePath::Local(_)) => Err(format!("{path} is not on the server")),
        }
    }

    /// Details of `path`; `follow` describes what a symbolic link points to instead of
    /// the link itself.
    pub(super) async fn stat(&self, path: &SidePath, follow: bool) -> Result<Node, String> {
        match self.target(path)? {
            Target::Local(path) => {
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
            Target::Remote(remote, path) => {
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
    pub(super) async fn exists(&self, path: &SidePath) -> bool {
        self.stat(path, false).await.is_ok()
    }

    /// The entries of a folder; see [`Endpoint::stat`] for `follow`. A link that cannot
    /// be followed (e.g. broken) is listed as itself.
    pub(super) async fn children(
        &self,
        dir: &SidePath,
        follow: bool,
    ) -> Result<Vec<(OsString, Node)>, String> {
        let mut children = Vec::new();
        match self.target(dir)? {
            Target::Local(local_dir) => {
                let unreadable = |error| format!("cannot open {}: {error}", local_dir.display());
                let mut entries = tokio::fs::read_dir(local_dir).await.map_err(unreadable)?;
                while let Some(entry) = entries.next_entry().await.map_err(unreadable)? {
                    let path = SidePath::Local(entry.path());
                    let mut node = self.stat(&path, false).await?;
                    if follow && entry.file_type().await.is_ok_and(|kind| kind.is_symlink()) {
                        node = self.stat(&path, true).await.unwrap_or(node);
                    }
                    children.push((entry.file_name(), node));
                }
            }
            Target::Remote(remote, remote_dir) => {
                for (name, metadata) in remote.read_dir_raw(remote_dir).await? {
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
    pub(super) async fn open_at(&self, path: &SidePath, offset: u64) -> Result<Reader, String> {
        let seek_failed = |error| format!("cannot read {path}: {error}");
        Ok(match self.target(path)? {
            Target::Local(path) => {
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
            Target::Remote(remote, path) => remote.open_at(path, offset).await?,
        })
    }

    /// Open a file for writing from `offset` on: 0 creates it (or empties it); more
    /// keeps its first `offset` bytes and continues after them.
    pub(super) async fn open_write_at(
        &self,
        path: &SidePath,
        offset: u64,
    ) -> Result<Writer, String> {
        let failed = |error| format!("cannot write {path}: {error}");
        Ok(match self.target(path)? {
            Target::Local(path) => {
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
            Target::Remote(remote, path) => remote.open_write_at(path, offset).await?,
        })
    }

    /// Read `len` bytes starting at `offset`.
    pub(super) async fn read_range(
        &self,
        path: &SidePath,
        offset: u64,
        len: usize,
    ) -> Result<Vec<u8>, String> {
        let mut reader = self.open_at(path, offset).await?;
        let mut bytes = vec![0; len];
        reader
            .read_exact(&mut bytes)
            .await
            .map_err(|error| format!("cannot read {path}: {error}"))?;
        Ok(bytes)
    }

    /// Create a folder; an existing folder is fine.
    pub(super) async fn create_dir(&self, path: &SidePath) -> Result<(), String> {
        match self.target(path)? {
            Target::Local(path) => match tokio::fs::create_dir(path).await {
                Err(error)
                    if !tokio::fs::metadata(path)
                        .await
                        .is_ok_and(|meta| meta.is_dir()) =>
                {
                    Err(format!("cannot create folder {}: {error}", path.display()))
                }
                _ => Ok(()),
            },
            Target::Remote(remote, path) => remote.create_dir(path).await,
        }
    }

    /// Delete a file (or a symbolic link itself).
    pub(super) async fn remove_file(&self, path: &SidePath) -> Result<(), String> {
        match self.target(path)? {
            Target::Local(path) => tokio::fs::remove_file(path)
                .await
                .map_err(|error| format!("cannot delete {}: {error}", path.display())),
            Target::Remote(remote, path) => remote.remove_file(path).await,
        }
    }

    /// Delete an empty folder.
    pub(super) async fn remove_dir(&self, path: &SidePath) -> Result<(), String> {
        match self.target(path)? {
            Target::Local(path) => tokio::fs::remove_dir(path)
                .await
                .map_err(|error| format!("cannot delete folder {}: {error}", path.display())),
            Target::Remote(remote, path) => remote.remove_dir(path).await,
        }
    }

    /// Rename (or move within the same side) `from` to `to`. Callers make sure `to`
    /// does not exist: servers differ on whether renaming onto a file replaces it.
    pub(super) async fn rename(&self, from: &SidePath, to: &SidePath) -> Result<(), String> {
        match (self.target(from)?, self.target(to)?) {
            (Target::Local(from), Target::Local(to)) => {
                tokio::fs::rename(from, to).await.map_err(|error| {
                    format!(
                        "cannot rename {} to {}: {error}",
                        from.display(),
                        to.display()
                    )
                })
            }
            (Target::Remote(remote, from), Target::Remote(_, to)) => remote.rename(from, to).await,
            // `target` gives both the same side, since both are on this endpoint's.
            _ => Err(format!(
                "cannot rename {from} to {to}: not on the same side"
            )),
        }
    }

    /// Set a file's modification time.
    pub(super) async fn set_modified(
        &self,
        path: &SidePath,
        time: SystemTime,
    ) -> Result<(), String> {
        match self.target(path)? {
            Target::Local(path) => {
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
            Target::Remote(remote, path) => remote.set_modified(path, time).await,
        }
    }
}

/// Where a file is written while being copied: its name plus [`super::PARTIAL_SUFFIX`].
pub(super) fn partial_path(dest: &SidePath) -> SidePath {
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
