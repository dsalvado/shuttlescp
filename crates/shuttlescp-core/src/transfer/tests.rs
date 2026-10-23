//! Transfer engine tests, run between two local folders (the engine treats both sides
//! the same way; the server side was exercised against a real server).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::path::{RemotePath, SidePath};

/// Build a source tree: `a/one.txt`, `a/sub/two.txt`, `top.txt` (600 kB).
fn make_tree(root: &Path) {
    std::fs::create_dir_all(root.join("a/sub")).expect("create folders");
    std::fs::write(root.join("a/one.txt"), "one").expect("write");
    std::fs::write(root.join("a/sub/two.txt"), "two!").expect("write");
    std::fs::write(root.join("top.txt"), big_content()).expect("write");
}

/// Contents of `top.txt`: varied bytes, so a misplaced chunk would be noticed.
fn big_content() -> Vec<u8> {
    (0..600_000_u32).map(|i| (i % 251) as u8).collect()
}

/// A source and an empty destination folder in a fresh temporary folder.
fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("temp dir");
    let (src, dst) = (temp.path().join("src"), temp.path().join("dst"));
    make_tree(&src);
    std::fs::create_dir(&dst).expect("dst");
    (temp, src, dst)
}

/// A local path.
fn local(path: &Path) -> SidePath {
    SidePath::Local(path.to_path_buf())
}

/// A path on the server.
fn remote(path: &str) -> SidePath {
    SidePath::Remote(RemotePath::new(path))
}

/// A job whose sources and destination are all local folders, as the engine tests run.
fn job(kind: JobKind, sources: Vec<PathBuf>, dest_dir: &Path) -> Job {
    Job {
        kind,
        side: Side::Local,
        sources: sources.into_iter().map(SidePath::Local).collect(),
        dest_dir: local(dest_dir),
        same_side: false,
        between_servers: false,
        left_server: false,
        resume: false,
    }
}

/// A job that copies or moves within one side (e.g. a drag onto a folder in the same
/// pane).
fn same_side_job(kind: JobKind, sources: Vec<PathBuf>, dest_dir: &Path) -> Job {
    Job {
        same_side: true,
        between_servers: false,
        left_server: false,
        ..job(kind, sources, dest_dir)
    }
}

/// Run a job between two local folders, answering conflicts with `answer`, and collect
/// its events.
async fn run_local_answering(
    job: Job,
    answer: Option<ConflictAnswer>,
) -> (JobSummary, Vec<TransferEvent>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink: Sink = {
        let events = Arc::clone(&events);
        Arc::new(move |event: TransferEvent| {
            if let (TransferEvent::Conflict { reply, .. }, Some(answer)) = (&event, answer) {
                reply.answer(answer);
            }
            events.lock().expect("lock").push(event);
        })
    };
    let summary = execute(
        &job,
        Endpoint::Local,
        Some(Endpoint::Local),
        &CancelFlag::default(),
        &sink,
    )
    .await;
    let events = events.lock().expect("lock").clone();
    (summary, events)
}

async fn run_local(job: Job) -> (JobSummary, Vec<TransferEvent>) {
    run_local_answering(job, None).await
}

fn conflicts(events: &[TransferEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, TransferEvent::Conflict { .. }))
        .count()
}

