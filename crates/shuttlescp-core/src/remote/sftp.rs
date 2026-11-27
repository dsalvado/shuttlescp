//! SFTP over SSH: verifying the server's host key against `~/.ssh/known_hosts`, logging
//! in, and the file operations behind [`super::Remote`].
//!
//! Logging in tries, in the order OpenSSH's `ssh` does: the site's private key file
//! (OpenSSH or PuTTY `.ppk`), then the keys held by the desktop's SSH agent, then the
//! password, then the server's own questions (keyboard-interactive: a one-time code for
//! two-factor authentication), as long as the server accepts each. A server may want
//! more than one of them (a key, then a code).

use std::io::SeekFrom;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use russh::client::{self, AuthResult, KeyboardInteractiveAuthResponse, Prompt};
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::known_hosts::learn_known_hosts;
use russh::keys::{
    HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate, check_known_hosts,
};
use russh::{MethodKind, MethodSet};
use russh_sftp::client::SftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::fs::Metadata;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use secrecy::{ExposeSecret, SecretString};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use super::command::{self, Change, CommandError, RunAs, shell_quote};
use super::{
    CONNECT_TIMEOUT, Challenge, ConnectError, Credentials, HostKey, RemoteMeta, RemoteReader,
    RemoteWriter, path_text,
};
use crate::checksum::{Checksum, first_checksum_in};
use crate::path::RemotePath;
use crate::sites::Site;

