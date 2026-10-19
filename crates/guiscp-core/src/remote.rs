//! Remote file access over SSH: connecting, verifying the server's host key against
//! `~/.ssh/known_hosts`, logging in, and listing folders with SFTP.
//!
//! Both "SFTP" and "SCP" sites browse with SFTP over the SSH connection, as WinSCP does:
//! the SCP protocol itself can copy files but cannot list folders.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use russh::client;
use russh::keys::known_hosts::learn_known_hosts;
use russh::keys::{HashAlg, PublicKeyOrCertificate, check_known_hosts};
use russh_sftp::client::fs::{File, Metadata};
use russh_sftp::protocol::{FileAttributes, OpenFlags};

/// A server's SSH host key. Re-exported so front ends can pass keys around (for
/// example after the user agrees to trust one) without depending on the SSH library.
pub use russh::keys::PublicKey as HostKey;
use russh_sftp::client::SftpSession;
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;

use crate::entry::{EntryKind, FileEntry};
use crate::sites::{Protocol, Site};

/// Give up connecting after this long.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Why connecting failed.
#[derive(Debug, Error)]
pub enum ConnectError {
    /// The site's protocol cannot be browsed yet.
    #[error("{0} connections are not supported yet")]
    Unsupported(&'static str),
    /// The server did not answer in time.
    #[error("{0} did not respond (timed out)")]
    Timeout(String),
    /// The server could not be reached, or the SSH connection broke.
    #[error("cannot connect to {host}: {reason}")]
    Network {
        /// Server address.
        host: String,
        /// What went wrong.
        reason: String,
    },
    /// The server's key is not in `known_hosts`; the user must decide whether to trust it.
    #[error("the server's host key is not known yet ({fingerprint})")]
    UnknownHostKey {
        /// SHA256 fingerprint to show the user.
        fingerprint: String,
        /// The key, to trust and remember if the user agrees.
        key: Box<HostKey>,
    },
    /// The server's key differs from the one recorded: possibly an attack.
    #[error(
        "WARNING: the host key of {host} has changed (known_hosts line {line}). \
         Someone could be intercepting the connection; refusing to connect."
    )]
    HostKeyChanged {
        /// Server address.
        host: String,
        /// Line of `known_hosts` holding the old key.
        line: usize,
    },
    /// No password was typed or saved.
    #[error("enter the password")]
    PasswordRequired,
    /// The server rejected the user name or password.
    #[error("login failed: wrong user name or password")]
    AuthenticationFailed,
    /// The server does not offer SFTP, or it failed to start.
    #[error("the server's file transfer (SFTP) service failed: {0}")]
    Sftp(String),
}

/// What the host key check found, recorded by the SSH handler for after the handshake.
#[derive(Debug)]
enum HostKeyVerdict {
    Unknown(HostKey),
    Changed(usize),
    Failed(String),
}

/// SSH event handler: decides whether to trust the server's host key.
struct HostKeyCheck {
    host: String,
    port: u16,
    /// A key the user has just agreed to trust (it gets recorded in `known_hosts`).
    approved: Option<HostKey>,
    /// Shared with `connect`, which turns a rejection into a meaningful error.
    verdict: Arc<Mutex<Option<HostKeyVerdict>>>,
}

impl HostKeyCheck {
    /// Remember why the key was rejected.
    fn reject(&self, verdict: HostKeyVerdict) -> Result<bool, russh::Error> {
        if let Ok(mut slot) = self.verdict.lock() {
            *slot = Some(verdict);
        }
        Ok(false)
    }
}

impl client::Handler for HostKeyCheck {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_key else {
            return self.reject(HostKeyVerdict::Failed(String::from(
                "host certificates are not supported",
            )));
        };
        if self.approved.as_ref() == Some(key) {
            if let Err(error) = learn_known_hosts(&self.host, self.port, key) {
                tracing::warn!("could not record the host key in known_hosts: {error}");
            }
            tracing::info!("trusted new host key for {}", self.host);
            return Ok(true);
        }
        match check_known_hosts(&self.host, self.port, key) {
            Ok(true) => Ok(true),
            Ok(false) => self.reject(HostKeyVerdict::Unknown(key.clone())),
            Err(russh::keys::Error::KeyChanged { line }) => {
                self.reject(HostKeyVerdict::Changed(line))
            }
            Err(error) => self.reject(HostKeyVerdict::Failed(error.to_string())),
        }
    }
}

/// An open SFTP session. Cheap to clone and safe to use from background tasks.
#[derive(Clone)]
pub struct Remote(Arc<RemoteInner>);

struct RemoteInner {
    sftp: SftpSession,
    /// Keeps the SSH connection open for as long as the session is in use.
    _ssh: client::Handle<HostKeyCheck>,
    host: String,
    home: PathBuf,
}

