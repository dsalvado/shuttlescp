//! Site-to-site copies against real servers, every protocol pairing.
//!
//! These need an SFTP and an FTP server, so they are ignored by a plain `cargo test`;
//! `scripts/test-site-to-site.sh` starts both in containers and runs them:
//!
//! ```sh
//! SHUTTLESCP_TEST_SFTP=sftp://user:password@host:port/folder \
//! SHUTTLESCP_TEST_FTP=ftp://user:password@host:port/folder \
//!     cargo test -p shuttlescp-core --test site_to_site -- --ignored
//! ```
//!
//! Each folder must exist and be writable. SSH host keys are trusted on first use, so
//! run them with a throwaway `HOME` (the script does) to keep `~/.ssh/known_hosts` clean.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use secrecy::SecretString;
use shuttlescp_core::entry::EntryKind;
use shuttlescp_core::path::{RemotePath, SidePath};
use shuttlescp_core::remote::{ConnectError, Credentials, Remote, connect};
use shuttlescp_core::sites::{FtpOptions, Protocol, Site, SshAuth};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use shuttlescp_core::transfer::{
    CancelFlag, ItemOutcome, Job, JobKind, JobSummary, Side, TransferEvent, Verification, Verify,
    run,
};

/// A test server, from `SHUTTLESCP_TEST_SFTP` or `SHUTTLESCP_TEST_FTP`.
struct Server {
    site: Site,
    password: String,
    /// A writable folder on it.
    dir: RemotePath,
}

impl Server {
    /// Read `scheme://user:password@host:port/folder` from the environment variable.
    /// Without `:password`, an SSH server is logged in to with the SSH agent's keys.
    fn from_env(var: &str) -> Self {
        let url = std::env::var(var).unwrap_or_else(|_| panic!("{var} is not set"));
        let (scheme, rest) = url.split_once("://").expect("scheme://");
        let (login, rest) = rest.split_once('@').expect("user@");
        let (user, password) = login.split_once(':').unwrap_or((login, ""));
        let (address, dir) = rest.split_once('/').expect("host:port/folder");
        let (host, port) = address.split_once(':').expect("host:port");
        let protocol = match scheme {
            "sftp" => Protocol::Sftp,
            "ftp" => Protocol::Ftp,
            other => panic!("unknown scheme {other}"),
        };
        Self {
            site: Site {
                id: format!("test-{scheme}"),
                protocol,
                host: host.to_owned(),
                port: port.parse().expect("port"),
                user: user.to_owned(),
                password_saved: false,
                ftp: FtpOptions::default(),
                ssh: SshAuth {
                    key_file: None,
                    use_agent: password.is_empty(),
                    passphrase_saved: false,
                },
                files_at_once: shuttlescp_core::sites::DEFAULT_FILES_AT_ONCE,
                alias: String::new(),
                description: String::new(),
                folder: String::new(),
                verify: shuttlescp_core::sites::SiteVerify::default(),
            },
            password: password.to_owned(),
            dir: RemotePath::new(format!("/{dir}")),
        }
    }

    /// Log in (a new connection each time, as two sessions would have), trusting an
    /// SSH host key seen for the first time.
    async fn connect(&self) -> Remote {
        let credentials = || Credentials {
            password: (!self.password.is_empty())
                .then(|| SecretString::from(self.password.clone())),
            key_passphrase: None,
        };
        match connect(&self.site, credentials(), None).await {
            Ok(remote) => remote,
            Err(ConnectError::UnknownHostKey { key, .. }) => {
                connect(&self.site, credentials(), Some(*key))
                    .await
                    .expect("log in after trusting the host key")
            }
            Err(error) => panic!("cannot log in to {}: {error}", self.site.host),
        }
    }
}

/// Run `job` with these connections; its totals, and why items failed (if any did).
async fn run_job(job: Job, remote: &Remote, dest: Option<&Remote>) -> (JobSummary, Vec<String>) {
    let errors = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&errors);
    let summary = run(
        job,
        Some(remote.clone()),
        dest.cloned(),
        CancelFlag::default(),
        move |event| {
            if let TransferEvent::ItemFinished {
                outcome: ItemOutcome::Failed(error),
                ..
            } = event
            {
                seen.lock().expect("lock").push(error);
            }
        },
    )
    .await;
    let errors = errors.lock().expect("lock").clone();
    (summary, errors)
}