/// Most agent keys offered to one server. Servers stop a login after a few failed
/// attempts (OpenSSH: 6 by default), and the password must still get its turn.
const MAX_AGENT_KEYS: usize = 4;

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
            return self.reject(HostKeyVerdict::Failed(crate::tr!(
                "connect-host-certificates"
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

/// An open SFTP session. When its connection turns out to be lost, it logs in again.
pub(super) struct Sftp {
    /// Where and how to log in again (the password stays in memory, as for FTP; shared
    /// with the session's [`Sftp::twin`]).
    site: Site,
    credentials: Arc<Credentials>,
    /// The connection in use, replaced by a new one once it is lost. Shared with a
    /// twin that runs on its connection.
    link: Arc<tokio::sync::Mutex<Arc<Link>>>,
    /// For a twin on the same connection as another session (see [`Sftp::twin`]): that
    /// session's link, which logs in again when the connection is lost.
    host: Option<Arc<tokio::sync::Mutex<Arc<Link>>>>,
}

/// One SSH connection, which SFTP sessions run on.
struct Connection {
    /// Keeps the connection open for as long as a session is in use (and opens other
    /// channels on it, see [`Sftp::server_name`]).
    ssh: client::Handle<HostKeyCheck>,
    /// The login needed the user's answers (a one-time code): logging in again would
    /// ask them again.
    interactive: bool,
}

/// An SFTP session, on its connection.
struct Link {
    session: SftpSession,
    connection: Arc<Connection>,
}

/// Why an operation failed: the server's answer, or the new login after a lost
/// connection.
#[derive(Debug)]
enum Failure {
    Sftp(SftpError),
    Login(ConnectError),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sftp(error) => error.fmt(f),
            Self::Login(error) => error.fmt(f),
        }
    }
}

/// The SFTP library found its connection closed. It says so before sending the
/// request, so the server never saw it, and it is safe to send it again on a new one.
fn is_lost(error: &SftpError) -> bool {
    matches!(error, SftpError::UnexpectedBehavior(message) if message == "session closed")
}

/// Longest wait for the server to say its name.
const NAME_TIMEOUT: Duration = Duration::from_secs(3);

/// Longest wait for the answer to one SFTP request. Up to 16 reads of up to 256 kB wait
/// in line, so on a slow server the last one also waits for the 15 before it: the
/// library's own 10 seconds failed every download slower than about 0.4 MB/s. A dead
/// connection is noticed sooner, by the keepalives.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// When the server has sent nothing for this long, ask whether it is still there.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// Unanswered keepalives in a row after which the connection counts as dead.
const KEEPALIVES_MISSED: usize = 4;

/// Longest wait for the server to compute a checksum: it reads the whole file.
const CHECKSUM_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Most bytes kept of the answer to a checksum command (a checksum takes 64).
const MAX_CHECKSUM_ANSWER: usize = 64 * 1024;

/// Most bytes kept of a permission command's output.
const MAX_COMMAND_ANSWER: usize = 64 * 1024;

/// How long a permission command may take (sudo waits a few seconds after a wrong
/// password before saying so).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

impl Sftp {
    /// The connection in use.
    async fn link(&self) -> Arc<Link> {
        Arc::clone(&*self.link.lock().await)
    }

    /// Log in again to replace `lost`, unless another task already has (then its new
    /// connection is used).
    ///
    /// # Returns
    ///
    /// The connection to use from now on
    ///
    /// # Errors
    ///
    /// Returns the [`ConnectError`] of the new login
    async fn reconnect(&self, lost: &Arc<Link>) -> Result<Arc<Link>, ConnectError> {
        // Held while logging in: the others who found the connection lost wait here.
        let mut current = self.link.lock().await;
        if Arc::ptr_eq(&current, lost) {
            tracing::info!(
                "the connection to {} was lost; logging in again",
                self.site.host
            );
            let link = match &self.host {
                Some(host) => self.session_on(host).await?,
                None => open_link(&self.site, &self.credentials, None).await?.0,
            };
            *current = Arc::new(link);
        }
        Ok(Arc::clone(&current))
    }

    /// A new session on the connection of `host` (the session this twin shares a
    /// connection with): the one it uses if still open, otherwise a new one `host` logs
    /// in to, so that both use it and the user answers the login's questions once.
    async fn session_on(&self, host: &tokio::sync::Mutex<Arc<Link>>) -> Result<Link, ConnectError> {
        let mut hosting = host.lock().await;
        if hosting.connection.ssh.is_closed() {
            let (link, _) = open_link(&self.site, &self.credentials, None).await?;
            *hosting = Arc::new(link);
        }
        let connection = Arc::clone(&hosting.connection);
        let (session, _) = start_sftp(&connection, &self.site).await?;
        Ok(Link {
            session,
            connection,
        })
    }

    /// Run `operation` on the connection in use. If that connection was lost, log in
    /// again and run it once more (see [`is_lost`]: the server never saw the first try).
    async fn run<T, F, Fut>(&self, operation: F) -> Result<T, Failure>
    where
        F: Fn(Arc<Link>) -> Fut,
        Fut: Future<Output = Result<T, SftpError>>,
    {
        let link = self.link().await;
        match operation(Arc::clone(&link)).await {
            Err(error) if is_lost(&error) => {
                let link = self.reconnect(&link).await.map_err(Failure::Login)?;
                operation(link).await.map_err(Failure::Sftp)
            }
            result => result.map_err(Failure::Sftp),
        }
    }

    /// The server's own name (`uname -n`), asked on a second channel of the SSH
    /// connection. `None` if the account may not run commands (SFTP-only accounts), the
    /// answer is not a plausible host name, or it takes longer than [`NAME_TIMEOUT`].
    pub(super) async fn server_name(&self) -> Option<String> {
        let link = self.link().await;
        let ask = async {
            let mut channel = link.connection.ssh.channel_open_session().await.ok()?;
            channel.exec(true, "uname -n").await.ok()?;
            let (mut output, mut status) = (Vec::new(), None);
            while let Some(message) = channel.wait().await {
                match message {
                    russh::ChannelMsg::Data { data } => output.extend_from_slice(&data),
                    russh::ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                    russh::ChannelMsg::Eof | russh::ChannelMsg::Close => break,
                    _ => {}
                }
                // An SFTP-only account may answer with anything: stop early.
                if output.len() > 256 {
                    return None;
                }
            }
            let _ = channel.close().await;
            status.is_none_or(|status| status == 0).then_some(())?;
            plausible_host_name(&String::from_utf8_lossy(&output))
        };
        tokio::time::timeout(NAME_TIMEOUT, ask).await.ok().flatten()
    }

    /// The SHA-256 of a file, computed by the server: `sha256sum` (Linux), else
    /// `shasum -a 256` (macOS) or `sha256 -q` (BSD), run on a second channel of the SSH
    /// connection.
    ///
    /// # Errors
    ///
    /// Returns why there is none: the account may not run commands (SFTP-only
    /// accounts), none of the tools is there, or the file cannot be read
    pub(super) async fn sha256(&self, path: &RemotePath) -> Result<Checksum, String> {
        let file = shell_quote(path.as_str());
        self.checksum_by_command(format!(
            "sha256sum -b -- {file} || shasum -a 256 -b -- {file} || sha256 -q -- {file}"
        ))
        .await
    }

    /// The SHA-256 of the bytes `start..end` of a file, computed by the server: `tail`
    /// skips to `start` (seeking, for a file), `head` stops at `end`, and the hash
    /// tools are tried as for [`Self::sha256`]. Run by `sh`, whatever the account's own
    /// shell is.
    ///
    /// # Errors
    ///
    /// As for [`Self::sha256`]; also when the file cannot be read (checked first, as a
    /// pipeline's failure would otherwise go unseen, giving the checksum of nothing)
    pub(super) async fn range_sha256(
        &self,
        path: &RemotePath,
        start: u64,
        end: u64,
    ) -> Result<Checksum, String> {
        self.checksum_by_command(range_sha256_command(path.as_str(), start, end))
            .await
    }

    /// Run `command` on a second channel of the SSH connection, and find the checksum
    /// it prints.
    async fn checksum_by_command(&self, command: String) -> Result<Checksum, String> {
        let link = self.link().await;
        let ask = async {
            let mut channel = link
                .connection
                .ssh
                .channel_open_session()
                .await
                .map_err(|error| error.to_string())?;
            channel
                .exec(true, command)
                .await
                .map_err(|_| crate::tr!("checksum-unsupported"))?;
            // Nothing to send. An SFTP-only account may start an SFTP server instead,
            // which then ends rather than wait for requests.
            let _ = channel.eof().await;
            let (mut output, mut errors, mut status) = (Vec::new(), Vec::new(), None);
            while let Some(message) = channel.wait().await {
                match message {
                    russh::ChannelMsg::Data { data } => output.extend_from_slice(&data),
                    russh::ChannelMsg::ExtendedData { data, .. } => {
                        errors.extend_from_slice(&data);
                    }
                    russh::ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                    russh::ChannelMsg::Close => break,
                    _ => {}
                }
                if output.len() + errors.len() > MAX_CHECKSUM_ANSWER {
                    break;
                }
            }
            let _ = channel.close().await;
            match (status, first_checksum_in(&String::from_utf8_lossy(&output))) {
                // Some servers send no exit status.
                (Some(0) | None, Some(checksum)) => Ok(checksum),
                _ => {
                    let errors = String::from_utf8_lossy(&errors);
                    Err(match errors.lines().next() {
                        Some(line) if !line.trim().is_empty() => line.trim().to_owned(),
                        _ => crate::tr!("checksum-unsupported"),
                    })
                }
            }
        };
        tokio::time::timeout(CHECKSUM_TIMEOUT, ask)
            .await
            .unwrap_or_else(|_| Err(crate::tr!("checksum-no-answer")))
    }

    pub(super) async fn stat(&self, path: &RemotePath) -> Result<RemoteMeta, String> {
        let text = path_text(path);
        self.run(|link| {
            let text = text.clone();
            async move { link.session.metadata(text).await }
        })
        .await
        .map(|metadata| meta(&metadata))
        .map_err(|error| crate::i18n::cannot("read", &text, &error))
    }

    pub(super) async fn lstat(&self, path: &RemotePath) -> Result<RemoteMeta, String> {
        let text = path_text(path);
        self.run(|link| {
            let text = text.clone();
            async move { link.session.symlink_metadata(text).await }
        })
        .await
        .map(|metadata| meta(&metadata))
        .map_err(|error| crate::i18n::cannot("read", &text, &error))
    }

    pub(super) async fn read_dir(
        &self,
        dir: &RemotePath,
    ) -> Result<Vec<(String, RemoteMeta)>, String> {
        let text = path_text(dir);
        let listing = self
            .run(|link| {
                let text = text.clone();
                async move { link.session.read_dir(text).await }
            })
            .await
            .map_err(|error| crate::i18n::cannot("open", &text, &error))?;
        Ok(listing
            .filter(|entry| entry.file_name() != "." && entry.file_name() != "..")
            .map(|entry| (entry.file_name(), meta(&entry.metadata())))
            .collect())
    }

    pub(super) async fn open_at(
        &self,
        path: &RemotePath,
        offset: u64,
    ) -> Result<RemoteReader, String> {
        let text = path_text(path);
        let mut file = self
            .run(|link| {
                let text = text.clone();
                async move { link.session.open(text).await }
            })
            .await
            .map_err(|error| crate::i18n::cannot("open", &text, &error))?;
        if offset > 0 {
            file.seek(SeekFrom::Start(offset))
                .await
                .map_err(|error| crate::i18n::cannot("read", &text, &error))?;
        }
        Ok(Box::pin(file))
    }

    pub(super) async fn open_write_at(
        &self,
        path: &RemotePath,
        offset: u64,
    ) -> Result<RemoteWriter, String> {
        self.open_for_writing(path, offset, offset == 0).await
    }

    /// Open a file to write one part of it at `position`, keeping all it holds, before
    /// and after (see `Remote::open_write_range`).
    pub(super) async fn open_write_range(
        &self,
        path: &RemotePath,
        position: u64,
    ) -> Result<RemoteWriter, String> {
        self.open_for_writing(path, position, false).await
    }

    /// Make a file exactly `size` bytes long (see `Remote::set_size`). Sets the size
    /// with "setstat"; a server that refuses that gets one byte written at the end
    /// instead, which can only make the file longer.
    pub(super) async fn set_size(&self, path: &RemotePath, size: u64) -> Result<(), String> {
        let text = path_text(path);
        let attributes = FileAttributes {
            size: Some(size),
            ..FileAttributes::empty()
        };
        let set = self
            .run(|link| {
                let (text, attributes) = (text.clone(), attributes.clone());
                async move { link.session.set_metadata(text, attributes).await }
            })
            .await;
        let Err(error) = set else {
            return Ok(());
        };
        if size == 0 {
            return Err(crate::i18n::cannot("write", &text, &error));
        }
        tracing::debug!("{text}: setting the size refused ({error}); writing its last byte");
        let mut file = self.open_for_writing(path, size - 1, false).await?;
        let written = async {
            file.write_all(&[0]).await?;
            file.shutdown().await
        };
        written
            .await
            .map_err(|error| crate::i18n::cannot("write", &text, &error))
    }

    /// Open a file for writing at `position`, creating it if missing; `truncate`
    /// empties it first.
    async fn open_for_writing(
        &self,
        path: &RemotePath,
        offset: u64,
        truncate: bool,
    ) -> Result<RemoteWriter, String> {
        let text = path_text(path);
        let mut flags = OpenFlags::CREATE | OpenFlags::WRITE;
        if truncate {
            flags |= OpenFlags::TRUNCATE;
        }
        let mut file = self
            .run(|link| {
                let text = text.clone();
                async move { link.session.open_with_flags(text, flags).await }
            })
            .await
            .map_err(|error| crate::i18n::cannot("write", &text, &error))?;
        if offset > 0 {
            file.seek(SeekFrom::Start(offset))
                .await
                .map_err(|error| crate::i18n::cannot("write", &text, &error))?;
        }
        Ok(Box::pin(file))
    }

    pub(super) async fn rename(&self, from: &RemotePath, to: &RemotePath) -> Result<(), String> {
        let (from_text, to_text) = (path_text(from), path_text(to));
        self.run(|link| {
            let (from, to) = (from_text.clone(), to_text.clone());
            async move { link.session.rename(from, to).await }
        })
        .await
        .map_err(|error| {
            crate::tr!(
                "error-cannot-rename",
                from = from_text.to_string(),
                to = to_text.to_string(),
                reason = error.to_string()
            )
        })
    }

    pub(super) async fn create_dir(&self, path: &RemotePath) -> Result<(), String> {
        let text = path_text(path);
        self.run(|link| {
            let text = text.clone();
            async move { link.session.create_dir(text).await }
        })
        .await
        .map_err(|error| crate::i18n::cannot("create-folder", &text, &error))
    }

    pub(super) async fn remove_file(&self, path: &RemotePath) -> Result<(), String> {
        let text = path_text(path);
        self.run(|link| {
            let text = text.clone();
            async move { link.session.remove_file(text).await }
        })
        .await
        .map_err(|error| crate::i18n::cannot("delete", &text, &error))
    }

    pub(super) async fn remove_dir(&self, path: &RemotePath) -> Result<(), String> {
        let text = path_text(path);
        self.run(|link| {
            let text = text.clone();
            async move { link.session.remove_dir(text).await }
        })
        .await
        .map_err(|error| crate::i18n::cannot("delete-folder", &text, &error))
    }

    pub(super) async fn set_modified(
        &self,
        path: &RemotePath,
        time: SystemTime,
    ) -> Result<(), String> {
        let text = path_text(path);
        let seconds = time
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|since| u32::try_from(since.as_secs()).ok())
            .ok_or_else(|| crate::tr!("error-sftp-date", path = text.clone()))?;
        let attributes = FileAttributes {
            atime: Some(seconds),
            mtime: Some(seconds),
            ..FileAttributes::empty()
        };
        self.run(|link| {
            let (text, attributes) = (text.clone(), attributes.clone());
            async move { link.session.set_metadata(text, attributes).await }
        })
        .await
        .map_err(|error| crate::i18n::cannot("set-time", &text, &error))
    }

    /// Change permission bits (SFTP "setstat" with only the permissions set). A link
    /// is followed: the server changes the file it points to.
    pub(super) async fn set_mode(&self, path: &RemotePath, mode: u32) -> Result<(), String> {
        let text = path_text(path);
        let attributes = FileAttributes {
            permissions: Some(mode & 0o7777),
            ..FileAttributes::empty()
        };
        self.run(|link| {
            let (text, attributes) = (text.clone(), attributes.clone());
            async move { link.session.set_metadata(text, attributes).await }
        })
        .await
        .map_err(|error| crate::i18n::cannot("set-permissions", &text, &error))
    }

    /// Make `change` to `path` with a shell command on a second channel of the SSH
    /// connection, as the user or through sudo (see [`command`]).
    ///
    /// # Errors
    ///
    /// Returns why it was not made: no shell, sudo's refusal, or the command's own error
    pub(super) async fn run_change(
        &self,
        path: &RemotePath,
        change: &Change,
        run_as: &RunAs,
    ) -> Result<(), CommandError> {
        let Some(line) = command::command_line(change, path.as_str(), run_as) else {
            return Ok(());
        };
        self.run_line(line, run_as).await
    }

    /// Check that sudo works as `run_as` says, changing nothing.
    pub(super) async fn check_sudo(&self, run_as: &RunAs) -> Result<(), CommandError> {
        self.run_line(command::sudo_check_line(run_as), run_as)
            .await
    }

    /// Run a command line made by [`command`], giving it the sudo password if there is
    /// one, and read what happened.
    async fn run_line(&self, line: String, run_as: &RunAs) -> Result<(), CommandError> {
        let password = match run_as {
            RunAs::Sudo {
                password: Some(password),
            } => Some(password),
            RunAs::User | RunAs::Sudo { password: None } => None,
        };
        let (output, errors, status) = self.run_command(line, password).await?;
        command::outcome(&output, &errors, status, run_as.is_sudo())
    }

    /// Run `line` in the account's shell and collect what it prints. `input`, if given,
    /// is sent to the command as one line and then the input is closed (sudo -S reads
    /// its password from there).
    ///
    /// # Returns
    ///
    /// Its output, its error output, and its exit status if the server sent one
    async fn run_command(
        &self,
        line: String,
        input: Option<&SecretString>,
    ) -> Result<(String, String, Option<u32>), CommandError> {
        let link = self.link().await;
        let run = async {
            let mut channel = link
                .connection
                .ssh
                .channel_open_session()
                .await
                .map_err(|_| CommandError::NoShell)?;
            channel
                .exec(true, line)
                .await
                .map_err(|_| CommandError::NoShell)?;
            if let Some(secret) = input {
                // Wiped from memory when dropped, rather than left lying around.
                let bytes = zeroize::Zeroizing::new(format!("{}\n", secret.expose_secret()));
                // If sending fails, the command just doesn't get it and says so.
                let _ = channel.data_bytes(bytes.as_bytes().to_vec()).await;
            }
            let _ = channel.eof().await;
            let (mut output, mut errors, mut status) = (Vec::new(), Vec::new(), None);
            while let Some(message) = channel.wait().await {
                match message {
                    russh::ChannelMsg::Data { data } => output.extend_from_slice(&data),
                    russh::ChannelMsg::ExtendedData { data, .. } => {
                        errors.extend_from_slice(&data);
                    }
                    russh::ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                    russh::ChannelMsg::Close => break,
                    _ => {}
                }
                if output.len() + errors.len() > MAX_COMMAND_ANSWER {
                    break;
                }
            }
            let _ = channel.close().await;
            Ok((
                String::from_utf8_lossy(&output).into_owned(),
                String::from_utf8_lossy(&errors).into_owned(),
                status,
            ))
        };
        tokio::time::timeout(COMMAND_TIMEOUT, run)
            .await
            .unwrap_or(Err(CommandError::NoAnswer))
    }

    /// Change the owner and/or group, by number. SFTP always sends the two together, so
    /// one left out is first read from the file and sent back unchanged (otherwise the
    /// library would send 0, making root its owner or group).
    pub(super) async fn set_owner(
        &self,
        path: &RemotePath,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> Result<(), String> {
        let text = path_text(path);
        let cannot = |error: &dyn std::fmt::Display| crate::i18n::cannot("set-owner", &text, error);
        let (uid, gid) = match (uid, gid) {
            (None, None) => return Ok(()),
            (Some(uid), Some(gid)) => (uid, gid),
            (uid, gid) => {
                let current = self
                    .run(|link| {
                        let text = text.clone();
                        async move { link.session.metadata(text).await }
                    })
                    .await
                    .map_err(|error| cannot(&error))?;
                match (uid.or(current.uid), gid.or(current.gid)) {
                    (Some(uid), Some(gid)) => (uid, gid),
                    // The server didn't say: better to change nothing than to guess.
                    _ => return Err(cannot(&crate::tr!("error-owner-unknown"))),
                }
            }
        };
        let attributes = FileAttributes {
            uid: Some(uid),
            gid: Some(gid),
            ..FileAttributes::empty()
        };
        self.run(|link| {
            let (text, attributes) = (text.clone(), attributes.clone());
            async move { link.session.set_metadata(text, attributes).await }
        })
        .await
        .map_err(|error| cannot(&error))
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
        mode: metadata.permissions,
        // SFTP version 3 sends only the numbers; a name, if the library ever has one,
        // is better.
        owner: metadata
            .user
            .clone()
            .or_else(|| metadata.uid.map(|uid| uid.to_string())),
        group: metadata
            .group
            .clone()
            .or_else(|| metadata.gid.map(|gid| gid.to_string())),
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
/// * `credentials` - Password and key passphrase to log in with
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
    credentials: Credentials,
    approved_key: Option<HostKey>,
) -> Result<(Sftp, RemotePath), ConnectError> {
    let (link, home) = open_link(site, &credentials, approved_key).await?;
    let sftp = Sftp {
        site: site.clone(),
        credentials: Arc::new(credentials),
        link: Arc::new(tokio::sync::Mutex::new(Arc::new(link))),
        host: None,
    };
    Ok((sftp, home))
}

impl Sftp {
    /// A second session to the same server with the same login. Usually on a
    /// connection of its own (its host key is known by now); when logging in needed the
    /// user's answers (a one-time code), on a second channel of this session's
    /// connection instead, so that they are not asked again.
    ///
    /// # Errors
    ///
    /// Returns the [`ConnectError`] of the login, or of starting SFTP
    pub(super) async fn twin(&self) -> Result<Sftp, ConnectError> {
        let current = self.link().await;
        let (link, host) = if current.connection.interactive {
            let connection = Arc::clone(&current.connection);
            let (session, _) = start_sftp(&connection, &self.site).await?;
            let link = Link {
                session,
                connection,
            };
            (link, Some(Arc::clone(&self.link)))
        } else {
            (
                open_link(&self.site, &self.credentials, None).await?.0,
                None,
            )
        };
        Ok(Sftp {
            site: self.site.clone(),
            credentials: Arc::clone(&self.credentials),
            link: Arc::new(tokio::sync::Mutex::new(Arc::new(link))),
            host,
        })
    }
}

/// One connection for [`connect`], or to replace a lost one: the same steps and
/// arguments, and it returns the connection and the user's home folder.
async fn open_link(
    site: &Site,
    credentials: &Credentials,
    approved_key: Option<HostKey>,
) -> Result<(Link, RemotePath), ConnectError> {
    let verdict = Arc::new(Mutex::new(None));
    let handler = HostKeyCheck {
        host: site.host.clone(),
        port: site.port,
        approved: approved_key,
        verdict: Arc::clone(&verdict),
    };
    tracing::info!("connecting to {}:{}", site.host, site.port);
    let config = client::Config {
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        keepalive_max: KEEPALIVES_MISSED,
        ..client::Config::default()
    };
    let connecting = client::connect(Arc::new(config), (site.host.as_str(), site.port), handler);
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

    let interactive = authenticate(&mut ssh, site, credentials).await?;
    let connection = Arc::new(Connection { ssh, interactive });
    let (session, home) = start_sftp(&connection, site).await?;
    Ok((
        Link {
            session,
            connection,
        },
        home,
    ))
}

/// Start an SFTP session on a new channel of `connection`: the session and the user's
/// home folder.
async fn start_sftp(
    connection: &Connection,
    site: &Site,
) -> Result<(SftpSession, RemotePath), ConnectError> {
    let network = |error: russh::Error| ConnectError::Network {
        host: site.host.clone(),
        reason: error.to_string(),
    };
    let channel = connection
        .ssh
        .channel_open_session()
        .await
        .map_err(network)?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(network)?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|error| ConnectError::Sftp(error.to_string()))?;
    sftp.set_timeout(REQUEST_TIMEOUT.as_secs());
    let home = sftp
        .canonicalize(".")
        .await
        .map_err(|error| ConnectError::Sftp(error.to_string()))?;
    Ok((sftp, RemotePath::new(home)))
}

/// What came of one login step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The login is complete.
    Done,
    /// Accepted, but the server asks for more (two-factor: a key, then a code).
    More,
    /// Not accepted.
    Refused,
}

