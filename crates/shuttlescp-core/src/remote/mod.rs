//! Remote file access: connecting and logging in, browsing, and the file operations
//! transfers use, over two families of protocols:
//!
//! * SFTP over SSH ([`sftp`]). "SCP" sites browse with SFTP too, as WinSCP does: the
//!   SCP protocol itself can copy files but cannot list folders.
//! * FTP, plain or with TLS (FTPS, explicit or implicit) ([`ftp`]); server certificates
//!   are checked by [`tls`].
//!
//! Front ends see one [`Remote`] type whichever protocol is in use.

mod drives;
mod ftp;
mod sftp;
mod tls;

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use secrecy::SecretString;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::entry::{EntryKind, FileEntry};
use crate::sites::{Protocol, Site};

/// A server's SSH host key. Re-exported so front ends can pass keys around (for
/// example after the user agrees to trust one) without depending on the SSH library.
pub use russh::keys::PublicKey as HostKey;
pub use sftp::fingerprint;
pub use tls::trust_certificate;

/// Give up connecting after this long.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Why connecting failed.
#[derive(Debug, Error)]
pub enum ConnectError {
    /// The site's protocol cannot be used yet.
    #[error("{0} connections are not supported yet")]
    Unsupported(&'static str),
    /// The server did not answer in time.
    #[error("{0} did not respond (timed out)")]
    Timeout(String),
    /// The server could not be reached, or the connection broke.
    #[error("cannot connect to {host}: {reason}")]
    Network {
        /// Server address.
        host: String,
        /// What went wrong.
        reason: String,
    },
    /// The server's SSH key is not in `known_hosts`; the user must decide whether to
    /// trust it.
    #[error("the server's host key is not known yet ({fingerprint})")]
    UnknownHostKey {
        /// SHA256 fingerprint to show the user.
        fingerprint: String,
        /// The key, to trust and remember if the user agrees.
        key: Box<HostKey>,
    },
    /// The server's SSH key differs from the one recorded: possibly an attack.
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
    /// The server's TLS certificate is not signed by an authority this system trusts
    /// (often a self-signed certificate) and was not trusted before; the user must decide
    /// whether to trust it (see [`trust_certificate`]).
    #[error("the server's certificate is not trusted ({reason})")]
    UntrustedCertificate {
        /// Server address.
        host: String,
        /// Server port.
        port: u16,
        /// SHA-256 fingerprint of the certificate, to show the user.
        fingerprint: String,
        /// Why it is not trusted.
        reason: String,
    },
    /// No password was typed or saved (and no key was tried).
    #[error("enter the password")]
    PasswordRequired,
    /// The server accepted none of the keys tried, and there is no password to try.
    #[error("the server did not accept the SSH key; enter the password, or check the key")]
    KeysRejected,
    /// The site's private key file is protected by a passphrase, and none was given
    /// (`wrong` false) or the one given does not open it (`wrong` true).
    #[error("the key file {} needs its passphrase", path.display())]
    KeyPassphraseRequired {
        /// The key file.
        path: PathBuf,
        /// A passphrase was tried and did not open the key.
        wrong: bool,
    },
    /// The site's private key file cannot be read, or is not a supported key.
    #[error("cannot use the key file {}: {reason}", path.display())]
    KeyUnreadable {
        /// The key file.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// The server rejected the user name, key or password.
    #[error("login failed: wrong user name, key or password")]
    AuthenticationFailed,
    /// The server does not offer SFTP, or it failed to start.
    #[error("the server's file transfer (SFTP) service failed: {0}")]
    Sftp(String),
    /// The FTP server refused something during login.
    #[error("FTP: {0}")]
    Ftp(String),
}

/// What to log in with, besides the site's key file and the SSH agent.
#[derive(Debug, Default)]
pub struct Credentials {
    /// The password, typed or saved; tried after any keys.
    pub password: Option<SecretString>,
    /// The passphrase of the site's key file, typed or saved.
    pub key_passphrase: Option<SecretString>,
}

/// What a listing or a lookup says about a file or folder on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RemoteMeta {
    /// A folder (or, when following links, a link to one).
    pub(crate) is_dir: bool,
    /// A symbolic link (only when not following links).
    pub(crate) is_symlink: bool,
    /// Bytes.
    pub(crate) size: u64,
    /// Last modification, if known.
    pub(crate) modified: Option<SystemTime>,
}

/// A server file being read.
pub(crate) type RemoteReader = Pin<Box<dyn AsyncRead + Send>>;
/// A server file being written. Shutting it down waits until the server has stored it.
pub(crate) type RemoteWriter = Pin<Box<dyn AsyncWrite + Send>>;

/// An open connection to a server. Cheap to clone and safe to use from background tasks.
#[derive(Clone)]
pub struct Remote(Arc<RemoteInner>);

struct RemoteInner {
    host: String,
    home: PathBuf,
    backend: Backend,
}

/// The protocol-specific half of a [`Remote`].
enum Backend {
    Sftp(sftp::Sftp),
    Ftp(ftp::Ftp),
}

// Written by hand: the protocol types do not implement `Debug`.
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

    /// List a folder on the server, sorted like local listings (folders first). A
    /// symbolic link to a folder is listed as a folder.
    ///
    /// # Arguments
    ///
    /// * `dir` - Absolute folder path on the server
    ///
    /// # Errors
    ///
    /// Returns a readable message if the folder cannot be read
    pub async fn list_dir(&self, dir: &Path) -> Result<Vec<FileEntry>, String> {
        let mut entries = Vec::new();
        for (name, meta) in self.read_dir_raw(dir).await? {
            // Listings describe a link itself; ask for its target to know whether it
            // leads to a folder.
            let is_dir = if meta.is_symlink {
                self.stat(&dir.join(&name))
                    .await
                    .is_ok_and(|target| target.is_dir)
            } else {
                meta.is_dir
            };
            entries.push(FileEntry::new(
                OsString::from(name),
                if is_dir {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                },
                meta.size,
                meta.modified,
            ));
        }
        entries.sort_unstable_by(FileEntry::listing_order);
        tracing::trace!(
            "read {} remote entries from {}",
            entries.len(),
            dir.display()
        );
        Ok(entries)
    }