/// A job; copies and moves from one server to another are site to site.
fn job(kind: JobKind, side: Side, sources: Vec<SidePath>, dest_dir: SidePath) -> Job {
    Job {
        between_servers: kind != JobKind::Delete
            && side == Side::Remote
            && dest_dir.side() == Side::Remote,
        left_server: false,
        kind,
        side,
        sources,
        dest_dir,
        same_side: false,
        resume: false,
        verify: Verify::default(),
    }
}

/// A folder named `name` in `parent`: `a.txt`, `sub/b.bin` (600 kB, several chunks) and
/// `c.bin` (1 MiB, exactly four chunks).
fn make_tree(parent: &Path, name: &str) -> PathBuf {
    let root = parent.join(name);
    std::fs::create_dir_all(root.join("sub")).expect("create folders");
    std::fs::write(root.join("a.txt"), "hello from a").expect("write");
    std::fs::write(root.join("sub/b.bin"), pattern(600_000)).expect("write");
    std::fs::write(root.join("c.bin"), pattern(1024 * 1024)).expect("write");
    root
}

fn pattern(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

/// Both trees hold the same files with the same contents.
fn assert_same_tree(expected: &Path, actual: &Path) {
    for file in ["a.txt", "sub/b.bin", "c.bin"] {
        let (want, got) = (expected.join(file), actual.join(file));
        assert_eq!(
            std::fs::read(&want).expect("read the original"),
            std::fs::read(&got).unwrap_or_else(|error| panic!("{}: {error}", got.display())),
            "{file} differs"
        );
    }
}

/// Copy a tree from `from` to `to` site to site, check it arrived whole (by downloading
/// it back), then move it back and check the move removed it from `to`.
async fn round_trip(from: &Server, to: &Server, to_dir: RemotePath) {
    let (a, b) = (from.connect().await, to.connect().await);
    let temp = tempfile::tempdir().expect("temp dir");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let name = format!("s2s-{stamp}");
    let tree = make_tree(temp.path(), &name);
    let on_a = SidePath::Remote(from.dir.join(&name));
    let on_b = SidePath::Remote(to_dir.join(&name));

    // Put the tree on server A.
    let upload = job(
        JobKind::Copy,
        Side::Local,
        vec![SidePath::Local(tree.clone())],
        SidePath::Remote(from.dir.clone()),
    );
    let (summary, errors) = run_job(upload, &a, None).await;
    assert!(summary.is_complete(), "upload to A: {summary:?} {errors:?}");

    // A to B, through this computer.
    let copy = job(
        JobKind::Copy,
        Side::Remote,
        vec![on_a.clone()],
        SidePath::Remote(to_dir.clone()),
    );
    let (summary, errors) = run_job(copy, &a, Some(&b)).await;
    assert!(summary.is_complete(), "copy A to B: {summary:?} {errors:?}");
    assert_eq!(summary.done, 5, "the folder, sub and three files");
    assert_eq!(summary.bytes, 12 + 600_000 + 1024 * 1024);

    // What B has, back here, matches the original.
    let back = temp.path().join("back");
    std::fs::create_dir(&back).expect("create");
    let download = job(
        JobKind::Copy,
        Side::Remote,
        vec![on_b.clone()],
        SidePath::Local(back.clone()),
    );
    let (summary, errors) = run_job(download, &b, None).await;
    assert!(
        summary.is_complete(),
        "download from B: {summary:?} {errors:?}"
    );
    assert_same_tree(&tree, &back.join(&name));

    // Move it from B back to A (replacing A's copy): gone from B afterwards.
    let delete_a = job(
        JobKind::Delete,
        Side::Remote,
        vec![on_a.clone()],
        on_a.clone(),
    );
    let (summary, errors) = run_job(delete_a, &a, None).await;
    assert!(summary.is_complete(), "delete on A: {summary:?} {errors:?}");
    let back_to_a = job(
        JobKind::Move,
        Side::Remote,
        vec![on_b.clone()],
        SidePath::Remote(from.dir.clone()),
    );
    let (summary, errors) = run_job(back_to_a, &b, Some(&a)).await;
    assert!(summary.is_complete(), "move B to A: {summary:?} {errors:?}");
    let listing = b.list_dir(&to_dir).await.expect("list B");
    assert!(
        listing
            .iter()
            .all(|entry| entry.name().as_os_str() != name.as_str()),
        "the move left {name} on B"
    );

    // Clean up A.
    let delete_a = job(JobKind::Delete, Side::Remote, vec![on_a.clone()], on_a);
    let (summary, errors) = run_job(delete_a, &a, None).await;
    assert!(summary.is_complete(), "clean up A: {summary:?} {errors:?}");
}

/// A second writable folder on the same server, for a pairing with itself.
fn other_dir(server: &Server) -> RemotePath {
    server.dir.join("other")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn sftp_to_ftp_and_back() {
    let (sftp, ftp) = (
        Server::from_env("SHUTTLESCP_TEST_SFTP"),
        Server::from_env("SHUTTLESCP_TEST_FTP"),
    );
    round_trip(&sftp, &ftp, ftp.dir.clone()).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn ftp_to_sftp_and_back() {
    let (sftp, ftp) = (
        Server::from_env("SHUTTLESCP_TEST_SFTP"),
        Server::from_env("SHUTTLESCP_TEST_FTP"),
    );
    round_trip(&ftp, &sftp, sftp.dir.clone()).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn sftp_to_sftp_and_back() {
    let sftp = Server::from_env("SHUTTLESCP_TEST_SFTP");
    round_trip(&sftp, &sftp, other_dir(&sftp)).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn ftp_to_ftp_and_back() {
    let ftp = Server::from_env("SHUTTLESCP_TEST_FTP");
    round_trip(&ftp, &ftp, other_dir(&ftp)).await;
}

/// How fast a large file goes from one real server to another, site to site. Not a
/// pass/fail test: it prints the speed of three runs. Uses the SSH agent and the
/// servers' entries in `~/.ssh/known_hosts` (run with your real HOME):
///
/// ```sh
/// SHUTTLESCP_LIVE_A=sftp://me@server-a:22/home/me SHUTTLESCP_LIVE_B=sftp://me@server-b:22/home/me \
///     cargo test --release -p shuttlescp-core --test site_to_site live_relay_speed -- --ignored --nocapture
/// ```
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs two real servers; see the comment"]
async fn live_relay_speed() {
    const SIZE: usize = 200 * 1024 * 1024;
    let (from, to) = (
        Server::from_env("SHUTTLESCP_LIVE_A"),
        Server::from_env("SHUTTLESCP_LIVE_B"),
    );
    let (a, b) = (from.connect().await, to.connect().await);
    let temp = tempfile::tempdir().expect("temp dir");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let name = format!("s2s-speed-{stamp}");
    let folder = temp.path().join(&name);
    std::fs::create_dir(&folder).expect("create");
    std::fs::write(folder.join("big.bin"), pattern(SIZE)).expect("write");
    let on_a = SidePath::Remote(from.dir.join(&name));
    let on_b = SidePath::Remote(to.dir.join(&name));
    let upload = job(
        JobKind::Copy,
        Side::Local,
        vec![SidePath::Local(folder)],
        SidePath::Remote(from.dir.clone()),
    );
    let (summary, errors) = run_job(upload, &a, None).await;
    assert!(summary.is_complete(), "upload to A: {summary:?} {errors:?}");

    for run in 1..=3 {
        let started = std::time::Instant::now();
        let copy = job(
            JobKind::Copy,
            Side::Remote,
            vec![on_a.clone()],
            SidePath::Remote(to.dir.clone()),
        );
        let (summary, errors) = run_job(copy, &a, Some(&b)).await;
        let seconds = started.elapsed().as_secs_f64();
        assert!(summary.is_complete(), "copy A to B: {summary:?} {errors:?}");
        println!(
            "run {run}: {SIZE} bytes in {seconds:.1} s = {:.1} MiB/s",
            SIZE as f64 / seconds / 1024.0 / 1024.0
        );
        let delete_b = job(
            JobKind::Delete,
            Side::Remote,
            vec![on_b.clone()],
            on_b.clone(),
        );
        let (summary, errors) = run_job(delete_b, &b, None).await;
        assert!(summary.is_complete(), "delete on B: {summary:?} {errors:?}");
    }
    let delete_a = job(JobKind::Delete, Side::Remote, vec![on_a.clone()], on_a);
    let (summary, errors) = run_job(delete_a, &a, None).await;
    assert!(summary.is_complete(), "clean up A: {summary:?} {errors:?}");
}

/// The two real servers' own names, as ShuttleSCP shows them next to their address
/// (needs the same SHUTTLESCP_LIVE_A / _B as `live_relay_speed`).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs two real servers; see live_relay_speed"]
async fn live_server_names() {
    for var in ["SHUTTLESCP_LIVE_A", "SHUTTLESCP_LIVE_B"] {
        let server = Server::from_env(var);
        let name = server.connect().await.server_name().await;
        println!("{}: {name:?}", server.site.host);
        assert!(name.is_some(), "{} gave no name", server.site.host);
    }
}

/// A symbolic link is listed as what it leads to: a folder link as a folder, a file link
/// as a file. The script makes `links/` on each server: `real-folder/`, `real-file`, and
/// `to-folder` and `to-file` linking to them. FTP lists twice: the second listing skips
/// what the first found the server does not support (vsftpd has neither MLSD nor MLST).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn links_list_as_what_they_lead_to() {
    for var in ["SHUTTLESCP_TEST_SFTP", "SHUTTLESCP_TEST_FTP"] {
        let server = Server::from_env(var);
        let remote = server.connect().await;
        let dir = server.dir.join("links");
        for listing in 1..=2 {
            let entries = remote.list_dir(&dir).await.expect("list the links folder");
            let kind = |name: &str| {
                entries
                    .iter()
                    .find(|entry| entry.name() == name)
                    .map(|entry| entry.kind())
                    .unwrap_or_else(|| panic!("{var}: {name} missing from listing {listing}"))
            };
            assert_eq!(kind("real-folder"), EntryKind::Directory, "{var}");
            assert_eq!(
                kind("to-folder"),
                EntryKind::Directory,
                "{var}, listing {listing}"
            );
            assert_eq!(kind("real-file"), EntryKind::File, "{var}");
            assert_eq!(kind("to-file"), EntryKind::File, "{var}, listing {listing}");
        }
    }
}

/// A site set to more files at once than its FTP server allows connections: the server
/// refuses the extra ones, and every file still arrives over those it allowed. The
/// script's `SHUTTLESCP_TEST_FTP_LIMITED` server takes 3 connections per client.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn more_files_at_once_than_the_ftp_server_allows() {
    // Arrange: 20 files, and a site asking for 8 at once.
    let mut server = Server::from_env("SHUTTLESCP_TEST_FTP_LIMITED");
    server.site.files_at_once = 8;
    let remote = server.connect().await;
    let temp = tempfile::tempdir().expect("temp dir");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let name = format!("limited-{stamp}");
    let folder = temp.path().join(&name);
    std::fs::create_dir(&folder).expect("create");
    for index in 0..20 {
        std::fs::write(
            folder.join(format!("{index:02}.bin")),
            pattern(50_000 + index),
        )
        .expect("write");
    }
    let on_server = SidePath::Remote(server.dir.join(&name));

    // Act: up, then back down.
    let upload = job(
        JobKind::Copy,
        Side::Local,
        vec![SidePath::Local(folder.clone())],
        SidePath::Remote(server.dir.clone()),
    );
    let (up, up_errors) = run_job(upload, &remote, None).await;
    let back = temp.path().join("back");
    std::fs::create_dir(&back).expect("create");
    let download = job(
        JobKind::Copy,
        Side::Remote,
        vec![on_server.clone()],
        SidePath::Local(back.clone()),
    );
    let (down, down_errors) = run_job(download, &remote, None).await;
    let clean_up = job(
        JobKind::Delete,
        Side::Remote,
        vec![on_server.clone()],
        on_server,
    );
    let (deleted, _) = run_job(clean_up, &remote, None).await;

    // Assert: nothing failed either way, and every file came back as it went.
    assert!(up.is_complete(), "upload: {up:?} {up_errors:?}");
    assert!(down.is_complete(), "download: {down:?} {down_errors:?}");
    assert!(deleted.is_complete(), "clean up: {deleted:?}");
    for index in 0..20 {
        let file = format!("{index:02}.bin");
        assert_eq!(
            std::fs::read(back.join(&name).join(&file)).expect("read the copy"),
            pattern(50_000 + index),
            "{file} differs"
        );
    }
}

/// A stand-in for the network between ShuttleSCP and a test server: a local port that
/// passes each connection on to the server. It can slow down what comes back, and cut
/// every connection at once, as a dropped line would.
#[derive(Clone)]
struct TestLink {
    port: u16,
    /// The tasks passing the bytes along; ending them closes both ends.
    tasks: Arc<Mutex<tokio::task::JoinSet<()>>>,
}

impl TestLink {
    /// Start passing connections on to `server`, what comes back at about
    /// `bytes_per_second` (`None`: as fast as it comes).
    async fn start(server: &Server, bytes_per_second: Option<usize>) -> Self {
        let target = format!("{}:{}", server.site.host, server.site.port);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listen");
        let port = listener.local_addr().expect("address").port();
        let tasks = Arc::new(Mutex::new(tokio::task::JoinSet::new()));
        let spawn_in = Arc::clone(&tasks);
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let server = tokio::net::TcpStream::connect(&target)
                    .await
                    .expect("connect to the server");
                let (mut from_client, mut to_client) = client.into_split();
                let (mut from_server, mut to_server) = server.into_split();
                let mut tasks = spawn_in.lock().expect("lock");
                tasks.spawn(async move {
                    let _ = tokio::io::copy(&mut from_client, &mut to_server).await;
                });
                tasks.spawn(async move {
                    let Some(bytes_per_second) = bytes_per_second else {
                        let _ = tokio::io::copy(&mut from_server, &mut to_client).await;
                        return;
                    };
                    // A tenth of a second's worth, every tenth of a second.
                    let mut buffer = vec![0; bytes_per_second / 10];
                    while let Ok(read @ 1..) = from_server.read(&mut buffer).await {
                        if to_client.write_all(&buffer[..read]).await.is_err() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                });
            }
        });
        Self { port, tasks }
    }

    /// Cut every connection made so far. New ones still get through.
    fn cut(&self) {
        self.tasks.lock().expect("lock").abort_all();
    }

    /// `server`, reached through this link.
    fn to(&self, server: &Server) -> Server {
        let mut site = server.site.clone();
        site.host = "127.0.0.1".to_owned();
        site.port = self.port;
        Server {
            site,
            password: server.password.clone(),
            dir: server.dir.clone(),
        }
    }
}

/// Browsing an SFTP server stays quick while a slow download runs: transfers have a
/// connection of their own. On one connection, a listing waits behind the download's
/// queued reads, about 20 s at 200 kB/s.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn sftp_browsing_does_not_wait_for_a_download() {
    // Arrange: a 4 MiB file on the server, downloading through a 200 kB/s link.
    const SIZE: usize = 4 * 1024 * 1024;
    let server = Server::from_env("SHUTTLESCP_TEST_SFTP");
    let remote = server.connect().await;
    let temp = tempfile::tempdir().expect("temp dir");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let name = format!("browse-{stamp}.bin");
    std::fs::write(temp.path().join(&name), pattern(SIZE)).expect("write");
    let upload = job(
        JobKind::Copy,
        Side::Local,
        vec![SidePath::Local(temp.path().join(&name))],
        SidePath::Remote(server.dir.clone()),
    );
    let (up, up_errors) = run_job(upload, &remote, None).await;
    assert!(up.is_complete(), "upload: {up:?} {up_errors:?}");
    let link = TestLink::start(&server, Some(200_000)).await;
    let browsing = link.to(&server).connect().await;
    let back = temp.path().join("back");
    std::fs::create_dir(&back).expect("create");
    let on_server = SidePath::Remote(server.dir.join(&name));
    let download = job(
        JobKind::Copy,
        Side::Remote,
        vec![on_server.clone()],
        SidePath::Local(back.clone()),
    );
    let cancel = CancelFlag::default();
    let transfers = browsing.for_transfers().await;
    let downloading = tokio::spawn(run(download, Some(transfers), None, cancel.clone(), |_| {}));
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Act: list a folder while the download runs.
    let started = std::time::Instant::now();
    let listing = browsing.list_dir(&server.dir).await;
    let took = started.elapsed();

    // Clean up: stop the download, delete the file.
    cancel.cancel();
    let _ = downloading.await;
    let clean_up = job(
        JobKind::Delete,
        Side::Remote,
        vec![on_server.clone()],
        on_server,
    );
    let (deleted, _) = run_job(clean_up, &remote, None).await;

    // Assert
    listing.expect("list while downloading");
    assert!(took < Duration::from_secs(5), "the listing took {took:?}");
    assert!(deleted.is_complete(), "clean up: {deleted:?}");
}