/// The login methods the server still accepts, as it last said (all, until it has).
struct Allowed(Option<MethodSet>);

impl Allowed {
    fn accepts(&self, method: MethodKind) -> bool {
        self.0
            .as_ref()
            .is_none_or(|methods| methods.contains(&method))
    }

    /// Take in the server's answer to a login step (`what`, for the log).
    fn after(&mut self, result: AuthResult, what: &str) -> Step {
        match result {
            AuthResult::Success => {
                tracing::info!("logged in with {what}");
                Step::Done
            }
            AuthResult::Failure {
                remaining_methods,
                partial_success,
            } => self.failure(remaining_methods, partial_success, what),
        }
    }

    /// Take in a "not yet": what the server accepts now, and whether `what` counted.
    fn failure(&mut self, remaining: MethodSet, partial_success: bool, what: &str) -> Step {
        let names: Vec<&str> = remaining.iter().map(<&str>::from).collect();
        let step = if partial_success {
            tracing::info!("the server accepted {what} and asks for more ({names:?})");
            Step::More
        } else {
            tracing::info!("the server did not accept {what} (it accepts {names:?})");
            Step::Refused
        };
        self.0 = Some(remaining);
        step
    }
}

/// Log in, as OpenSSH's `ssh` does: with the site's key file, the SSH agent's keys, the
/// password, then the server's questions ("keyboard-interactive"), each only while the
/// server still accepts that method. A server may want more than one: after a key it
/// may ask for a one-time code ("partial success"); the login is done when it says so.
///
/// # Returns
///
/// Whether the user answered questions (a one-time code): logging in again would ask
/// again
///
/// # Errors
///
/// Returns [`ConnectError::KeyPassphraseRequired`] or [`ConnectError::KeyUnreadable`]
/// when the key file cannot be used, [`ConnectError::Cancelled`] when the user cancels
/// the questions, [`ConnectError::AuthenticationFailed`] when the password or the
/// answers are refused, [`ConnectError::KeysRejected`] when only keys were tried and
/// all refused, and [`ConnectError::PasswordRequired`] when there was nothing to try
async fn authenticate<H: client::Handler>(
    ssh: &mut client::Handle<H>,
    site: &Site,
    credentials: &Credentials,
) -> Result<bool, ConnectError> {
    let user = site.user.as_str();
    let failed = |reason: String| ConnectError::Network {
        host: site.host.clone(),
        reason,
    };
    // RSA keys sign with the best hash the server supports (rsa-sha2-512 or -256).
    let rsa_hash = ssh
        .best_supported_rsa_hash()
        .await
        .map_err(|error| failed(error.to_string()))?
        .flatten();
    let mut allowed = Allowed(None);
    let mut keys_tried = 0;
    // A step was accepted, but the server wants more.
    let mut accepted_some = false;

    // 1. The site's key file.
    if let Some(path) = &site.ssh.key_file {
        let key = load_key(path, credentials.key_passphrase.as_ref())?;
        keys_tried += 1;
        let auth = ssh
            .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), rsa_hash))
            .await
            .map_err(|error| failed(error.to_string()))?;
        match allowed.after(auth, &format!("the key file {}", path.display())) {
            Step::Done => return Ok(false),
            Step::More => accepted_some = true,
            Step::Refused => {}
        }
    }

    // 2. The SSH agent's keys (GNOME Keyring, ssh-agent, ...). No agent is fine.
    if site.ssh.use_agent && allowed.accepts(MethodKind::PublicKey) {
        match connect_agent().await {
            Ok(mut agent) => {
                let identities = agent.request_identities().await.unwrap_or_else(|error| {
                    tracing::warn!("cannot list the SSH agent's keys: {error}");
                    Vec::new()
                });
                let public_keys = identities
                    .into_iter()
                    .filter_map(|identity| match identity {
                        AgentIdentity::PublicKey { key, .. } => Some(key),
                        AgentIdentity::Certificate { .. } => None,
                    });
                for key in public_keys.take(MAX_AGENT_KEYS) {
                    // Once one is accepted, the server may want something else.
                    if !allowed.accepts(MethodKind::PublicKey) {
                        break;
                    }
                    keys_tried += 1;
                    let fingerprint = key.fingerprint(HashAlg::Sha256);
                    let auth = ssh
                        .authenticate_publickey_with(user, key, rsa_hash, &mut agent)
                        .await
                        .map_err(|error| failed(error.to_string()))?;
                    match allowed.after(auth, &format!("the SSH agent's key {fingerprint}")) {
                        Step::Done => return Ok(false),
                        Step::More => accepted_some = true,
                        Step::Refused => {}
                    }
                }
            }
            Err(error) => tracing::debug!("no SSH agent to use: {error}"),
        }
    }

    // 3. The password, where the server takes it as such.
    let mut password_refused = false;
    let mut secrets_tried = false;
    if let Some(password) = &credentials.password
        && allowed.accepts(MethodKind::Password)
    {
        secrets_tried = true;
        let auth = ssh
            .authenticate_password(user, password.expose_secret())
            .await
            .map_err(|error| failed(error.to_string()))?;
        match allowed.after(auth, "the password") {
            Step::Done => return Ok(false),
            Step::More => accepted_some = true,
            Step::Refused => password_refused = true,
        }
    }

    // 4. The server's questions: a one-time code, or the password asked its own way.
    // The password answers a question asking for it, unless it was just refused.
    let password = credentials.password.as_ref().filter(|_| !password_refused);
    if allowed.accepts(MethodKind::KeyboardInteractive)
        && (credentials.prompter.is_some() || password.is_some())
    {
        secrets_tried = true;
        let (step, asked) =
            keyboard_interactive(ssh, site, credentials, password, &mut allowed).await?;
        if step == Step::Done {
            return Ok(asked);
        }
    }

    Err(if secrets_tried || accepted_some {
        ConnectError::AuthenticationFailed
    } else if keys_tried > 0 {
        ConnectError::KeysRejected
    } else {
        ConnectError::PasswordRequired
    })
}