// Written by hand: the SSH and SFTP types do not implement `Debug`.
impl fmt::Debug for Remote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Remote")
            .field("host", &self.0.host)
            .field("home", &self.0.home)
            .finish_non_exhaustive()
    }
}

impl Remote {
    /// Server host name, as entered in the site.
    pub fn host(&self) -> &str {
        &self.0.host
    }

    /// The user's home folder on the server, where browsing starts.
    pub fn home(&self) -> &Path {
        &self.0.home
    }

    /// List a folder on the server, sorted like local listings (folders first).
    ///
    /// # Arguments
    ///
    /// * `dir` - Absolute folder path on the server
    ///
    /// # Errors
    ///
    /// Returns a readable message if the folder cannot be read
    pub async fn list_dir(&self, dir: &Path) -> Result<Vec<FileEntry>, String> {
        let dir_text = dir.to_string_lossy().into_owned();
        let listing = self
            .0
            .sftp
            .read_dir(dir_text.clone())
            .await
            .map_err(|error| format!("cannot open {dir_text}: {error}"))?;
        let mut entries = Vec::new();
        for entry in listing {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let metadata = entry.metadata();
            // Listings describe a symlink itself; ask for its target to know whether it
            // leads to a folder.
            let is_dir = if metadata.is_symlink() {
                let target = dir.join(&name).to_string_lossy().into_owned();
                self.0
                    .sftp
                    .metadata(target)
                    .await
                    .is_ok_and(|target| target.is_dir())
            } else {
                metadata.is_dir()
            };
            entries.push(FileEntry::new(
                OsString::from(name),
                if is_dir {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                },
                metadata.size.unwrap_or(0),
                metadata
                    .mtime
                    .map(|seconds| UNIX_EPOCH + Duration::from_secs(u64::from(seconds))),
            ));
        }
        entries.sort_unstable_by(FileEntry::listing_order);
        tracing::trace!("read {} remote entries from {dir_text}", entries.len());
        Ok(entries)
    }
}

/// File operations used by transfers. Paths are absolute paths on the server; errors are
/// readable messages naming the path.
impl Remote {
    /// Details of `path`, following a symbolic link to what it points to.
    pub(crate) async fn stat(&self, path: &Path) -> Result<Metadata, String> {
        let text = path_text(path);
        self.0
            .sftp
            .metadata(text.clone())
            .await
            .map_err(|error| format!("cannot read {text}: {error}"))
    }

    /// Details of `path` itself: a symbolic link is described as a link.
    pub(crate) async fn lstat(&self, path: &Path) -> Result<Metadata, String> {
        let text = path_text(path);
        self.0
            .sftp
            .symlink_metadata(text.clone())
            .await
            .map_err(|error| format!("cannot read {text}: {error}"))
    }

    /// The entries of a folder (without "." and ".."), each described as itself:
    /// symbolic links are not followed.
    pub(crate) async fn read_dir_raw(&self, dir: &Path) -> Result<Vec<(String, Metadata)>, String> {
        let text = path_text(dir);
        let listing = self
            .0
            .sftp
            .read_dir(text.clone())
            .await
            .map_err(|error| format!("cannot open {text}: {error}"))?;
        Ok(listing
            .filter(|entry| entry.file_name() != "." && entry.file_name() != "..")
            .map(|entry| (entry.file_name(), entry.metadata()))
            .collect())
    }

    /// Open a file for reading.
    pub(crate) async fn open(&self, path: &Path) -> Result<File, String> {
        let text = path_text(path);
        self.0
            .sftp
            .open(text.clone())
            .await
            .map_err(|error| format!("cannot open {text}: {error}"))
    }

    /// Open a file for writing, creating it if needed; `truncate` empties an existing
    /// file, otherwise its contents are kept (to continue writing after them).
    pub(crate) async fn open_write(&self, path: &Path, truncate: bool) -> Result<File, String> {
        let text = path_text(path);
        let mut flags = OpenFlags::CREATE | OpenFlags::WRITE;
        if truncate {
            flags |= OpenFlags::TRUNCATE;
        }
        self.0
            .sftp
            .open_with_flags(text.clone(), flags)
            .await
            .map_err(|error| format!("cannot write {text}: {error}"))
    }

    /// Rename `from` to `to`. Most servers refuse if `to` exists.
    pub(crate) async fn rename(&self, from: &Path, to: &Path) -> Result<(), String> {
        let (from_text, to_text) = (path_text(from), path_text(to));
        self.0
            .sftp
            .rename(from_text.clone(), to_text.clone())
            .await
            .map_err(|error| format!("cannot rename {from_text} to {to_text}: {error}"))
    }