/// A download whose connection is lost partway continues from its partial copy on a
/// new login, and arrives whole.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn sftp_download_continues_after_losing_the_connection() {
    // Arrange: a 4 MiB file on the server, and a link at 500 kB/s (about 8 s for it)
    // that drops 2 s into the download.
    const SIZE: usize = 4 * 1024 * 1024;
    let server = Server::from_env("SHUTTLESCP_TEST_SFTP");
    let remote = server.connect().await;
    let temp = tempfile::tempdir().expect("temp dir");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let name = format!("dropped-{stamp}.bin");
    std::fs::write(temp.path().join(&name), pattern(SIZE)).expect("write");
    let upload = job(
        JobKind::Copy,
        Side::Local,
        vec![SidePath::Local(temp.path().join(&name))],
        SidePath::Remote(server.dir.clone()),
    );
    let (up, up_errors) = run_job(upload, &remote, None).await;
    assert!(up.is_complete(), "upload: {up:?} {up_errors:?}");
    let link = TestLink::start(&server, Some(500_000)).await;
    let linked = link.to(&server).connect().await;
    let dropping = link.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        dropping.cut();
    });

    // Act
    let back = temp.path().join("back");
    std::fs::create_dir(&back).expect("create");
    let on_server = SidePath::Remote(server.dir.join(&name));
    let download = job(
        JobKind::Copy,
        Side::Remote,
        vec![on_server.clone()],
        SidePath::Local(back.clone()),
    );
    let resumed_at = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&resumed_at);
    let down = run(
        download,
        Some(linked.clone()),
        None,
        CancelFlag::default(),
        move |event| {
            if let TransferEvent::Resumed { from, .. } = event {
                *seen.lock().expect("lock") = Some(from);
            }
        },
    )
    .await;
    let clean_up = job(
        JobKind::Delete,
        Side::Remote,
        vec![on_server.clone()],
        on_server,
    );
    let (deleted, _) = run_job(clean_up, &remote, None).await;

    // Assert: it continued where the connection was lost, and arrived whole.
    assert!(down.is_complete(), "download: {down:?}");
    assert!(deleted.is_complete(), "clean up: {deleted:?}");
    let resumed_at = resumed_at.lock().expect("lock").expect("it was continued");
    assert!(resumed_at > 0, "it started over");
    assert_eq!(
        std::fs::read(back.join(&name)).expect("read the copy"),
        pattern(SIZE),
        "the copy differs"
    );
}

