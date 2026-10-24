//! FTP and FTPS (explicit or implicit TLS): logging in, and the file operations behind
//! [`super::Remote`].
//!
//! An FTP connection carries one file transfer at a time, so several transfers in
//! parallel need several connections: they come from a small pool, opened as needed
//! (up to [`MAX_CONNECTIONS`]) and reused. A pooled connection the server has closed
//! meanwhile (servers drop idle ones) is replaced and the operation retried once.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use suppaftp::list::{File, ListParser};
use suppaftp::tokio::AsyncRustlsStream;
use suppaftp::tokio::{AsyncRustlsConnector, AsyncRustlsFtpStream, TransferStream};
use suppaftp::types::FileType;
use suppaftp::{FtpError, Mode, Status};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::tls::{self, RejectedSlot};
use super::{CONNECT_TIMEOUT, ConnectError, RemoteMeta, RemoteReader, RemoteWriter, path_text};
use crate::path::RemotePath;
use crate::sites::{FtpEncryption, Site};

/// Most connections opened to one server at once. Enough for a few parallel transfers
/// plus browsing; many servers refuse more than a handful per client.
const MAX_CONNECTIONS: usize = 6;
/// How long active mode waits for the server to connect back.
const ACTIVE_MODE_TIMEOUT: Duration = Duration::from_secs(15);

/// One FTP connection (with or without TLS; the type is the same).
type Connection = AsyncRustlsFtpStream;

/// How to open a connection to the server.
struct Settings {
    host: String,
    port: u16,
    user: String,
    password: SecretString,
    encryption: FtpEncryption,
    passive: bool,
    /// TLS settings, for encrypted sites.
    tls: Option<Arc<rustls::ClientConfig>>,
    /// "TLS if available" found the server does not offer TLS: further connections
    /// go straight to plain FTP.
    plain_fallback: AtomicBool,
    /// The server said it does not know MLSD (or MLST): further listings (or lookups)
    /// go straight to the older way, saving a refused command each time.
    no_mlsd: AtomicBool,
    no_mlst: AtomicBool,
}

/// An FTP server: its settings and a pool of open connections.
pub(super) struct Ftp {
    settings: Arc<Settings>,
    pool: Arc<Pool>,
}

/// Open connections not in use, and the limit on how many may exist.
struct Pool {
    idle: Mutex<Vec<Connection>>,
    slots: Arc<Semaphore>,
}

/// A connection checked out of the pool; it goes back when dropped (unless discarded).
struct Lease {
    connection: Option<Connection>,
    pool: Arc<Pool>,
    /// Holding it counts against [`MAX_CONNECTIONS`].
    _slot: OwnedSemaphorePermit,
    /// Came from the pool (so it may have been closed by the server meanwhile).
    reused: bool,
}

impl Lease {
    fn connection(&mut self) -> &mut Connection {
        // Only `discard` takes the connection, and it consumes the lease.
        self.connection.as_mut().expect("leased connection")
    }

    /// Drop the connection instead of returning it to the pool (it is broken).
    fn discard(mut self) {
        self.connection = None;
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let (Some(connection), Ok(mut idle)) = (self.connection.take(), self.pool.idle.lock()) {
            idle.push(connection);
        }
    }
}