    /// Create a folder; an existing folder is fine.
    pub(crate) async fn create_dir(&self, path: &Path) -> Result<(), String> {
        if self
            .stat(path)
            .await
            .is_ok_and(|metadata| metadata.is_dir())
        {
            return Ok(());
        }
        let text = path_text(path);
        self.0
            .sftp
            .create_dir(text.clone())
            .await
            .map_err(|error| format!("cannot create folder {text}: {error}"))
    }

    /// Delete a file (or a symbolic link itself).
    pub(crate) async fn remove_file(&self, path: &Path) -> Result<(), String> {
        let text = path_text(path);
        self.0
            .sftp
            .remove_file(text.clone())
            .await
            .map_err(|error| format!("cannot delete {text}: {error}"))
    }

    /// Delete an empty folder.
    pub(crate) async fn remove_dir(&self, path: &Path) -> Result<(), String> {
        let text = path_text(path);
        self.0
            .sftp
            .remove_dir(text.clone())
            .await
            .map_err(|error| format!("cannot delete folder {text}: {error}"))
    }

    /// Set a file's modification (and access) time.
    pub(crate) async fn set_modified(&self, path: &Path, seconds: u32) -> Result<(), String> {
        let text = path_text(path);
        let attributes = FileAttributes {
            atime: Some(seconds),
            mtime: Some(seconds),
            ..FileAttributes::empty()
        };
        self.0
            .sftp
            .set_metadata(text.clone(), attributes)
            .await
            .map_err(|error| format!("cannot set the time of {text}: {error}"))
    }
}

/// A server path as the text SFTP expects.
fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// SHA256 fingerprint of a host key, as `ssh` shows it (e.g. "SHA256:abc...").
///
/// # Arguments
///
/// * `key` - Host key
pub fn fingerprint(key: &HostKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

/// Connect to `site`, check its host key, log in and start SFTP.
///
/// # Arguments
///
/// * `site` - Server details
/// * `password` - Password to log in with
/// * `approved_key` - A host key the user has just agreed to trust (after an
///   [`ConnectError::UnknownHostKey`]); it is added to `known_hosts`
///
/// # Errors
///
/// Returns a [`ConnectError`] describing the step that failed
pub async fn connect(
    site: &Site,
    password: Option<SecretString>,
    approved_key: Option<HostKey>,
) -> Result<Remote, ConnectError> {
    if site.protocol == Protocol::Ftp {
        return Err(ConnectError::Unsupported(Protocol::Ftp.label()));
    }
    let verdict = Arc::new(Mutex::new(None));
    let handler = HostKeyCheck {
        host: site.host.clone(),
        port: site.port,
        approved: approved_key,
        verdict: Arc::clone(&verdict),
    };
    tracing::info!("connecting to {}:{}", site.host, site.port);
    let connecting = client::connect(
        Arc::new(client::Config::default()),
        (site.host.as_str(), site.port),
        handler,
    );
    let mut ssh = match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
        Err(_) => return Err(ConnectError::Timeout(site.host.clone())),
        Ok(Err(error)) => {
            let recorded = verdict.lock().ok().and_then(|mut slot| slot.take());
            return Err(match recorded {
                Some(HostKeyVerdict::Unknown(key)) => ConnectError::UnknownHostKey {
                    fingerprint: fingerprint(&key),
                    key: Box::new(key),
                },
                Some(HostKeyVerdict::Changed(line)) => ConnectError::HostKeyChanged {
                    host: site.host.clone(),
                    line,
                },
                Some(HostKeyVerdict::Failed(reason)) => ConnectError::Network {
                    host: site.host.clone(),
                    reason,
                },
                None => ConnectError::Network {
                    host: site.host.clone(),
                    reason: error.to_string(),
                },
            });
        }
        Ok(Ok(ssh)) => ssh,
    };

    let password = password.ok_or(ConnectError::PasswordRequired)?;
    let network = |error: russh::Error| ConnectError::Network {
        host: site.host.clone(),
        reason: error.to_string(),
    };
    let auth = ssh
        .authenticate_password(site.user.as_str(), password.expose_secret())
        .await
        .map_err(network)?;
    if !auth.success() {
        return Err(ConnectError::AuthenticationFailed);
    }
    let channel = ssh.channel_open_session().await.map_err(network)?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(network)?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|error| ConnectError::Sftp(error.to_string()))?;
    let home = sftp
        .canonicalize(".")
        .await
        .map_err(|error| ConnectError::Sftp(error.to_string()))?;
    tracing::info!(
        "connected to {} as {}; home is {home}",
        site.host,
        site.user
    );
    Ok(Remote(Arc::new(RemoteInner {
        sftp,
        _ssh: ssh,
        host: site.host.clone(),
        home: PathBuf::from(home),
    })))
}