/// After the connection to an SFTP server is lost, the next action logs in again by
/// itself, instead of failing with "session closed" until the user logs in again.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn sftp_logs_in_again_after_losing_the_connection() {
    // Arrange: a session through a test link, which then drops.
    let server = Server::from_env("SHUTTLESCP_TEST_SFTP");
    let link = TestLink::start(&server, None).await;
    let remote = link.to(&server).connect().await;
    let before = remote.list_dir(&server.dir).await.expect("list");
    link.cut();
    // Give the SSH library a moment to see the connection closed.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Act
    let after = remote.list_dir(&server.dir).await;

    // Assert: listed as before.
    let names = |entries: &[shuttlescp_core::entry::FileEntry]| {
        entries
            .iter()
            .map(|entry| entry.name().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(&after.expect("list after the connection was lost")),
        names(&before)
    );
}

/// A download from a slow SFTP server finishes. The SFTP library keeps 16 reads of up
/// to 256 kB waiting at once and gave each one 10 seconds, so any download slower than
/// about 0.4 MB/s failed with "Timeout".
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn download_from_a_slow_sftp_server() {
    // Arrange: a 4 MiB file on the server, put there at full speed.
    const SIZE: usize = 4 * 1024 * 1024;
    let server = Server::from_env("SHUTTLESCP_TEST_SFTP");
    let remote = server.connect().await;
    let temp = tempfile::tempdir().expect("temp dir");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let name = format!("slow-{stamp}.bin");
    std::fs::write(temp.path().join(&name), pattern(SIZE)).expect("write");
    let upload = job(
        JobKind::Copy,
        Side::Local,
        vec![SidePath::Local(temp.path().join(&name))],
        SidePath::Remote(server.dir.clone()),
    );
    let (up, up_errors) = run_job(upload, &remote, None).await;
    assert!(up.is_complete(), "upload: {up:?} {up_errors:?}");
    let slow_link = TestLink::start(&server, Some(200_000)).await;
    let slow_remote = slow_link.to(&server).connect().await;

    // Act: download it through the slow link.
    let back = temp.path().join("back");
    std::fs::create_dir(&back).expect("create");
    let on_server = SidePath::Remote(server.dir.join(&name));
    let download = job(
        JobKind::Copy,
        Side::Remote,
        vec![on_server.clone()],
        SidePath::Local(back.clone()),
    );
    let (down, down_errors) = run_job(download, &slow_remote, None).await;
    let clean_up = job(
        JobKind::Delete,
        Side::Remote,
        vec![on_server.clone()],
        on_server,
    );
    let (deleted, _) = run_job(clean_up, &remote, None).await;

    // Assert: it arrived whole.
    assert!(down.is_complete(), "download: {down:?} {down_errors:?}");
    assert!(deleted.is_complete(), "clean up: {deleted:?}");
    assert_eq!(
        std::fs::read(back.join(&name)).expect("read the copy"),
        pattern(SIZE),
        "the copy differs"
    );
}