/// Connect to an FTP site and log in.
///
/// # Returns
///
/// The server and the user's home folder
///
/// # Errors
///
/// Returns a [`ConnectError`] describing the step that failed
pub(super) async fn connect(
    site: &Site,
    password: Option<SecretString>,
) -> Result<(Ftp, RemotePath), ConnectError> {
    // FTP servers commonly accept "anonymous" with any password (often an email address).
    let password = match password {
        Some(password) => password,
        None if site.user.eq_ignore_ascii_case("anonymous") => SecretString::from("shuttlescp@"),
        None => return Err(ConnectError::PasswordRequired),
    };
    let encryption = site.ftp.encryption;
    let (tls, rejected) = match encryption {
        FtpEncryption::None => (None, RejectedSlot::default()),
        FtpEncryption::ExplicitIfAvailable
        | FtpEncryption::ExplicitTls
        | FtpEncryption::ImplicitTls => {
            let (config, rejected) =
                tls::client_config(&site.host, site.port).map_err(ConnectError::Ftp)?;
            (Some(config), rejected)
        }
    };
    let settings = Arc::new(Settings {
        host: site.host.clone(),
        port: site.port,
        user: site.user.clone(),
        password,
        encryption,
        passive: site.ftp.passive,
        tls,
        plain_fallback: AtomicBool::new(false),
        no_mlsd: AtomicBool::new(false),
        no_mlst: AtomicBool::new(false),
    });
    tracing::info!(
        "connecting to {}:{} (FTP, {}, {} mode)",
        site.host,
        site.port,
        encryption.label(),
        if site.ftp.passive {
            "passive"
        } else {
            "active"
        }
    );
    let mut first = open(&settings).await.map_err(|error| {
        // A refused certificate shows up as a TLS error; say which certificate.
        match rejected.lock().ok().and_then(|mut slot| slot.take()) {
            Some(refused) => ConnectError::UntrustedCertificate {
                host: site.host.clone(),
                port: site.port,
                fingerprint: refused.fingerprint,
                reason: refused.reason,
            },
            None => error,
        }
    })?;
    if encryption == FtpEncryption::None || settings.plain_fallback.load(Ordering::Relaxed) {
        tracing::warn!(
            "{}: plain FTP sends the password and files unencrypted",
            site.host
        );
    }
    let home = first
        .pwd()
        .await
        .map_err(|error| ConnectError::Ftp(format!("cannot read the home folder: {error}")))?;
    let pool = Arc::new(Pool {
        idle: Mutex::new(vec![first]),
        slots: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
    });
    Ok((Ftp { settings, pool }, RemotePath::new(home)))
}

/// Open and log in one connection.
async fn open(settings: &Settings) -> Result<Connection, ConnectError> {
    let host = settings.host.clone();
    let network = |error: FtpError| ConnectError::Network {
        host: host.clone(),
        reason: error.to_string(),
    };
    let address = (settings.host.as_str(), settings.port);
    let connector = settings.tls.as_ref().map(|config| {
        AsyncRustlsConnector::from(tokio_rustls::TlsConnector::from(Arc::clone(config)))
    });
    let mut connection = match (settings.encryption, connector) {
        (FtpEncryption::ImplicitTls, Some(connector)) => timed(
            &settings.host,
            Connection::connect_secure_implicit(address, connector, &settings.host),
        )
        .await?
        .map_err(network)?,
        (
            encryption @ (FtpEncryption::ExplicitTls | FtpEncryption::ExplicitIfAvailable),
            Some(connector),
        ) if !settings.plain_fallback.load(Ordering::Relaxed) => {
            let plain = timed(&settings.host, Connection::connect(address))
                .await?
                .map_err(network)?;
            match timed(&settings.host, plain.into_secure(connector, &settings.host)).await? {
                Ok(secure) => secure,
                // The server refused AUTH TLS: it does not offer TLS.
                Err(FtpError::UnexpectedResponse(response)) => {
                    let reply = String::from_utf8_lossy(&response.body).trim().to_owned();
                    if encryption == FtpEncryption::ExplicitTls {
                        return Err(ConnectError::Ftp(format!(
                            "the server does not offer TLS encryption ({reply}); choose \
                             \"{}\" to connect without requiring it",
                            FtpEncryption::ExplicitIfAvailable.label()
                        )));
                    }
                    tracing::warn!(
                        "{} does not offer TLS ({reply}); continuing with plain FTP",
                        settings.host
                    );
                    settings.plain_fallback.store(true, Ordering::Relaxed);
                    // Asking for TLS used up that connection; start a plain one.
                    timed(&settings.host, Connection::connect(address))
                        .await?
                        .map_err(network)?
                }
                Err(other) => return Err(network(other)),
            }
        }
        _ => timed(&settings.host, Connection::connect(address))
            .await?
            .map_err(network)?,
    };
    if settings.encryption == FtpEncryption::ImplicitTls {
        // Explicit TLS negotiates data-channel protection while switching to TLS; with
        // implicit TLS it must be asked for, or files would travel unencrypted (or the
        // server would refuse them).
        for command in ["PBSZ 0", "PROT P"] {
            connection
                .custom_command(command, &[Status::CommandOk])
                .await
                .map_err(|error| ConnectError::Ftp(format!("{command}: {}", describe(&error))))?;
        }
    }
    connection
        .login(settings.user.as_str(), settings.password.expose_secret())
        .await
        .map_err(|error| match error {
            FtpError::UnexpectedResponse(_) => ConnectError::AuthenticationFailed,
            other => network(other),
        })?;
    // Exact bytes, not text with line endings converted.
    connection
        .transfer_type(FileType::Binary)
        .await
        .map_err(|error| ConnectError::Ftp(error.to_string()))?;
    // Ask for UTF-8 names; servers that always use UTF-8 (or do not know) may refuse.
    let _ = connection.opts("UTF8", Some("ON")).await;
    if settings.passive {
        connection.set_mode(Mode::Passive);
        // Use the server's address for data connections even if it reports a private
        // one (a common misconfiguration behind NAT).
        connection.set_passive_nat_workaround(true);
    } else {
        connection = connection.active_mode(ACTIVE_MODE_TIMEOUT);
    }
    Ok(connection)
}