/// Log in by answering the server's questions ("keyboard-interactive"), in as many
/// rounds as it asks.
///
/// # Arguments
///
/// * `password` - Answers the first question asking for a password (asked again, it
///   was wrong: then the user answers)
///
/// # Returns
///
/// How the step ended, and whether the user answered questions
async fn keyboard_interactive<H: client::Handler>(
    ssh: &mut client::Handle<H>,
    site: &Site,
    credentials: &Credentials,
    mut password: Option<&SecretString>,
    allowed: &mut Allowed,
) -> Result<(Step, bool), ConnectError> {
    let failed = |error: russh::Error| ConnectError::Network {
        host: site.host.clone(),
        reason: error.to_string(),
    };
    let what = "the answers to the server's questions";
    let mut asked = false;
    let mut reply = ssh
        .authenticate_keyboard_interactive_start(site.user.as_str(), None)
        .await
        .map_err(failed)?;
    loop {
        reply = match reply {
            KeyboardInteractiveAuthResponse::Success => {
                tracing::info!("logged in with {what}");
                return Ok((Step::Done, asked));
            }
            KeyboardInteractiveAuthResponse::Failure {
                remaining_methods,
                partial_success,
            } => {
                let step = allowed.failure(remaining_methods, partial_success, what);
                return Ok((step, asked));
            }
            KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } => {
                let questions = Challenge {
                    server: format!("{}@{}", site.user, site.host),
                    name,
                    instructions,
                    prompts: prompts
                        .into_iter()
                        .map(|Prompt { prompt, echo }| (prompt, echo))
                        .collect(),
                };
                let answers = answers(questions, &mut password, credentials, &mut asked).await?;
                ssh.authenticate_keyboard_interactive_respond(answers)
                    .await
                    .map_err(failed)?
            }
        };
    }
}