/// An SSH server whose account has a shell computes a file's checksum with
/// `sha256sum`, the same as this computer's, whatever the file's name.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn a_server_with_a_shell_computes_checksums() {
    // Arrange: a file with an awkward name on the server.
    let server = Server::from_env("SHUTTLESCP_TEST_SSH_SHELL");
    let remote = server.connect().await;
    let temp = tempfile::tempdir().expect("temp dir");
    let name = "it's $(id) file.bin";
    let local = temp.path().join(name);
    std::fs::write(&local, pattern(300_000)).expect("write");
    let upload = job(
        JobKind::Copy,
        Side::Local,
        vec![SidePath::Local(local.clone())],
        SidePath::Remote(server.dir.clone()),
    );
    let (up, up_errors) = run_job(upload, &remote, None).await;
    assert!(up.is_complete(), "upload: {up:?} {up_errors:?}");

    // Act
    let on_server = remote.sha256(&server.dir.join(name)).await;
    let missing = remote.sha256(&server.dir.join("missing.bin")).await;

    // Assert
    let here = shuttlescp_core::checksum::local_sha256(local)
        .await
        .expect("local checksum");
    assert_eq!(on_server.expect("server checksum"), here);
    assert!(
        missing.is_err(),
        "a missing file has no checksum: {missing:?}"
    );
}