/// Wait for `future`, giving up after [`CONNECT_TIMEOUT`].
async fn timed<T>(host: &str, future: impl Future<Output = T>) -> Result<T, ConnectError> {
    tokio::time::timeout(CONNECT_TIMEOUT, future)
        .await
        .map_err(|_| ConnectError::Timeout(host.to_owned()))
}

/// Start downloading `path` from `offset` on.
async fn start_download(
    connection: &mut Connection,
    path: String,
    offset: u64,
) -> Result<TransferStream<AsyncRustlsStream>, FtpError> {
    if offset > 0 {
        connection.resume_transfer(to_usize(offset)?).await?;
    }
    connection.retr_as_stream(path).await
}

/// Start uploading to `path`: from scratch, or after its first `offset` bytes by
/// appending (APPE) to it, as it is exactly that long.
async fn start_upload(
    connection: &mut Connection,
    path: String,
    offset: u64,
) -> Result<TransferStream<AsyncRustlsStream>, FtpError> {
    if offset > 0 {
        connection.append_with_stream(path).await
    } else {
        connection.put_with_stream(path).await
    }
}

/// An FTP command run on a leased connection.
type Operation<T> = dyn for<'a> Fn(&'a mut Connection) -> Pin<Box<dyn Future<Output = Result<T, FtpError>> + Send + 'a>>
    + Send
    + Sync;

impl Ftp {
    /// Take a connection from the pool, or open a new one (waiting while
    /// [`MAX_CONNECTIONS`] are in use).
    async fn lease(&self) -> Result<Lease, String> {
        let slot = Arc::clone(&self.pool.slots)
            .acquire_owned()
            .await
            .map_err(|error| error.to_string())?;
        let idle = self.pool.idle.lock().ok().and_then(|mut idle| idle.pop());
        let (connection, reused) = match idle {
            Some(connection) => (connection, true),
            None => (
                open(&self.settings)
                    .await
                    .map_err(|error| error.to_string())?,
                false,
            ),
        };
        Ok(Lease {
            connection: Some(connection),
            pool: Arc::clone(&self.pool),
            _slot: slot,
            reused,
        })
    }

    /// Run `operation` on a leased connection. If a reused connection turns out to be
    /// closed, it is replaced and the operation tried once more.
    ///
    /// # Arguments
    ///
    /// * `what` - Describes the operation for error messages, e.g. "cannot delete x"
    async fn run<T>(&self, what: &str, operation: &Operation<T>) -> Result<T, String> {
        loop {
            let mut lease = self
                .lease()
                .await
                .map_err(|error| format!("{what}: {error}"))?;
            match operation(lease.connection()).await {
                Ok(value) => return Ok(value),
                Err(error) if lease.reused && is_broken(&error) => {
                    tracing::debug!("FTP connection closed by the server ({error}); reconnecting");
                    lease.discard();
                }
                Err(error) => return Err(format!("{what}: {}", describe(&error))),
            }
        }
    }

