//! SFTP over SSH: verifying the server's host key against `~/.ssh/known_hosts`, logging
//! in, and the file operations behind [`super::Remote`].
//!
//! Logging in tries, in the order OpenSSH's `ssh` does: the site's private key file
//! (OpenSSH or PuTTY `.ppk`), then the keys held by the desktop's SSH agent, then the
//! password. The first one the server accepts wins.

use std::io::SeekFrom;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use russh::client;
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::known_hosts::learn_known_hosts;
use russh::keys::{
    HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate, check_known_hosts,
};
use russh_sftp::client::SftpSession;
use russh_sftp::client::fs::Metadata;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use secrecy::{ExposeSecret, SecretString};
use tokio::io::AsyncSeekExt;

use super::{
    CONNECT_TIMEOUT, ConnectError, Credentials, HostKey, RemoteMeta, RemoteReader, RemoteWriter,
    path_text,
};
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
    /// Keeps the SSH connection open for as long as the session is in use (and opens
    /// other channels on it, see [`Sftp::server_name`]).
    ssh: client::Handle<HostKeyCheck>,
}

/// Longest wait for the server to say its name.
const NAME_TIMEOUT: Duration = Duration::from_secs(3);

impl Sftp {
    /// The server's own name (`uname -n`), asked on a second channel of the SSH
    /// connection. `None` if the account may not run commands (SFTP-only accounts), the
    /// answer is not a plausible host name, or it takes longer than [`NAME_TIMEOUT`].
    pub(super) async fn server_name(&self) -> Option<String> {
        let ask = async {
            let mut channel = self.ssh.channel_open_session().await.ok()?;
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

    pub(super) async fn stat(&self, path: &RemotePath) -> Result<RemoteMeta, String> {
        let text = path_text(path);
        self.session
            .metadata(text.clone())
            .await
            .map(|metadata| meta(&metadata))
            .map_err(|error| format!("cannot read {text}: {error}"))
    }

    pub(super) async fn lstat(&self, path: &RemotePath) -> Result<RemoteMeta, String> {
        let text = path_text(path);
        self.session
            .symlink_metadata(text.clone())
            .await
            .map(|metadata| meta(&metadata))
            .map_err(|error| format!("cannot read {text}: {error}"))
    }

    pub(super) async fn read_dir(
        &self,
        dir: &RemotePath,
    ) -> Result<Vec<(String, RemoteMeta)>, String> {
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

    pub(super) async fn open_at(
        &self,
        path: &RemotePath,
        offset: u64,
    ) -> Result<RemoteReader, String> {
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
        path: &RemotePath,
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

    pub(super) async fn rename(&self, from: &RemotePath, to: &RemotePath) -> Result<(), String> {
        let (from_text, to_text) = (path_text(from), path_text(to));
        self.session
            .rename(from_text.clone(), to_text.clone())
            .await
            .map_err(|error| format!("cannot rename {from_text} to {to_text}: {error}"))
    }

    pub(super) async fn create_dir(&self, path: &RemotePath) -> Result<(), String> {
        let text = path_text(path);
        self.session
            .create_dir(text.clone())
            .await
            .map_err(|error| format!("cannot create folder {text}: {error}"))
    }

    pub(super) async fn remove_file(&self, path: &RemotePath) -> Result<(), String> {
        let text = path_text(path);
        self.session
            .remove_file(text.clone())
            .await
            .map_err(|error| format!("cannot delete {text}: {error}"))
    }

    pub(super) async fn remove_dir(&self, path: &RemotePath) -> Result<(), String> {
        let text = path_text(path);
        self.session
            .remove_dir(text.clone())
            .await
            .map_err(|error| format!("cannot delete folder {text}: {error}"))
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

    authenticate(&mut ssh, site, &credentials).await?;
    let network = |error: russh::Error| ConnectError::Network {
        host: site.host.clone(),
        reason: error.to_string(),
    };
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
    Ok((Sftp { session: sftp, ssh }, RemotePath::new(home)))
}

/// Log in: with the site's key file, then the agent's keys, then the password.
///
/// # Errors
///
/// Returns [`ConnectError::KeyPassphraseRequired`] or [`ConnectError::KeyUnreadable`]
/// when the key file cannot be used, [`ConnectError::AuthenticationFailed`] when the
/// password is refused, [`ConnectError::KeysRejected`] when only keys were tried and
/// all refused, and [`ConnectError::PasswordRequired`] when there was nothing to try
async fn authenticate(
    ssh: &mut client::Handle<HostKeyCheck>,
    site: &Site,
    credentials: &Credentials,
) -> Result<(), ConnectError> {
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
    let mut keys_tried = 0;

    // 1. The site's key file.
    if let Some(path) = &site.ssh.key_file {
        let key = load_key(path, credentials.key_passphrase.as_ref())?;
        keys_tried += 1;
        let auth = ssh
            .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), rsa_hash))
            .await
            .map_err(|error| failed(error.to_string()))?;
        if auth.success() {
            tracing::info!("logged in with the key file {}", path.display());
            return Ok(());
        }
        tracing::info!("the server did not accept the key file {}", path.display());
    }

    // 2. The SSH agent's keys (GNOME Keyring, ssh-agent, ...). No agent is fine.
    if site.ssh.use_agent {
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
                    keys_tried += 1;
                    let fingerprint = key.fingerprint(HashAlg::Sha256);
                    let auth = ssh
                        .authenticate_publickey_with(user, key, rsa_hash, &mut agent)
                        .await
                        .map_err(|error| failed(error.to_string()))?;
                    if auth.success() {
                        tracing::info!("logged in with the SSH agent's key {fingerprint}");
                        return Ok(());
                    }
                }
            }
            Err(error) => tracing::debug!("no SSH agent to use: {error}"),
        }
    }

    // 3. The password.
    let Some(password) = &credentials.password else {
        return Err(if keys_tried > 0 {
            ConnectError::KeysRejected
        } else {
            ConnectError::PasswordRequired
        });
    };
    let auth = ssh
        .authenticate_password(user, password.expose_secret())
        .await
        .map_err(|error| failed(error.to_string()))?;
    if auth.success() {
        Ok(())
    } else {
        Err(ConnectError::AuthenticationFailed)
    }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use secrecy::SecretString;

    use super::*;

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
}