/// The answers to one round of the server's questions: `password` for the first one
/// asking for a password (it is used up), the user's for the others (none asked if
/// none are left; servers sometimes send a round without questions).
///
/// # Errors
///
/// Returns [`ConnectError::Cancelled`] if the user cancels, and
/// [`ConnectError::AuthenticationFailed`] if there are questions nobody can answer
async fn answers(
    questions: Challenge,
    password: &mut Option<&SecretString>,
    credentials: &Credentials,
    asked: &mut bool,
) -> Result<Vec<String>, ConnectError> {
    let mut answers: Vec<Option<String>> = questions
        .prompts
        .iter()
        .map(|(prompt, echo)| {
            super::asks_for_password(prompt, *echo)
                .then(|| password.take())
                .flatten()
                .map(|password| password.expose_secret().to_owned())
        })
        .collect();
    let open: Vec<usize> = (0..answers.len())
        .filter(|&index| answers[index].is_none())
        .collect();
    if !open.is_empty() {
        let Some(prompter) = &credentials.prompter else {
            tracing::info!("the server asks questions, and there is nobody to answer them");
            return Err(ConnectError::AuthenticationFailed);
        };
        let for_user = Challenge {
            prompts: open
                .iter()
                .map(|&index| questions.prompts[index].clone())
                .collect(),
            ..questions
        };
        tracing::info!(
            "the server asks {} question(s); asking the user",
            for_user.prompts.len()
        );
        let given = prompter(for_user).await.ok_or(ConnectError::Cancelled)?;
        if given.len() != open.len() {
            return Err(ConnectError::AuthenticationFailed);
        }
        *asked = true;
        for (index, answer) in open.into_iter().zip(given) {
            answers[index] = Some(answer.expose_secret().to_owned());
        }
    }
    Ok(answers.into_iter().map(Option::unwrap_or_default).collect())
}