    pub(super) async fn stat(&self, path: &RemotePath) -> Result<RemoteMeta, String> {
        let text = path_text(path);
        let found = if self.settings.no_mlst.load(Ordering::Relaxed) {
            None
        } else {
            let mlst_path = text.clone();
            let found = self
                .run(&format!("cannot read {text}"), &move |connection| {
                    let path = mlst_path.clone();
                    Box::pin(async move {
                        unless_unsupported(connection.mlst(Some(path.as_str())).await)
                    })
                })
                .await;
            if matches!(found, Ok(None)) {
                tracing::debug!("the server has no MLST; looking items up in listings from now on");
                self.settings.no_mlst.store(true, Ordering::Relaxed);
            }
            found.ok().flatten()
        };
        match found {
            Some(line) => ListParser::parse_mlst(&line)
                .map(|file| meta(&file))
                .map_err(|error| format!("cannot read {text}: {error}")),
            // Servers without MLST: look the item up in its folder's listing.
            None => {
                let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                    // The root folder.
                    return Ok(RemoteMeta {
                        is_dir: true,
                        is_symlink: false,
                        size: 0,
                        modified: None,
                    });
                };
                self.read_dir(&parent)
                    .await?
                    .into_iter()
                    .find(|(entry, _)| entry == name)
                    .map(|(_, meta)| meta)
                    .ok_or_else(|| format!("cannot read {text}: no such file or folder"))
            }
        }
    }

    pub(super) async fn read_dir(
        &self,
        dir: &RemotePath,
    ) -> Result<Vec<(String, RemoteMeta)>, String> {
        let text = path_text(dir);
        let what = format!("cannot open {text}");
        let lines = if self.settings.no_mlsd.load(Ordering::Relaxed) {
            None
        } else {
            let mlsd_path = text.clone();
            let lines = self
                .run(&what, &move |connection| {
                    let path = mlsd_path.clone();
                    Box::pin(async move {
                        unless_unsupported(connection.mlsd(Some(path.as_str())).await)
                    })
                })
                .await;
            if matches!(lines, Ok(None)) {
                tracing::debug!("the server has no MLSD; listing with LIST from now on");
                self.settings.no_mlsd.store(true, Ordering::Relaxed);
            }
            lines.ok().flatten()
        };
        let files: Vec<File> = match lines {
            Some(lines) => lines
                .iter()
                .filter_map(|line| ListParser::parse_mlsd(line).ok())
                .collect(),
            // Servers without MLSD: parse the older, free-form LIST output.
            None => {
                let list_path = text.clone();
                self.run(&what, &move |connection| {
                    let path = list_path.clone();
                    Box::pin(async move { connection.list(Some(path.as_str())).await })
                })
                .await?
                .iter()
                .filter_map(|line| {
                    ListParser::parse_posix(line)
                        .or_else(|_| ListParser::parse_dos(line))
                        .ok()
                })
                .collect()
            }
        };
        Ok(files
            .iter()
            .filter(|file| file.name() != "." && file.name() != "..")
            .map(|file| (file.name().to_owned(), meta(file)))
            .collect())
    }

    /// Whether `path` (a symbolic link) leads to a folder: one CWD into it, which needs
    /// no data connection, unlike a listing. Every path used is absolute, so leaving a
    /// pooled connection in that folder is harmless.
    pub(super) async fn leads_to_folder(&self, path: &RemotePath) -> bool {
        let text = path_text(path);
        self.run(&format!("cannot open {text}"), &move |connection| {
            let path = text.clone();
            Box::pin(async move { connection.cwd(path.as_str()).await })
        })
        .await
        .is_ok()
    }

    pub(super) async fn open_at(
        &self,
        path: &RemotePath,
        offset: u64,
    ) -> Result<RemoteReader, String> {
        let text = path_text(path);
        let mut lease = self.lease().await?;
        let stream = match start_download(lease.connection(), text.clone(), offset).await {
            Ok(stream) => stream,
            Err(error) if lease.reused && is_broken(&error) => {
                lease.discard();
                lease = self.lease().await?;
                start_download(lease.connection(), text.clone(), offset)
                    .await
                    .map_err(|error| format!("cannot open {text}: {}", describe(&error)))?
            }
            Err(error) => return Err(format!("cannot open {text}: {}", describe(&error))),
        };
        Ok(Box::pin(FtpReader {
            stream: Some(stream),
            lease: Some(lease),
            finishing: None,
        }))
    }

    pub(super) async fn open_write_at(
        &self,
        path: &RemotePath,
        offset: u64,
    ) -> Result<RemoteWriter, String> {
        let text = path_text(path);
        let mut lease = self.lease().await?;
        let stream = match start_upload(lease.connection(), text.clone(), offset).await {
            Ok(stream) => stream,
            Err(error) if lease.reused && is_broken(&error) => {
                lease.discard();
                lease = self.lease().await?;
                start_upload(lease.connection(), text.clone(), offset)
                    .await
                    .map_err(|error| format!("cannot write {text}: {}", describe(&error)))?
            }
            Err(error) => return Err(format!("cannot write {text}: {}", describe(&error))),
        };
        Ok(Box::pin(FtpWriter {
            stream: Some(stream),
            lease: Some(lease),
            finishing: None,
        }))
    }

    pub(super) async fn rename(&self, from: &RemotePath, to: &RemotePath) -> Result<(), String> {
        let (from, to) = (path_text(from), path_text(to));
        let what = format!("cannot rename {from} to {to}");
        self.run(&what, &move |connection| {
            let (from, to) = (from.clone(), to.clone());
            Box::pin(async move { connection.rename(from, to).await })
        })
        .await
    }

    pub(super) async fn create_dir(&self, path: &RemotePath) -> Result<(), String> {
        let text = path_text(path);
        let what = format!("cannot create folder {text}");
        self.run(&what, &move |connection| {
            let path = text.clone();
            Box::pin(async move { connection.mkdir(path).await })
        })
        .await
    }

    pub(super) async fn remove_file(&self, path: &RemotePath) -> Result<(), String> {
        let text = path_text(path);
        let what = format!("cannot delete {text}");
        self.run(&what, &move |connection| {
            let path = text.clone();
            Box::pin(async move { connection.rm(path).await })
        })
        .await
    }

    pub(super) async fn remove_dir(&self, path: &RemotePath) -> Result<(), String> {
        let text = path_text(path);
        let what = format!("cannot delete folder {text}");
        self.run(&what, &move |connection| {
            let path = text.clone();
            Box::pin(async move { connection.rmdir(path).await })
        })
        .await
    }

    /// Set the modification time with MFMT, a widespread extension (not every server
    /// has it).
    pub(super) async fn set_modified(
        &self,
        path: &RemotePath,
        time: SystemTime,
    ) -> Result<(), String> {
        let text = path_text(path);
        let stamp = DateTime::<Utc>::from(time)
            .format("%Y%m%d%H%M%S")
            .to_string();
        let what = format!("cannot set the time of {text}");
        self.run(&what, &move |connection| {
            let command = format!("MFMT {stamp} {text}");
            Box::pin(async move {
                connection
                    .custom_command(command, &[Status::File])
                    .await
                    .map(|_| ())
            })
        })
        .await
    }
}