/// Servers that cannot compute checksums (an SFTP-only account, vsftpd) say so, and
/// soon, rather than hang.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn servers_without_checksums_say_so() {
    for var in ["SHUTTLESCP_TEST_SFTP", "SHUTTLESCP_TEST_FTP"] {
        // Arrange
        let server = Server::from_env(var);
        let remote = server.connect().await;
        let file = server.dir.join("links/real-file");

        // Act: twice (FTP remembers the first refusal).
        let asking = async { (remote.sha256(&file).await, remote.sha256(&file).await) };
        let answers = tokio::time::timeout(Duration::from_secs(10), asking).await;

        // Assert
        let (first, second) = answers.unwrap_or_else(|_| panic!("{var}: no answer in 10 s"));
        assert!(first.is_err(), "{var}: {first:?}");
        assert!(second.is_err(), "{var}: {second:?}");
    }
}

/// Downloads checked against their source: matched where the server can compute
/// checksums, left unchecked (but done) where it cannot.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn downloads_are_verified_where_the_server_can() {
    for (var, can) in [
        ("SHUTTLESCP_TEST_SSH_SHELL", true),
        ("SHUTTLESCP_TEST_SFTP", false),
    ] {
        // Arrange: a file on the server.
        let server = Server::from_env(var);
        let remote = server.connect().await;
        let temp = tempfile::tempdir().expect("temp dir");
        let name = "verified.bin";
        std::fs::write(temp.path().join(name), pattern(700_000)).expect("write");
        let upload = job(
            JobKind::Copy,
            Side::Local,
            vec![SidePath::Local(temp.path().join(name))],
            SidePath::Remote(server.dir.clone()),
        );
        let (up, up_errors) = run_job(upload, &remote, None).await;
        assert!(up.is_complete(), "{var} upload: {up:?} {up_errors:?}");
        let back = temp.path().join("back");
        std::fs::create_dir(&back).expect("create");
        let on_server = SidePath::Remote(server.dir.join(name));
        let download = Job {
            verify: Verify {
                enabled: true,
                read_back_up_to: None,
            },
            ..job(
                JobKind::Copy,
                Side::Remote,
                vec![on_server.clone()],
                SidePath::Local(back.clone()),
            )
        };

        // Act
        let found = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&found);
        let down = run(
            download,
            Some(remote.clone()),
            None,
            CancelFlag::default(),
            move |event| {
                if let TransferEvent::Verified { verification, .. } = event {
                    seen.lock().expect("lock").push(verification);
                }
            },
        )
        .await;
        let clean_up = job(
            JobKind::Delete,
            Side::Remote,
            vec![on_server.clone()],
            on_server,
        );
        let (deleted, _) = run_job(clean_up, &remote, None).await;

        // Assert
        assert!(down.is_complete(), "{var} download: {down:?}");
        assert!(deleted.is_complete(), "{var} clean up: {deleted:?}");
        let expected = shuttlescp_core::checksum::local_sha256(temp.path().join(name))
            .await
            .expect("checksum");
        let found = found.lock().expect("lock").clone();
        match found.as_slice() {
            [Verification::Matched(checksum)] if can => assert_eq!(*checksum, expected),
            [Verification::Unchecked(_)] if !can => {}
            other => panic!("{var}: {other:?}"),
        }
    }
}