/// A connection to the SSH agent, whatever kind of stream reaches it.
type Agent = AgentClient<Box<dyn AgentStream + Send + Unpin>>;

/// Connect to the SSH agent named by `SSH_AUTH_SOCK` (GNOME Keyring, ssh-agent, ...).
///
/// # Errors
///
/// Returns an error if no agent is set or it cannot be reached
#[cfg(unix)]
async fn connect_agent() -> Result<Agent, russh::keys::Error> {
    Ok(AgentClient::connect_env().await?.dynamic())
}

/// Connect to an SSH agent on Windows: the pipe named by `SSH_AUTH_SOCK` if set, else
/// Windows' own OpenSSH agent service, else PuTTY's Pageant.
///
/// # Errors
///
/// Returns Pageant's error if no agent can be reached
#[cfg(windows)]
async fn connect_agent() -> Result<Agent, russh::keys::Error> {
    const OPENSSH_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
    let pipe = std::env::var_os("SSH_AUTH_SOCK").unwrap_or_else(|| OPENSSH_PIPE.into());
    match AgentClient::connect_named_pipe(&pipe).await {
        Ok(agent) => return Ok(agent.dynamic()),
        Err(error) => tracing::debug!("no SSH agent at {}: {error}", pipe.display()),
    }
    Ok(AgentClient::connect_pageant().await?.dynamic())
}

/// Read a private key file (OpenSSH, PKCS#8 or PuTTY `.ppk`), opening it with
/// `passphrase` if it is protected.
///
/// The key is first tried without a passphrase: a protected key then reports that it
/// needs one, which the caller asks the user for.
///
/// # Errors
///
/// Returns [`ConnectError::KeyPassphraseRequired`] if the key is protected and no
/// passphrase (or a wrong one) was given, and [`ConnectError::KeyUnreadable`] if the
/// file cannot be read or is not a supported key
pub(super) fn load_key(
    path: &Path,
    passphrase: Option<&SecretString>,
) -> Result<PrivateKey, ConnectError> {
    let unreadable = |reason: String| ConnectError::KeyUnreadable {
        path: path.to_path_buf(),
        reason,
    };
    let text = std::fs::read_to_string(path).map_err(|error| unreadable(error.to_string()))?;
    let error = match russh::keys::decode_secret_key(&text, None) {
        Ok(key) => return Ok(key),
        Err(error) => error,
    };
    if !is_protected(&text, &error) {
        return Err(unreadable(error.to_string()));
    }
    let Some(passphrase) = passphrase else {
        return Err(ConnectError::KeyPassphraseRequired {
            path: path.to_path_buf(),
            wrong: false,
        });
    };
    russh::keys::decode_secret_key(&text, Some(passphrase.expose_secret())).map_err(|_| {
        ConnectError::KeyPassphraseRequired {
            path: path.to_path_buf(),
            wrong: true,
        }
    })
}

/// Whether a key that failed to load without a passphrase is passphrase-protected:
/// the library says so, or the file's own header does (PEM "ENCRYPTED", or a PuTTY key
/// whose "Encryption" is not "none").
fn is_protected(text: &str, error: &russh::keys::Error) -> bool {
    matches!(error, russh::keys::Error::KeyIsEncrypted)
        || text.lines().take(3).any(|line| line.contains("ENCRYPTED"))
        || text
            .lines()
            .find_map(|line| line.strip_prefix("Encryption:"))
            .is_some_and(|value| value.trim() != "none")
}

/// `text` (a command's output) as a host name, if it is one: a single line of letters,
/// digits, dots, dashes and underscores.
fn plausible_host_name(text: &str) -> Option<String> {
    let name = text.trim();
    let valid = !name.is_empty()
        && name.len() <= 253
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    valid.then(|| name.to_owned())
}