/// [`RemoteMeta`] from a listing entry.
fn meta(file: &File) -> RemoteMeta {
    RemoteMeta {
        is_dir: file.is_directory(),
        is_symlink: file.is_symlink(),
        // `usize` always fits in a `u64` on the platforms ShuttleSCP runs on.
        size: file.size() as u64,
        modified: Some(file.modified()),
    }
}

/// A command's result, with "the server does not know this command" (500, 502, 504)
/// as `None` rather than an error.
fn unless_unsupported<T>(result: Result<T, FtpError>) -> Result<Option<T>, FtpError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if unsupported(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Whether the server refused a command because it does not implement it.
fn unsupported(error: &FtpError) -> bool {
    matches!(
        error,
        FtpError::UnexpectedResponse(response)
            if matches!(
                response.status,
                Status::BadCommand | Status::NotImplemented | Status::NotImplementedParameter
            )
    )
}

/// Whether an error means the connection itself is gone (so a fresh one may work).
fn is_broken(error: &FtpError) -> bool {
    match error {
        FtpError::ConnectionError(_) | FtpError::BadResponse => true,
        // 421: the server is closing the connection (e.g. idle timeout).
        FtpError::UnexpectedResponse(response) => response.status == Status::NotAvailable,
        _ => false,
    }
}

/// An FTP error as a readable message: the server's reply text when there is one.
fn describe(error: &FtpError) -> String {
    match error {
        FtpError::UnexpectedResponse(response) => {
            String::from_utf8_lossy(&response.body).trim().to_owned()
        }
        other => other.to_string(),
    }
}

/// A file offset as the `usize` the FTP library takes.
fn to_usize(offset: u64) -> Result<usize, FtpError> {
    usize::try_from(offset).map_err(|_| {
        FtpError::ConnectionError(io::Error::new(
            io::ErrorKind::InvalidInput,
            "offset too large",
        ))
    })
}

/// A file being downloaded. At the end of the file it reads the server's "transfer
/// complete" reply before the connection goes back to the pool. (If it is dropped
/// earlier, the connection reads that reply before its next command.)
struct FtpReader {
    stream: Option<TransferStream<AsyncRustlsStream>>,
    /// Keeps the connection out of the pool until the transfer ends.
    lease: Option<Lease>,
    /// The server's reply, once the end of the file was reached.
    finishing: Option<Pin<Box<dyn Future<Output = io::Result<()>> + Send>>>,
}

impl AsyncRead for FtpReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if let Some(finishing) = self.finishing.as_mut() {
            return finishing.as_mut().poll(cx);
        }
        let Some(stream) = self.stream.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        let before = buf.filled().len();
        let read = Pin::new(stream).poll_read(cx, buf);
        if !matches!(read, Poll::Ready(Ok(()))) || buf.filled().len() > before {
            return read;
        }
        // End of the file: finish the transfer, then report the end.
        let (Some(stream), lease) = (self.stream.take(), self.lease.take()) else {
            return Poll::Ready(Ok(()));
        };
        let mut finishing: Pin<Box<dyn Future<Output = io::Result<()>> + Send>> =
            Box::pin(async move {
                let result = stream.finish().await;
                drop(lease);
                result.map_err(|error| io::Error::other(describe(&error)))
            });
        let poll = finishing.as_mut().poll(cx);
        self.finishing = Some(finishing);
        poll
    }
}

