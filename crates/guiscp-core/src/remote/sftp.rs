//! SFTP over SSH: verifying the server's host key against `~/.ssh/known_hosts`, logging
//! in, and the file operations behind [`super::Remote`].

use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use russh::client;
use russh::keys::known_hosts::learn_known_hosts;
use russh::keys::{HashAlg, PublicKeyOrCertificate, check_known_hosts};
use russh_sftp::client::SftpSession;
use russh_sftp::client::fs::Metadata;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use secrecy::{ExposeSecret, SecretString};
use tokio::io::AsyncSeekExt;

use super::{
    CONNECT_TIMEOUT, ConnectError, HostKey, RemoteMeta, RemoteReader, RemoteWriter, path_text,
};
use crate::sites::Site;

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

/// An open SFTP session.
pub(super) struct Sftp {
    session: SftpSession,
    /// Keeps the SSH connection open for as long as the session is in use.
    _ssh: client::Handle<HostKeyCheck>,
}

impl Sftp {
    pub(super) async fn stat(&self, path: &Path) -> Result<RemoteMeta, String> {
        let text = path_text(path);
        self.session
            .metadata(text.clone())
            .await
            .map(|metadata| meta(&metadata))
            .map_err(|error| format!("cannot read {text}: {error}"))
    }

    pub(super) async fn lstat(&self, path: &Path) -> Result<RemoteMeta, String> {
        let text = path_text(path);
        self.session
            .symlink_metadata(text.clone())
            .await
            .map(|metadata| meta(&metadata))
            .map_err(|error| format!("cannot read {text}: {error}"))
    }

    pub(super) async fn read_dir(&self, dir: &Path) -> Result<Vec<(String, RemoteMeta)>, String> {
        let text = path_text(dir);
        let listing = self
            .session
            .read_dir(text.clone())
            .await
            .map_err(|error| format!("cannot open {text}: {error}"))?;
        Ok(listing
            .filter(|entry| entry.file_name() != "." && entry.file_name() != "..")
            .map(|entry| (entry.file_name(), meta(&entry.metadata())))
            .collect())
    }

    pub(super) async fn open_at(&self, path: &Path, offset: u64) -> Result<RemoteReader, String> {
        let text = path_text(path);
        let mut file = self
            .session
            .open(text.clone())
            .await
            .map_err(|error| format!("cannot open {text}: {error}"))?;
        if offset > 0 {
            file.seek(SeekFrom::Start(offset))
                .await
                .map_err(|error| format!("cannot read {text}: {error}"))?;
        }
        Ok(Box::pin(file))
    }

    pub(super) async fn open_write_at(
        &self,
        path: &Path,
        offset: u64,
    ) -> Result<RemoteWriter, String> {
        let text = path_text(path);
        let mut flags = OpenFlags::CREATE | OpenFlags::WRITE;
        if offset == 0 {
            flags |= OpenFlags::TRUNCATE;
        }
        let mut file = self
            .session
            .open_with_flags(text.clone(), flags)
            .await
            .map_err(|error| format!("cannot write {text}: {error}"))?;
        if offset > 0 {
            file.seek(SeekFrom::Start(offset))
                .await
                .map_err(|error| format!("cannot write {text}: {error}"))?;
        }
        Ok(Box::pin(file))
    }

    pub(super) async fn rename(&self, from: &Path, to: &Path) -> Result<(), String> {
        let (from_text, to_text) = (path_text(from), path_text(to));
        self.session
            .rename(from_text.clone(), to_text.clone())
            .await
            .map_err(|error| format!("cannot rename {from_text} to {to_text}: {error}"))
    }

    pub(super) async fn create_dir(&self, path: &Path) -> Result<(), String> {
        let text = path_text(path);
        self.session
            .create_dir(text.clone())
            .await
            .map_err(|error| format!("cannot create folder {text}: {error}"))
    }

    pub(super) async fn remove_file(&self, path: &Path) -> Result<(), String> {
        let text = path_text(path);
        self.session
            .remove_file(text.clone())
            .await
            .map_err(|error| format!("cannot delete {text}: {error}"))
    }

    pub(super) async fn remove_dir(&self, path: &Path) -> Result<(), String> {
        let text = path_text(path);
        self.session
            .remove_dir(text.clone())
            .await
            .map_err(|error| format!("cannot delete folder {text}: {error}"))
    }

    pub(super) async fn set_modified(&self, path: &Path, time: SystemTime) -> Result<(), String> {
        let text = path_text(path);
        let seconds = time
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|since| u32::try_from(since.as_secs()).ok())
            .ok_or_else(|| format!("{text} has a date SFTP cannot store"))?;
        let attributes = FileAttributes {
            atime: Some(seconds),
            mtime: Some(seconds),
            ..FileAttributes::empty()
        };
        self.session
            .set_metadata(text.clone(), attributes)
            .await
            .map_err(|error| format!("cannot set the time of {text}: {error}"))
    }
}

/// [`RemoteMeta`] from SFTP file attributes.
fn meta(metadata: &Metadata) -> RemoteMeta {
    RemoteMeta {
        is_dir: metadata.is_dir(),
        is_symlink: metadata.is_symlink(),
        size: metadata.size.unwrap_or(0),
        modified: metadata
            .mtime
            .map(|seconds| UNIX_EPOCH + Duration::from_secs(u64::from(seconds))),
    }
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
/// # Returns
///
/// The session and the user's home folder
///
/// # Errors
///
/// Returns a [`ConnectError`] describing the step that failed
pub(super) async fn connect(
    site: &Site,
    password: Option<SecretString>,
    approved_key: Option<HostKey>,
) -> Result<(Sftp, PathBuf), ConnectError> {
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
    Ok((
        Sftp {
            session: sftp,
            _ssh: ssh,
        },
        PathBuf::from(home),
    ))
}