/// The command printing the SHA-256 of the bytes `start..end` of `file` (see
/// [`Sftp::range_sha256`]). `tail -c +N` counts from 1, so it starts at `start + 1`.
fn range_sha256_command(file: &str, start: u64, end: u64) -> String {
    let (from, length) = (start + 1, end.saturating_sub(start));
    let script = format!(
        "f={}; [ -f \"$f\" ] && [ -r \"$f\" ] || {{ echo \"cannot read $f\" >&2; exit 1; }}; \
         r() {{ tail -c +{from} -- \"$f\" | head -c {length}; }}; \
         r | sha256sum -b || r | shasum -a 256 -b || r | sha256 -q",
        shell_quote(file)
    );
    format!("sh -c {}", shell_quote(&script))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use secrecy::SecretString;

    use crate::remote::Prompter;

    use super::*;

    /// Run `command` as an SSH server would, through a shell here.
    fn run_here(command: &str) -> std::process::Output {
        std::process::Command::new("sh")
            .args(["-c", command])
            .output()
            .expect("run sh")
    }

    #[test]
    fn the_range_command_hashes_just_that_range() {
        // Arrange: a name a shell would misread, and bytes that differ along the file.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("it's a $file");
        let bytes: Vec<u8> = (0..100_000_u32).map(|at| (at % 251) as u8).collect();
        std::fs::write(&path, &bytes).expect("write");
        let file = path.to_str().expect("utf-8");

        for (start, end) in [(0, 100_000), (1, 2), (40_000, 70_001), (99_999, 100_000)] {
            // Act
            let output = run_here(&range_sha256_command(file, start, end));

            // Assert
            let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
            sha2::Digest::update(&mut hasher, &bytes[start as usize..end as usize]);
            assert_eq!(
                first_checksum_in(&String::from_utf8_lossy(&output.stdout)),
                Some(Checksum::from_hasher(hasher)),
                "{start}..{end}"
            );
        }
    }

    #[test]
    fn the_range_command_fails_for_a_missing_file_rather_than_hash_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("missing");

        let output = run_here(&range_sha256_command(
            missing.to_str().expect("utf-8"),
            0,
            10,
        ));

        assert!(!output.status.success());
        assert_eq!(
            first_checksum_in(&String::from_utf8_lossy(&output.stdout)),
            None
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read"));
    }

    #[test]
    fn only_plausible_host_names_are_taken() {
        assert_eq!(
            plausible_host_name("oracle-vm\n").as_deref(),
            Some("oracle-vm")
        );
        assert_eq!(
            plausible_host_name("db01.example.com").as_deref(),
            Some("db01.example.com")
        );
        assert_eq!(plausible_host_name(""), None);
        assert_eq!(
            plausible_host_name("This service allows sftp connections only.\n"),
            None
        );
        assert_eq!(plausible_host_name("a\nb"), None);
    }

    /// A quoted name reaches the command as it is, whatever it holds, and nothing in it
    /// runs: checked with a real shell.
    #[cfg(unix)]
    #[test]
    fn quoted_names_stay_names() {
        // Arrange
        let dir = tempfile::tempdir().expect("temp dir");
        let pwned = dir.path().join("pwned");
        let names = [
            "plain.txt".to_owned(),
            "two words.txt".to_owned(),
            "it's.txt".to_owned(),
            "'''".to_owned(),
            "$HOME `id` $(id).txt".to_owned(),
            format!("a'; touch '{}'; echo '", pwned.display()),
            "-n".to_owned(),
            "line\nbreak.txt".to_owned(),
            "back\\slash \\' \"quotes\"".to_owned(),
        ];
        for name in names {
            // Act
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("printf %s {}", shell_quote(&name)))
                .output()
                .expect("run sh");

            // Assert
            assert_eq!(String::from_utf8_lossy(&output.stdout), name);
            assert!(!pwned.exists(), "{name:?} ran a command");
        }
    }
    use crate::sites::{FtpOptions, Protocol, SshAuth};

    /// A key in `test-data/keys` (throwaway keys, see the README there).
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/keys")
            .join(name)
    }

    fn passphrase(text: &str) -> SecretString {
        SecretString::from(text)
    }

    #[test]
    fn plain_keys_load_without_a_passphrase() {
        for name in ["ed25519", "rsa", "ed25519.ppk"] {
            assert!(load_key(&fixture(name), None).is_ok(), "{name} should load");
        }
    }

    #[test]
    fn a_putty_key_is_the_same_key_as_its_openssh_original() {
        let openssh = load_key(&fixture("ed25519"), None).expect("openssh");
        let putty = load_key(&fixture("ed25519.ppk"), None).expect("ppk");

        assert_eq!(
            openssh.public_key().key_data(),
            putty.public_key().key_data()
        );
    }

    #[test]
    fn a_protected_key_asks_for_its_passphrase_and_opens_with_it() {
        let path = fixture("ed25519-protected");

        assert!(matches!(
            load_key(&path, None),
            Err(ConnectError::KeyPassphraseRequired { wrong: false, .. })
        ));
        assert!(matches!(
            load_key(&path, Some(&passphrase("not it"))),
            Err(ConnectError::KeyPassphraseRequired { wrong: true, .. })
        ));
        assert!(load_key(&path, Some(&passphrase("correct horse battery staple"))).is_ok());
    }

    #[test]
    fn a_missing_file_or_a_non_key_is_reported_as_unreadable() {
        for name in ["not-a-key.txt", "no-such-file"] {
            assert!(
                matches!(
                    load_key(&fixture(name), None),
                    Err(ConnectError::KeyUnreadable { .. })
                ),
                "{name}"
            );
        }
    }

    /// Logs in to a real SSH server in the ways a user can. Needs a server that
    /// accepts both the given key file and a key in the running SSH agent, already in
    /// `~/.ssh/known_hosts`:
    ///
    /// ```sh
    /// SHUTTLESCP_LIVE_SSH=me@host SHUTTLESCP_LIVE_KEY=~/.ssh/id_ed25519 \
    ///     cargo test -p shuttlescp-core live_ -- --ignored
    /// ```
    #[tokio::test]
    #[ignore = "needs a real SSH server; see the comment"]
    async fn live_login_with_a_key_file_the_agent_or_nothing() {
        let target = std::env::var("SHUTTLESCP_LIVE_SSH").expect("SHUTTLESCP_LIVE_SSH=user@host");
        let key = std::env::var("SHUTTLESCP_LIVE_KEY").expect("SHUTTLESCP_LIVE_KEY=key file");
        let (user, host) = target.split_once('@').expect("user@host");
        let site = |key_file: Option<PathBuf>, use_agent: bool| Site {
            id: String::from("live-test"),
            protocol: Protocol::Sftp,
            host: host.to_owned(),
            port: 22,
            user: user.to_owned(),
            password_saved: false,
            ftp: FtpOptions::default(),
            ssh: SshAuth {
                key_file,
                use_agent,
                passphrase_saved: false,
            },
            files_at_once: crate::sites::DEFAULT_FILES_AT_ONCE,
            parts_per_file: crate::sites::DEFAULT_PARTS_PER_FILE,
            alias: String::new(),
            description: String::new(),
            folder: String::new(),
            verify: crate::sites::SiteVerify::default(),
        };
        let login = |site: Site| async move {
            connect(&site, Credentials::default(), None)
                .await
                .map(|(_, home)| home)
        };

        // The key file alone.
        let home = login(site(Some(PathBuf::from(&key)), false)).await;
        assert!(home.is_ok(), "key file: {:?}", home.err());
        // The agent alone.
        let home = login(site(None, true)).await;
        assert!(home.is_ok(), "agent: {:?}", home.err());
        // A key the server does not know, and no password.
        let refused = login(site(Some(fixture("ed25519")), false)).await;
        assert!(
            matches!(refused, Err(ConnectError::KeysRejected)),
            "{:?}",
            refused.err()
        );
        // Nothing to log in with.
        let nothing = login(site(None, false)).await;
        assert!(
            matches!(nothing, Err(ConnectError::PasswordRequired)),
            "{:?}",
            nothing.err()
        );
    }

    /// The password and code the test server wants.
    const PASSWORD: &str = "pass word";
    const CODE: &str = "123456";

    /// A test SSH server wanting two factors, as OpenSSH set up for two-factor
    /// authentication ("AuthenticationMethods publickey,keyboard-interactive", and PAM
    /// asking the password and a code): the test key, then its questions. Without
    /// `code`, it takes no key and asks only the password, as some servers do.
    struct TwoFactor {
        key: HostKey,
        code: Option<&'static str>,
        key_accepted: bool,
    }

    impl russh::server::Handler for TwoFactor {
        type Error = russh::Error;

        async fn auth_publickey(
            &mut self,
            _user: &str,
            key: russh::server::VerifiedPublicKey<'_>,
        ) -> Result<russh::server::Auth, Self::Error> {
            if self.code.is_none() || key.public_key.key_data() != self.key.key_data() {
                return Ok(russh::server::Auth::reject());
            }
            self.key_accepted = true;
            Ok(russh::server::Auth::Reject {
                proceed_with_methods: Some(MethodSet::from(&[MethodKind::KeyboardInteractive][..])),
                partial_success: true,
            })
        }

        async fn auth_keyboard_interactive<'a>(
            &'a mut self,
            _user: &str,
            _submethods: &str,
            response: Option<russh::server::Response<'a>>,
        ) -> Result<russh::server::Auth, Self::Error> {
            use std::borrow::Cow;
            if self.code.is_some() && !self.key_accepted {
                return Ok(russh::server::Auth::reject());
            }
            let mut expected = vec![PASSWORD.to_owned()];
            expected.extend(self.code.map(str::to_owned));
            let Some(response) = response else {
                let mut prompts = vec![(Cow::from("Password: "), false)];
                if self.code.is_some() {
                    prompts.push((Cow::from("Verification code: "), false));
                }
                return Ok(russh::server::Auth::Partial {
                    name: Cow::from(""),
                    instructions: Cow::from(""),
                    prompts: Cow::from(prompts),
                });
            };
            let answers: Vec<String> = response
                .map(|answer| String::from_utf8_lossy(&answer).into_owned())
                .collect();
            Ok(if answers == expected {
                russh::server::Auth::Accept
            } else {
                russh::server::Auth::reject()
            })
        }
    }

    /// Trusts any server: the test server's key is made up.
    struct TrustAll;

    impl client::Handler for TrustAll {
        type Error = russh::Error;

        async fn check_server_key(
            &mut self,
            _server_key: &PublicKeyOrCertificate,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    /// The questions put to the user.
    type Asked = Arc<Mutex<Vec<Challenge>>>;

    /// A prompter giving `answers` (`None`: the user cancels), recording the questions.
    fn prompter(answers: Option<&'static [&'static str]>, asked: &Asked) -> Prompter {
        let asked = Arc::clone(asked);
        Arc::new(move |challenge| {
            asked.lock().expect("asked").push(challenge);
            Box::pin(async move {
                answers.map(|answers| answers.iter().map(|&answer| answer.into()).collect())
            })
        })
    }

    /// Log in to a [`TwoFactor`] server (code `code`) with the test key file.
    async fn log_in(
        code: Option<&'static str>,
        credentials: Credentials,
    ) -> Result<bool, ConnectError> {
        let host_key = load_key(&fixture("rsa"), None).expect("host key");
        let user_key = load_key(&fixture("ed25519"), None).expect("user key");
        let config = russh::server::Config {
            keys: vec![host_key],
            methods: MethodSet::from(&[MethodKind::PublicKey, MethodKind::KeyboardInteractive][..]),
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        };
        let server = TwoFactor {
            key: user_key.public_key().clone(),
            code,
            key_accepted: false,
        };
        let (client_end, server_end) = tokio::io::duplex(64 * 1024);
        tokio::spawn(async move {
            if let Ok(running) =
                russh::server::run_stream(Arc::new(config), server_end, server).await
            {
                let _ = running.await;
            }
        });
        let mut ssh =
            client::connect_stream(Arc::new(client::Config::default()), client_end, TrustAll)
                .await
                .expect("handshake");
        let site = Site {
            id: String::from("two-factor-test"),
            protocol: Protocol::Sftp,
            host: String::from("test.example"),
            port: 22,
            user: String::from("tester"),
            password_saved: false,
            ftp: FtpOptions::default(),
            ssh: SshAuth {
                key_file: Some(fixture("ed25519")),
                use_agent: false,
                passphrase_saved: false,
            },
            files_at_once: crate::sites::DEFAULT_FILES_AT_ONCE,
            parts_per_file: crate::sites::DEFAULT_PARTS_PER_FILE,
            alias: String::new(),
            description: String::new(),
            folder: String::new(),
            verify: crate::sites::SiteVerify::default(),
        };
        authenticate(&mut ssh, &site, &credentials).await
    }

    fn with(password: Option<&str>, prompter: Option<Prompter>) -> Credentials {
        Credentials {
            password: password.map(SecretString::from),
            key_passphrase: None,
            prompter,
        }
    }

    #[tokio::test]
    async fn after_the_key_only_the_code_is_asked_and_the_password_answers_itself() {
        // Arrange
        let asked = Asked::default();
        let credentials = with(Some(PASSWORD), Some(prompter(Some(&[CODE]), &asked)));

        // Act
        let result = log_in(Some(CODE), credentials).await;

        // Assert
        assert!(result.expect("logged in"), "the user was asked");
        let asked = asked.lock().expect("asked");
        let [question] = asked.as_slice() else {
            panic!("expected one round of questions: {asked:?}");
        };
        assert_eq!(question.server, "tester@test.example");
        assert_eq!(
            question.prompts,
            vec![(String::from("Verification code: "), false)]
        );
    }

    #[tokio::test]
    async fn without_a_password_the_user_answers_both_questions() {
        let asked = Asked::default();
        let credentials = with(None, Some(prompter(Some(&[PASSWORD, CODE]), &asked)));

        let result = log_in(Some(CODE), credentials).await;

        assert!(result.is_ok(), "{result:?}");
        assert_eq!(asked.lock().expect("asked")[0].prompts.len(), 2);
    }

    #[tokio::test]
    async fn a_wrong_code_fails_the_login() {
        let asked = Asked::default();
        let credentials = with(Some(PASSWORD), Some(prompter(Some(&["000000"]), &asked)));

        let result = log_in(Some(CODE), credentials).await;

        assert!(
            matches!(result, Err(ConnectError::AuthenticationFailed)),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn cancelling_the_questions_cancels_the_login() {
        let asked = Asked::default();
        let credentials = with(Some(PASSWORD), Some(prompter(None, &asked)));

        let result = log_in(Some(CODE), credentials).await;

        assert!(matches!(result, Err(ConnectError::Cancelled)), "{result:?}");
    }

    #[tokio::test]
    async fn without_a_prompter_a_code_cannot_be_given() {
        let result = log_in(Some(CODE), with(Some(PASSWORD), None)).await;

        assert!(
            matches!(result, Err(ConnectError::AuthenticationFailed)),
            "{result:?}"
        );
    }

    /// A server asking only the password, its own way: answered without the user, so
    /// a second connection can log in by itself.
    #[tokio::test]
    async fn a_password_question_is_answered_without_asking_the_user() {
        let asked = Asked::default();
        let credentials = with(Some(PASSWORD), Some(prompter(Some(&[]), &asked)));

        let result = log_in(None, credentials).await;

        assert!(!result.expect("logged in"), "the user was not asked");
        assert!(asked.lock().expect("asked").is_empty());
    }

    /// Asked again, the password was wrong: the user is asked, not sent it twice.
    #[tokio::test]
    async fn a_wrong_password_is_not_sent_twice() {
        let asked = Asked::default();
        let credentials = with(Some("wrong"), Some(prompter(None, &asked)));

        let result = log_in(None, credentials).await;

        assert!(result.is_err(), "{result:?}");
        assert!(asked.lock().expect("asked").len() <= 1);
    }
}