/// A file being uploaded. Shutting it down waits for the server to confirm the file
/// was stored, so a finished upload is really complete.
struct FtpWriter {
    stream: Option<TransferStream<AsyncRustlsStream>>,
    lease: Option<Lease>,
    /// The server's confirmation, once shutting down started.
    finishing: Option<Pin<Box<dyn Future<Output = io::Result<()>> + Send>>>,
}

impl FtpWriter {
    fn stream(&mut self) -> io::Result<&mut TransferStream<AsyncRustlsStream>> {
        self.stream
            .as_mut()
            .ok_or_else(|| io::Error::other("the upload is already finished"))
    }
}

impl AsyncWrite for FtpWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.stream() {
            Ok(stream) => Pin::new(stream).poll_write(cx, buf),
            Err(error) => Poll::Ready(Err(error)),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.stream() {
            Ok(stream) => Pin::new(stream).poll_flush(cx),
            Err(_) => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.finishing.is_none() {
            let (Some(stream), lease) = (self.stream.take(), self.lease.take()) else {
                return Poll::Ready(Ok(()));
            };
            self.finishing = Some(Box::pin(async move {
                let result = stream.finish().await;
                // Back to the pool only once the transfer is complete.
                drop(lease);
                result.map_err(|error| io::Error::other(describe(&error)))
            }));
        }
        match self.finishing.as_mut() {
            Some(finishing) => finishing.as_mut().poll(cx),
            None => Poll::Ready(Ok(())),
        }
    }
}
