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
use std::time::{SystemTime, UNIX_EPOCH};

use secrecy::SecretString;
use shuttlescp_core::path::{RemotePath, SidePath};
use shuttlescp_core::remote::{ConnectError, Credentials, Remote, connect};
use shuttlescp_core::sites::{FtpOptions, Protocol, Site, SshAuth};
use std::sync::{Arc, Mutex};

use shuttlescp_core::transfer::{
    CancelFlag, ItemOutcome, Job, JobKind, JobSummary, Side, TransferEvent, run,
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