/// Run `job` checking its copies as `verify` says; how the job ended, and what each
/// check found.
async fn run_verifying(
    job: Job,
    verify: Verify,
    remote: &Remote,
) -> (JobSummary, Vec<Verification>) {
    let found = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&found);
    let job = Job { verify, ..job };
    let summary = run(
        job,
        Some(remote.clone()),
        None,
        CancelFlag::default(),
        move |event| {
            if let TransferEvent::Verified { verification, .. } = event {
                seen.lock().expect("lock").push(verification);
            }
        },
    )
    .await;
    let found = found.lock().expect("lock").clone();
    (summary, found)
}

/// A server that cannot compute checksums (the SFTP-only account): copies up to the
/// read-back limit are checked by reading them back; larger ones, or with reading back
/// off, are left unchecked.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs test servers; see scripts/test-site-to-site.sh"]
async fn copies_are_read_back_up_to_the_limit() {
    // Arrange: a 700 kB file here.
    const SIZE: usize = 700_000;
    let server = Server::from_env("SHUTTLESCP_TEST_SFTP");
    let remote = server.connect().await;
    let temp = tempfile::tempdir().expect("temp dir");
    let name = "read-back.bin";
    std::fs::write(temp.path().join(name), pattern(SIZE)).expect("write");
    let expected = shuttlescp_core::checksum::local_sha256(temp.path().join(name))
        .await
        .expect("checksum");
    let verify = |read_back_up_to| Verify {
        enabled: true,
        read_back_up_to,
    };
    let on_server = SidePath::Remote(server.dir.join(name));
    let upload = job(
        JobKind::Copy,
        Side::Local,
        vec![SidePath::Local(temp.path().join(name))],
        SidePath::Remote(server.dir.clone()),
    );
    let download = |into: &str| {
        let back = temp.path().join(into);
        std::fs::create_dir(&back).expect("create");
        job(
            JobKind::Copy,
            Side::Remote,
            vec![on_server.clone()],
            SidePath::Local(back),
        )
    };

    // Act
    let up = run_verifying(upload, verify(Some(1_000_000)), &remote).await;
    let read_back = run_verifying(download("a"), verify(Some(1_000_000)), &remote).await;
    let too_large = run_verifying(download("b"), verify(Some(100_000)), &remote).await;
    let never = run_verifying(download("c"), verify(None), &remote).await;
    let clean_up = job(
        JobKind::Delete,
        Side::Remote,
        vec![on_server.clone()],
        on_server,
    );
    let (deleted, _) = run_job(clean_up, &remote, None).await;

    // Assert: all done; checked as the limit says.
    for (what, (summary, _)) in [
        ("up", &up),
        ("read back", &read_back),
        ("too large", &too_large),
        ("never", &never),
    ] {
        assert!(summary.is_complete(), "{what}: {summary:?}");
    }
    assert!(deleted.is_complete(), "clean up: {deleted:?}");
    assert_eq!(up.1, vec![Verification::Matched(expected)], "upload");
    assert_eq!(
        read_back.1,
        vec![Verification::Matched(expected)],
        "download"
    );
    assert_eq!(too_large.1, vec![Verification::LargeFile], "over the limit");
    assert!(
        matches!(never.1.as_slice(), [Verification::Unchecked(_)]),
        "never: {:?}",
        never.1
    );
}