fn answer(choice: ConflictChoice, apply_to_all: bool) -> Option<ConflictAnswer> {
    Some(ConflictAnswer {
        choice,
        apply_to_all,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn copy_reproduces_the_tree_and_reports_every_item() {
    // Arrange
    let (_temp, src, dst) = setup();

    // Act
    let (summary, events) = run_local(job(
        JobKind::Copy,
        vec![src.join("a"), src.join("top.txt")],
        &dst,
    ))
    .await;

    // Assert: 5 items (a, top.txt, one.txt, sub, two.txt), all copied, sources kept.
    assert_eq!(summary.done, 5);
    assert_eq!(summary.bytes, 600_007, "the three files' sizes");
    assert!(summary.is_complete());
    assert_eq!(
        std::fs::read_to_string(dst.join("a/sub/two.txt")).expect("read"),
        "two!"
    );
    assert_eq!(
        std::fs::read(dst.join("top.txt")).expect("read"),
        big_content()
    );
    assert!(src.join("a/sub/two.txt").exists());
    assert!(
        !dst.join("top.txt.filepart").exists(),
        "partial file renamed"
    );
    let TransferEvent::Planned(items) = &events[0] else {
        panic!("first event must be the plan");
    };
    assert_eq!(items.len(), 5);
    let folder_a = items.iter().find(|item| item.name == "a").expect("a");
    assert_eq!(folder_a.size, 7, "folder size is the total of its files");
    assert_eq!(conflicts(&events), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn copy_keeps_the_modification_time() {
    let (_temp, src, dst) = setup();
    let old = UNIX_EPOCH + Duration::from_secs(1_500_000_000);
    std::fs::File::options()
        .write(true)
        .open(src.join("top.txt"))
        .and_then(|file| file.set_modified(old))
        .expect("set time");

    run_local(job(JobKind::Copy, vec![src.join("top.txt")], &dst)).await;

    let copied = std::fs::metadata(dst.join("top.txt")).and_then(|meta| meta.modified());
    assert_eq!(copied.expect("time"), old);
}

#[tokio::test(flavor = "multi_thread")]
async fn existing_file_is_asked_about_and_skip_keeps_it() {
    let (_temp, src, dst) = setup();
    std::fs::write(dst.join("top.txt"), "mine").expect("write");

    let (summary, events) = run_local_answering(
        job(JobKind::Copy, vec![src.join("top.txt")], &dst),
        answer(ConflictChoice::Skip, false),
    )
    .await;

    assert_eq!(conflicts(&events), 1);
    assert_eq!(summary.skipped, 1);
    assert!(summary.is_complete());
    assert_eq!(
        std::fs::read_to_string(dst.join("top.txt")).expect("read"),
        "mine"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn overwrite_for_all_asks_once_and_replaces_every_file() {
    // Arrange: both files of folder `a` already exist at the destination.
    let (_temp, src, dst) = setup();
    std::fs::create_dir_all(dst.join("a/sub")).expect("folders");
    std::fs::write(dst.join("a/one.txt"), "old").expect("write");
    std::fs::write(dst.join("a/sub/two.txt"), "old").expect("write");

    // Act
    let (summary, events) = run_local_answering(
        job(JobKind::Copy, vec![src.join("a")], &dst),
        answer(ConflictChoice::Overwrite, true),
    )
    .await;

    // Assert
    assert_eq!(conflicts(&events), 1, "apply to all answers the rest");
    assert_eq!(summary.done, 4);
    assert_eq!(
        std::fs::read_to_string(dst.join("a/one.txt")).expect("read"),
        "one"
    );
    assert_eq!(
        std::fs::read_to_string(dst.join("a/sub/two.txt")).expect("read"),
        "two!"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_at_a_conflict_stops_the_job() {
    let (_temp, src, dst) = setup();
    std::fs::write(dst.join("top.txt"), "mine").expect("write");

    let (summary, _) = run_local_answering(
        job(JobKind::Copy, vec![src.join("top.txt")], &dst),
        answer(ConflictChoice::CancelJob, false),
    )
    .await;

    assert_eq!(summary.cancelled, 1);
    assert_eq!(
        std::fs::read_to_string(dst.join("top.txt")).expect("read"),
        "mine"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn partial_copy_is_continued_where_it_stopped() {
    // Arrange: an interrupted copy left the first 300 kB in top.txt.filepart.
    let (_temp, src, dst) = setup();
    std::fs::write(dst.join("top.txt.filepart"), &big_content()[..300_000]).expect("write");

    // Act
    let (summary, events) = run_local(job(JobKind::Copy, vec![src.join("top.txt")], &dst)).await;

    // Assert: the copy is whole, and it started at 300 kB rather than 0.
    assert!(summary.is_complete());
    assert_eq!(
        std::fs::read(dst.join("top.txt")).expect("read"),
        big_content()
    );
    assert!(!dst.join("top.txt.filepart").exists());
    let resumed_from = events.iter().find_map(|event| match event {
        TransferEvent::Resumed { from, .. } => Some(*from),
        _ => None,
    });
    assert_eq!(resumed_from, Some(300_000));
}

#[tokio::test(flavor = "multi_thread")]
async fn partial_copy_that_does_not_match_is_started_over() {
    let (_temp, src, dst) = setup();
    std::fs::write(dst.join("top.txt.filepart"), vec![0_u8; 300_000]).expect("write");

    let (summary, _) = run_local(job(JobKind::Copy, vec![src.join("top.txt")], &dst)).await;

    assert!(summary.is_complete());
    assert_eq!(
        std::fs::read(dst.join("top.txt")).expect("read"),
        big_content()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn resumed_move_skips_finished_files_and_missing_sources() {
    // Arrange: a move of `a` and `top.txt` was interrupted after top.txt was moved and
    // a/one.txt was copied (but its source not yet deleted).
    let (_temp, src, dst) = setup();
    run_local(job(JobKind::Copy, vec![src.join("top.txt")], &dst)).await;
    std::fs::remove_file(src.join("top.txt")).expect("moved away");
    std::fs::create_dir(dst.join("a")).expect("folder");
    run_local(job(
        JobKind::Copy,
        vec![src.join("a/one.txt")],
        &dst.join("a"),
    ))
    .await;

    // Act
    let mut resumed = job(
        JobKind::Move,
        vec![src.join("a"), src.join("top.txt")],
        &dst,
    );
    resumed.resume = true;
    let (summary, events) = run_local(resumed).await;

    // Assert: no questions about the already-copied file; everything ends up moved.
    assert_eq!(conflicts(&events), 0);
    assert!(summary.is_complete(), "{summary:?}");
    assert!(!src.join("a").exists());
    assert_eq!(
        std::fs::read_to_string(dst.join("a/one.txt")).expect("read"),
        "one"
    );
    assert_eq!(
        std::fs::read_to_string(dst.join("a/sub/two.txt")).expect("read"),
        "two!"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn move_deletes_sources_only_after_copying() {
    let (_temp, src, dst) = setup();

    let (summary, _) = run_local(job(JobKind::Move, vec![src.join("a")], &dst)).await;

    assert_eq!(summary.done, 4);
    assert!(!src.join("a").exists(), "moved folder is gone");
    assert_eq!(
        std::fs::read_to_string(dst.join("a/one.txt")).expect("read"),
        "one"
    );
    assert!(src.join("top.txt").exists(), "unselected file untouched");
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_copy_keeps_the_source_of_a_move() {
    let temp = tempfile::tempdir().expect("temp dir");
    let src = temp.path().join("src");
    make_tree(&src);

    // The destination folder does not exist, so the copy fails.
    let (summary, _) = run_local(job(
        JobKind::Move,
        vec![src.join("top.txt")],
        &temp.path().join("missing"),
    ))
    .await;

    assert_eq!(summary.failed, 1);
    assert!(src.join("top.txt").exists());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn delete_removes_a_link_but_not_what_it_points_to() {
    // Arrange: doomed/link -> keep (a folder outside what is deleted).
    let temp = tempfile::tempdir().expect("temp dir");
    let (doomed, keep) = (temp.path().join("doomed"), temp.path().join("keep"));
    std::fs::create_dir_all(&doomed).expect("doomed");
    std::fs::create_dir_all(&keep).expect("keep");
    std::fs::write(keep.join("precious.txt"), "x").expect("write");
    std::os::unix::fs::symlink(&keep, doomed.join("link")).expect("symlink");

    // Act
    let (summary, _) = run_local(job(JobKind::Delete, vec![doomed.clone()], temp.path())).await;

    // Assert
    assert_eq!(summary.done, 2);
    assert!(!doomed.exists());
    assert!(keep.join("precious.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelled_job_copies_nothing() {
    let (_temp, src, dst) = setup();
    let cancel = CancelFlag::default();
    cancel.cancel();
    let sink: Sink = Arc::new(|_| {});

    let summary = execute(
        &job(JobKind::Copy, vec![src.join("top.txt")], &dst),
        Endpoint::Local,
        Some(Endpoint::Local),
        &cancel,
        &sink,
    )
    .await;

    assert_eq!(summary.cancelled, 1);
    assert!(!dst.join("top.txt").exists());
    assert!(!dst.join("top.txt.filepart").exists());
}

/// Run a job between two local folders, stopping the items named in `stop` as soon as
/// the job is planned (as the user would from the transfer list).
async fn run_local_stopping(job: Job, stop: &'static [&'static str]) -> JobSummary {
    let cancel = CancelFlag::default();
    let sink: Sink = {
        let cancel = cancel.clone();
        Arc::new(move |event: TransferEvent| {
            if let TransferEvent::Planned(items) = event {
                for item in items
                    .iter()
                    .filter(|item| stop.contains(&item.name.as_str()))
                {
                    cancel.stop_item(item.id);
                }
            }
        })
    };
    execute(&job, Endpoint::Local, Some(Endpoint::Local), &cancel, &sink).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_file_is_left_out_and_the_rest_is_copied() {
    let (_temp, src, dst) = setup();

    let summary = run_local_stopping(
        job(
            JobKind::Copy,
            vec![src.join("a"), src.join("top.txt")],
            &dst,
        ),
        &["top.txt"],
    )
    .await;

    // a, one.txt, sub, two.txt copied; top.txt stopped, with no partial copy left.
    assert_eq!((summary.done, summary.stopped), (4, 1));
    assert!(summary.is_complete(), "stopping a file is not a failure");
    assert!(dst.join("a/sub/two.txt").exists());
    assert!(!dst.join("top.txt").exists());
    assert!(!dst.join("top.txt.filepart").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn moving_keeps_a_stopped_file_and_its_folder() {
    let (_temp, src, dst) = setup();

    let summary =
        run_local_stopping(job(JobKind::Move, vec![src.join("a")], &dst), &["one.txt"]).await;

    // one.txt stays, so a stays too (stopped); sub and two.txt moved.
    assert_eq!((summary.done, summary.stopped), (2, 2));
    assert!(src.join("a/one.txt").exists());
    assert!(!src.join("a/sub").exists());
    assert!(dst.join("a/sub/two.txt").exists());
    assert!(!dst.join("a/one.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_leaves_a_stopped_file_and_its_folder() {
    let (_temp, src, _dst) = setup();

    let summary = run_local_stopping(
        job(JobKind::Delete, vec![src.join("a")], &src),
        &["two.txt"],
    )
    .await;

    // two.txt stays, so sub and a stay (stopped); one.txt deleted.
    assert_eq!((summary.done, summary.stopped), (1, 3));
    assert!(src.join("a/sub/two.txt").exists());
    assert!(!src.join("a/one.txt").exists());
}

/// Copy top.txt over an interrupted copy (its first 300 kB), stopping the job with
/// `stop` the moment the copy continues; returns what the partial file holds after.
async fn stop_while_resuming(stop: fn(&CancelFlag)) -> (JobSummary, Option<u64>) {
    let (_temp, src, dst) = setup();
    let partial = dst.join("top.txt.filepart");
    std::fs::write(&partial, &big_content()[..300_000]).expect("write");
    let cancel = CancelFlag::default();
    let sink: Sink = {
        let cancel = cancel.clone();
        Arc::new(move |event: TransferEvent| {
            if matches!(event, TransferEvent::Resumed { .. }) {
                stop(&cancel);
            }
        })
    };
    let job = job(JobKind::Copy, vec![src.join("top.txt")], &dst);
    let summary = execute(&job, Endpoint::Local, Some(Endpoint::Local), &cancel, &sink).await;
    let kept = std::fs::metadata(&partial)
        .ok()
        .map(|metadata| metadata.len());
    (summary, kept)
}

#[tokio::test(flavor = "multi_thread")]
async fn pausing_keeps_the_partial_copy_and_cancelling_removes_it() {
    let (paused, kept) = stop_while_resuming(CancelFlag::pause).await;
    assert_eq!(paused.cancelled, 1);
    assert_eq!(kept, Some(300_000), "kept for resuming");

    let (cancelled, kept) = stop_while_resuming(CancelFlag::cancel).await;
    assert_eq!(cancelled.cancelled, 1);
    assert_eq!(kept, None, "removed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_file_waits_then_continues_from_its_partial_copy() {
    // Arrange: an interrupted copy left top.txt's first 300 kB; the job pauses the file
    // as soon as it continues, and resumes it a little later.
    let (_temp, src, dst) = setup();
    std::fs::write(dst.join("top.txt.filepart"), &big_content()[..300_000]).expect("write");
    let cancel = CancelFlag::default();
    let resumes = Arc::new(Mutex::new(Vec::new()));
    let sink: Sink = {
        let (cancel, resumes) = (cancel.clone(), Arc::clone(&resumes));
        Arc::new(move |event: TransferEvent| {
            if let TransferEvent::Resumed { id, from } = event {
                resumes.lock().expect("lock").push(from);
                if !cancel.is_item_paused(id) && resumes.lock().expect("lock").len() == 1 {
                    cancel.pause_item(id);
                    let cancel = cancel.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(300));
                        cancel.resume_item(id);
                    });
                }
            }
        })
    };

    // Act
    let summary = execute(
        &job(
            JobKind::Copy,
            vec![src.join("a"), src.join("top.txt")],
            &dst,
        ),
        Endpoint::Local,
        Some(Endpoint::Local),
        &cancel,
        &sink,
    )
    .await;

    // Assert: everything copied; top.txt continued twice from its partial copy (it was
    // paused before writing more), never from the start.
    assert!(summary.is_complete());
    assert_eq!(summary.done, 5);
    assert_eq!(
        std::fs::read(dst.join("top.txt")).expect("read"),
        big_content()
    );
    assert_eq!(*resumes.lock().expect("lock"), vec![300_000, 300_000]);
}

#[tokio::test(flavor = "multi_thread")]
async fn files_of_every_size_copy_whole() {
    // Sizes around the 256 KiB chunk: empty, one byte, exactly one and two chunks, and
    // one byte either side, so every way a file can end is copied.
    const CHUNK: usize = 256 * 1024;
    let sizes = [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK, 2 * CHUNK + 7];
    let (_temp, src, dst) = setup();
    let mut sources = Vec::new();
    for size in sizes {
        let path = src.join(format!("f{size}"));
        std::fs::write(
            &path,
            (0..size).map(|i| (i % 253) as u8).collect::<Vec<_>>(),
        )
        .expect("write");
        sources.push(path);
    }

    let (summary, _) = run_local(job(JobKind::Copy, sources, &dst)).await;

    assert!(summary.is_complete(), "{summary:?}");
    for size in sizes {
        let copy = std::fs::read(dst.join(format!("f{size}"))).expect("read");
        assert_eq!(copy.len(), size, "size {size}");
        assert!(copy.iter().enumerate().all(|(i, &b)| b == (i % 253) as u8));
    }
}

#[test]
fn site_to_site_jobs_go_to_the_server_and_say_so() {
    let job = Job {
        kind: JobKind::Copy,
        side: Side::Remote,
        sources: vec![remote("/srv/a/report.pdf")],
        dest_dir: remote("/home/b"),
        same_side: false,
        between_servers: true,
        left_server: false,
        resume: false,
    };
    assert_eq!(job.dest_side(), Side::Remote);
    assert!(job.uses_server());
    assert_eq!(
        job.title(),
        "Copy report.pdf to /home/b on the other server"
    );
    let moved = Job {
        kind: JobKind::Move,
        ..job.clone()
    };
    assert_eq!(
        moved.title(),
        "Move report.pdf to /home/b on the other server"
    );

    // Saved and read back with both ends as server paths.
    let json = serde_json::to_string(&job).expect("save");
    let back: Job = serde_json::from_str(&json).expect("load");
    assert_eq!(back, job);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_site_to_site_job_without_its_destination_does_not_start() {
    let job = Job {
        kind: JobKind::Copy,
        side: Side::Remote,
        sources: vec![remote("/srv/a/report.pdf")],
        dest_dir: remote("/home/b"),
        same_side: false,
        between_servers: true,
        left_server: false,
        resume: false,
    };
    let summary = run(job, None, None, CancelFlag::default(), |_| {}).await;
    assert_eq!(summary.error.as_deref(), Some("not connected to a server"));
}

#[tokio::test(flavor = "multi_thread")]
async fn giving_up_a_job_deletes_its_partial_files_only() {
    // Arrange: an interrupted copy of the tree left top.txt.filepart; one.txt finished.
    let (_temp, src, dst) = setup();
    std::fs::write(dst.join("top.txt.filepart"), &big_content()[..300_000]).expect("write");
    std::fs::create_dir(dst.join("a")).expect("create");
    std::fs::write(dst.join("a/one.txt"), "one").expect("write");
    // Both folders are local here, so the job copies within one side.
    let job = same_side_job(
        JobKind::Copy,
        vec![src.join("a"), src.join("top.txt")],
        &dst,
    );

    // Act
    let removed = discard_partials(job, None, None).await.expect("discard");

    // Assert: the partial copy is gone; finished files and the sources stay.
    assert_eq!(removed, 1);
    assert!(!dst.join("top.txt.filepart").exists());
    assert!(dst.join("a/one.txt").exists());
    assert!(src.join("top.txt").exists());
}

#[test]
fn jobs_writing_the_same_files_conflict() {
    let upload = |sources: &[&str], dest: &str| Job {
        kind: JobKind::Copy,
        side: Side::Local,
        sources: sources
            .iter()
            .map(|source| local(Path::new(source)))
            .collect(),
        dest_dir: remote(dest),
        same_side: false,
        between_servers: false,
        left_server: false,
        resume: false,
    };
    let three = upload(&["/home/me/a", "/home/me/b", "/home/me/c"], "/srv");
    // The same files again (the case that once ran twice), and one of them.
    assert!(three.conflicts_with(&upload(&["/home/me/a", "/home/me/b", "/home/me/c"], "/srv")));
    assert!(three.conflicts_with(&upload(&["/home/me/b"], "/srv")));
    // A file inside a folder another job copies.
    assert!(upload(&["/x/a"], "/srv/b").conflicts_with(&upload(&["/y/b"], "/srv")));
    // Elsewhere on the server, or a different name: no conflict.
    assert!(!three.conflicts_with(&upload(&["/home/me/a"], "/srv/other")));
    assert!(!three.conflicts_with(&upload(&["/home/me/d"], "/srv")));
    // The same names, but downloaded to this computer instead.
    let download = Job {
        kind: JobKind::Copy,
        side: Side::Remote,
        sources: vec![remote("/srv/a")],
        dest_dir: local(Path::new("/srv")),
        ..three.clone()
    };
    assert!(!three.conflicts_with(&download));
}

#[tokio::test(flavor = "multi_thread")]
async fn create_folder_and_rename_refuse_taken_names() {
    let (_temp, src, _dst) = setup();

    let made = create_folder(None, &local(&src), "new")
        .await
        .expect("create");
    assert_eq!(made, local(&src.join("new")));
    assert!(src.join("new").is_dir());
    assert!(
        create_folder(None, &local(&src), "new").await.is_err(),
        "already exists"
    );

    let renamed = rename(None, &local(&src.join("top.txt")), "renamed.txt")
        .await
        .expect("rename");
    assert_eq!(renamed, local(&src.join("renamed.txt")));
    assert!(src.join("renamed.txt").exists() && !src.join("top.txt").exists());
    let refused = rename(None, &renamed, "a").await;
    assert!(refused.is_err(), "never replaces an existing item");
    assert!(src.join("renamed.txt").exists());
}

#[test]
fn check_name_rejects_unusable_names() {
    for side in [Side::Local, Side::Remote] {
        assert!(check_name("report 2.pdf", side).is_ok());
        for bad in ["", "  ", ".", "..", "a/b", "nul\0"] {
            assert!(check_name(bad, side).is_err(), "{bad:?} should be refused");
        }
    }
    // Server names follow the server's rules, which it enforces itself.
    assert!(check_name("a:b?", Side::Remote).is_ok());
    assert_eq!(
        check_name("a:b?", Side::Local).is_err(),
        cfg!(windows),
        "local names follow Windows' rules on Windows only"
    );
}

#[test]
fn windows_names_follow_windows_rules() {
    for good in [
        "report.pdf",
        ".gitignore",
        "console.txt",
        "COM10",
        "LPT0",
        "a b",
        "café",
    ] {
        assert_eq!(windows_name_problem(good), None, "{good:?} is fine");
    }
    for bad in [
        "a\\b",
        "a:b",
        "a*",
        "a?",
        "say \"hi\"",
        "<a>",
        "a|b",
        "tab\there",
        "end.",
        "end ",
        "CON",
        "con.txt",
        "Nul",
        "aux .log",
        "COM1",
        "lpt9.doc",
    ] {
        assert!(
            windows_name_problem(bad).is_some(),
            "{bad:?} should be refused"
        );
    }
    assert_eq!(
        windows_name_problem("a:b").as_deref(),
        Some("a name cannot contain \":\"")
    );
}

#[test]
fn windows_safe_names_encode_what_windows_forbids() {
    assert_eq!(windows_safe_name("report.pdf"), "report.pdf");
    assert_eq!(windows_safe_name("notes:v2.txt"), "notes%3Av2.txt");
    assert_eq!(windows_safe_name("a<b>|c?*"), "a%3Cb%3E%7Cc%3F%2A");
    assert_eq!(windows_safe_name("say \"hi\""), "say %22hi%22");
    assert_eq!(windows_safe_name("back\\slash"), "back%5Cslash");
    assert_eq!(windows_safe_name("tab\there"), "tab%09here");
    assert_eq!(windows_safe_name("ends."), "ends%2E");
    assert_eq!(windows_safe_name("ends "), "ends%20");
    assert_eq!(windows_safe_name("con.txt"), "co%6E.txt");
    assert_eq!(windows_safe_name("COM1"), "COM%31");
    assert_eq!(windows_safe_name("aux .log"), "au%78 .log");
    // Whatever the name, the result is one Windows accepts.
    for odd in ["a:b.", "CON", "nul ", "x\u{1}y", "café: été", "...", " "] {
        let safe = windows_safe_name(odd);
        assert_eq!(windows_name_problem(&safe), None, "{odd:?} became {safe:?}");
    }
}

#[test]
fn copies_to_this_computer_get_usable_names_on_windows() {
    let dest = |dir: SidePath| ops::dest_name(std::ffi::OsStr::new("a:b"), &dir).into_owned();

    let here = dest(local(Path::new("/srv")));
    assert_eq!(here, if cfg!(windows) { "a%3Ab" } else { "a:b" });
    assert_eq!(dest(remote("/srv")), "a:b", "uploads keep their names");
}

#[test]
fn job_titles_name_the_direction() {
    let one = job(
        JobKind::Copy,
        vec![PathBuf::from("/home/me/report.pdf")],
        Path::new("/srv"),
    );
    assert_eq!(one.title(), "Upload report.pdf to /srv");
    let many = Job {
        kind: JobKind::Delete,
        side: Side::Remote,
        sources: vec![remote("/a"), remote("/b")],
        dest_dir: local(Path::new("")),
        same_side: false,
        between_servers: false,
        left_server: false,
        resume: false,
    };
    assert_eq!(many.title(), "Delete 2 items from the server");
    assert!(many.uses_server());
}

#[test]
fn same_side_jobs_stay_on_their_side() {
    let local = same_side_job(
        JobKind::Move,
        vec![PathBuf::from("/home/me/report.pdf")],
        Path::new("/home/me/old"),
    );
    assert_eq!(local.title(), "Move report.pdf to /home/me/old");
    assert_eq!(local.dest_side(), Side::Local);
    assert!(!local.uses_server());

    let remote = Job {
        side: Side::Remote,
        kind: JobKind::Copy,
        ..local
    };
    assert_eq!(
        remote.title(),
        "Copy report.pdf to /home/me/old on the server"
    );
    assert_eq!(remote.dest_side(), Side::Remote);
    assert!(remote.uses_server());
}

#[test]
fn jobs_saved_before_same_side_existed_still_load() {
    let text = r#"{"kind":"copy","side":"local","sources":["/a"],"dest_dir":"/srv"}"#;

    let job: Job = serde_json::from_str(text).expect("deserialize");

    assert!(!job.same_side);
    assert_eq!(job.dest_side(), Side::Remote);
}

#[test]
fn check_same_side_refuses_pointless_and_impossible_drops() {
    let sources = [local(Path::new("/data/a"))];

    assert!(check_same_side(&sources, &local(Path::new("/data/b"))).is_ok());
    assert!(check_same_side(&sources, &local(Path::new("/"))).is_ok());
    assert_eq!(
        check_same_side(&sources, &local(Path::new("/data"))),
        Err(String::from("a is already in /data"))
    );
    assert_eq!(
        check_same_side(&sources, &local(Path::new("/data/a"))),
        Err(String::from("a cannot go inside itself"))
    );
    assert_eq!(
        check_same_side(&sources, &local(Path::new("/data/a/deeper"))),
        Err(String::from("a cannot go inside itself"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn same_side_copy_puts_a_copy_in_another_folder() {
    let (_temp, src, _dst) = setup();
    std::fs::create_dir(src.join("b")).expect("b");

    let (summary, _) = run_local(same_side_job(
        JobKind::Copy,
        vec![src.join("a"), src.join("top.txt")],
        &src.join("b"),
    ))
    .await;

    assert_eq!(summary.done, 5);
    assert_eq!(
        std::fs::read_to_string(src.join("b/a/sub/two.txt")).expect("read"),
        "two!"
    );
    assert_eq!(
        std::fs::read(src.join("b/top.txt")).expect("read"),
        big_content()
    );
    assert!(src.join("a/sub/two.txt").exists(), "original kept");
}

#[tokio::test(flavor = "multi_thread")]
async fn same_side_move_renames_into_place() {
    let (_temp, src, _dst) = setup();
    std::fs::create_dir(src.join("b")).expect("b");

    let (summary, events) = run_local(same_side_job(
        JobKind::Move,
        vec![src.join("a"), src.join("top.txt")],
        &src.join("b"),
    ))
    .await;

    // Every item is reported done exactly once, including those inside the folder.
    assert_eq!(summary.done, 5);
    assert!(summary.is_complete());
    let finished = events
        .iter()
        .filter(|event| matches!(event, TransferEvent::ItemFinished { .. }))
        .count();
    assert_eq!(finished, 5);
    assert!(!src.join("a").exists(), "moved folder is gone");
    assert!(!src.join("top.txt").exists(), "moved file is gone");
    assert_eq!(
        std::fs::read_to_string(src.join("b/a/sub/two.txt")).expect("read"),
        "two!"
    );
    assert!(
        !src.join("b/top.txt.filepart").exists(),
        "renamed, not copied"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn same_side_move_into_an_existing_folder_merges_and_asks() {
    // Arrange: b/a already exists with its own one.txt.
    let (_temp, src, _dst) = setup();
    std::fs::create_dir_all(src.join("b/a")).expect("b/a");
    std::fs::write(src.join("b/a/one.txt"), "old").expect("write");

    // Act
    let (summary, events) = run_local_answering(
        same_side_job(JobKind::Move, vec![src.join("a")], &src.join("b")),
        answer(ConflictChoice::Overwrite, true),
    )
    .await;

    // Assert: the folders were merged file by file, asking about the one in the way.
    assert_eq!(conflicts(&events), 1);
    assert!(summary.is_complete());
    assert_eq!(
        std::fs::read_to_string(src.join("b/a/one.txt")).expect("read"),
        "one"
    );
    assert_eq!(
        std::fs::read_to_string(src.join("b/a/sub/two.txt")).expect("read"),
        "two!"
    );
    assert!(!src.join("a").exists(), "emptied source folder removed");
}

#[tokio::test(flavor = "multi_thread")]
async fn same_side_job_into_itself_does_not_start() {
    let (_temp, src, _dst) = setup();

    let (summary, _) = run_local(same_side_job(
        JobKind::Move,
        vec![src.join("a")],
        &src.join("a/sub"),
    ))
    .await;

    assert_eq!(summary.error.as_deref(), Some("a cannot go inside itself"));
    assert!(src.join("a/sub/two.txt").exists(), "nothing touched");
}

#[test]
fn jobs_round_trip_through_json() {
    // An upload: local sources, a destination on the server.
    let original = Job {
        dest_dir: remote("/dest"),
        ..job(JobKind::Move, vec![PathBuf::from("/x/y")], Path::new(""))
    };
    let text = serde_json::to_string(&original).expect("serialize");
    let back: Job = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(back, original);
}

#[test]
fn file_info_same_as_compares_size_and_whole_seconds() {
    let at = |seconds, nanos| Some(UNIX_EPOCH + Duration::new(seconds, nanos));
    let a = FileInfo {
        size: 5,
        modified: at(100, 900),
    };
    assert!(a.same_as(&FileInfo {
        size: 5,
        modified: at(100, 0)
    }));
    assert!(!a.same_as(&FileInfo {
        size: 6,
        modified: at(100, 0)
    }));
    assert!(!a.same_as(&FileInfo {
        size: 5,
        modified: None
    }));
}

#[test]
fn paths_in_use_are_the_targets() {
    let upload = Job {
        dest_dir: remote("/srv"),
        ..job(
            JobKind::Copy,
            vec![PathBuf::from("/home/me/a"), PathBuf::from("/home/me/b.txt")],
            Path::new(""),
        )
    };
    assert_eq!(
        upload.paths_in_use(),
        [remote("/srv/a"), remote("/srv/b.txt")]
    );

    // A delete has no destination: what it removes is in use.
    let delete = Job {
        kind: JobKind::Delete,
        ..upload
    };
    assert_eq!(
        delete.paths_in_use(),
        [
            local(Path::new("/home/me/a")),
            local(Path::new("/home/me/b.txt")),
        ]
    );
}