    /// Details of `path`, following a symbolic link to what it points to.
    pub(crate) async fn stat(&self, path: &Path) -> Result<RemoteMeta, String> {
        match &self.0.backend {
            Backend::Sftp(sftp) => sftp.stat(path).await,
            Backend::Ftp(ftp) => ftp.stat(path).await,
        }
    }

    /// Details of `path` itself: a symbolic link is described as a link (where the
    /// protocol can tell).
    pub(crate) async fn lstat(&self, path: &Path) -> Result<RemoteMeta, String> {
        match &self.0.backend {
            Backend::Sftp(sftp) => sftp.lstat(path).await,
            // FTP has no separate "describe the link itself" lookup.
            Backend::Ftp(ftp) => ftp.stat(path).await,
        }
    }

    /// The entries of a folder (without "." and ".."), each described as itself:
    /// symbolic links are not followed.
    pub(crate) async fn read_dir_raw(
        &self,
        dir: &Path,
    ) -> Result<Vec<(String, RemoteMeta)>, String> {
        match &self.0.backend {
            Backend::Sftp(sftp) => sftp.read_dir(dir).await,
            Backend::Ftp(ftp) => ftp.read_dir(dir).await,
        }
    }

    /// Open a file for reading, starting `offset` bytes in.
    pub(crate) async fn open_at(&self, path: &Path, offset: u64) -> Result<RemoteReader, String> {
        match &self.0.backend {
            Backend::Sftp(sftp) => sftp.open_at(path, offset).await,
            Backend::Ftp(ftp) => ftp.open_at(path, offset).await,
        }
    }

    /// Open a file for writing from `offset` on: 0 creates it (or empties it); more
    /// keeps its first `offset` bytes (the file must be exactly that long) and continues
    /// after them.
    pub(crate) async fn open_write_at(
        &self,
        path: &Path,
        offset: u64,
    ) -> Result<RemoteWriter, String> {
        match &self.0.backend {
            Backend::Sftp(sftp) => sftp.open_write_at(path, offset).await,
            Backend::Ftp(ftp) => ftp.open_write_at(path, offset).await,
        }
    }

    /// Rename `from` to `to`. Most servers refuse if `to` exists.
    pub(crate) async fn rename(&self, from: &Path, to: &Path) -> Result<(), String> {
        match &self.0.backend {
            Backend::Sftp(sftp) => sftp.rename(from, to).await,
            Backend::Ftp(ftp) => ftp.rename(from, to).await,
        }
    }

    /// Create a folder; an existing folder is fine.
    pub(crate) async fn create_dir(&self, path: &Path) -> Result<(), String> {
        if self.stat(path).await.is_ok_and(|meta| meta.is_dir) {
            return Ok(());
        }
        match &self.0.backend {
            Backend::Sftp(sftp) => sftp.create_dir(path).await,
            Backend::Ftp(ftp) => ftp.create_dir(path).await,
        }
    }

    /// Delete a file (or a symbolic link itself).
    pub(crate) async fn remove_file(&self, path: &Path) -> Result<(), String> {
        match &self.0.backend {
            Backend::Sftp(sftp) => sftp.remove_file(path).await,
            Backend::Ftp(ftp) => ftp.remove_file(path).await,
        }
    }

    /// Delete an empty folder.
    pub(crate) async fn remove_dir(&self, path: &Path) -> Result<(), String> {
        match &self.0.backend {
            Backend::Sftp(sftp) => sftp.remove_dir(path).await,
            Backend::Ftp(ftp) => ftp.remove_dir(path).await,
        }
    }

    /// Set a file's modification time. FTP servers that do not offer it (the MFMT
    /// command) report an error, which callers treat as harmless.
    pub(crate) async fn set_modified(&self, path: &Path, time: SystemTime) -> Result<(), String> {
        match &self.0.backend {
            Backend::Sftp(sftp) => sftp.set_modified(path, time).await,
            Backend::Ftp(ftp) => ftp.set_modified(path, time).await,
        }
    }
}

/// A server path as the text the protocols expect.
fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Connect to `site`, check its identity (SSH host key, or TLS certificate for FTPS),
/// and log in.
///
/// # Arguments
///
/// * `site` - Server details
/// * `credentials` - Password (and, for SSH, the key file's passphrase) to log in with
/// * `approved_key` - An SSH host key the user has just agreed to trust (after an
///   [`ConnectError::UnknownHostKey`]); it is added to `known_hosts`
///
/// # Errors
///
/// Returns a [`ConnectError`] describing the step that failed
pub async fn connect(
    site: &Site,
    credentials: Credentials,
    approved_key: Option<HostKey>,
) -> Result<Remote, ConnectError> {
    let (backend, home) = match site.protocol {
        Protocol::Sftp | Protocol::Scp => {
            let (sftp, home) = sftp::connect(site, credentials, approved_key).await?;
            (Backend::Sftp(sftp), home)
        }
        Protocol::Ftp => {
            let (ftp, home) = ftp::connect(site, credentials.password).await?;
            (Backend::Ftp(ftp), home)
        }
    };
    tracing::info!(
        "connected to {} as {}; home is {}",
        site.host,
        site.user,
        home.display()
    );
    Ok(Remote(Arc::new(RemoteInner {
        host: site.host.clone(),
        home,
        backend,
    })))
}
