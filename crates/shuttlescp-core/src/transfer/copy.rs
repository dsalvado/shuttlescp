//! Carrying out a planned job: settling conflicts, copying (or moving) files and
//! folders, and deleting.

use crate::path::SidePath;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Semaphore, oneshot};
use tokio::task::JoinSet;

use super::endpoint::{Endpoint, partial_path};
use super::{
    CancelFlag, ConflictAnswer, ConflictChoice, ConflictReply, FILES_AT_ONCE, ItemKind,
    ItemOutcome, Job, JobKind, PlanItem, Sink, TransferEvent,
};
use crate::format::format_size;

/// Bytes read and written per step of a file copy.
const CHUNK_BYTES: usize = 256 * 1024;
/// Shortest time between two progress reports for one file, so a fast copy does not
/// flood the user interface with updates.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// Bytes compared at the end of a partial copy before continuing it, to make sure it
/// still matches the source.
const RESUME_CHECK_BYTES: u64 = 64 * 1024;
/// How often a job waiting for a conflict answer checks whether it was cancelled.
const ANSWER_POLL: Duration = Duration::from_millis(200);

/// What to do with one file of a copy job, decided before copying starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileAction {
    /// Copy it; nothing is in the way.
    Copy,
    /// Copy it, replacing the file there.
    Overwrite,
    /// An identical copy is already there (an interrupted run finished it): nothing to
    /// copy, but a move still deletes the source.
    AlreadyThere,
}

/// Copy (or move) every planned item.
///
/// # Returns
///
/// Each item's outcome, by id
pub(super) async fn copy_items(
    job: &Job,
    items: &[PlanItem],
    src: &Endpoint,
    dst: &Endpoint,
    cancel: &CancelFlag,
    sink: &Sink,
) -> Vec<ItemOutcome> {
    let delete_source = job.kind == JobKind::Move;
    let mut outcomes: Vec<Option<ItemOutcome>> = vec![None; items.len()];
    let finish = |outcomes: &mut Vec<Option<ItemOutcome>>, id: usize, outcome: ItemOutcome| {
        sink(TransferEvent::ItemFinished {
            id,
            outcome: outcome.clone(),
        });
        outcomes[id] = Some(outcome);
    };

    // 0. Moving within one side: what has nothing in its way is renamed into place,
    //    with everything inside it, instead of being copied.
    let children = children_of(items);
    let mut renamed = vec![false; items.len()];
    if delete_source && job.same_side {
        for id in rename_into_place(items, &children, src, cancel).await {
            renamed[id] = true;
            finish(&mut outcomes, id, ItemOutcome::Done);
        }
    }

    // 1. Settle every file already at the destination, one question at a time, before
    //    anything is copied.
    let actions = settle_conflicts(job, items, &renamed, cancel, sink, &mut |id, outcome| {
        finish(&mut outcomes, id, outcome);
    })
    .await;

    // 2. Folders, parents before children, so every file has somewhere to go.
    let failed_parent = |outcomes: &[Option<ItemOutcome>], item: &PlanItem| {
        item.parent
            .is_some_and(|parent| matches!(outcomes[parent], Some(ItemOutcome::Failed(_))))
    };
    for item in items
        .iter()
        .filter(|item| item.kind == ItemKind::Folder && !renamed[item.id])
    {
        if failed_parent(&outcomes, item) {
            outcomes[item.id] = Some(ItemOutcome::Failed(String::from(
                "the folder it is in could not be created",
            )));
        } else if let Err(error) = dst.create_dir(&item.dest).await {
            tracing::warn!("{error}");
            outcomes[item.id] = Some(ItemOutcome::Failed(error));
        }
    }

    // 3. Files, several at a time; each runs as its own task.
    let slots = Arc::new(Semaphore::new(FILES_AT_ONCE));
    let mut running = JoinSet::new();
    for item in items.iter().filter(|item| item.kind == ItemKind::File) {
        let Some(action) = actions[item.id] else {
            // Already settled (skipped, failed or cancelled) while handling conflicts.
            continue;
        };
        if failed_parent(&outcomes, item) {
            let outcome =
                ItemOutcome::Failed(String::from("the folder it goes in could not be created"));
            finish(&mut outcomes, item.id, outcome);
            continue;
        }
        // Waits here while FILES_AT_ONCE files are already being copied. The semaphore
        // is never closed, so acquiring cannot fail.
        let Ok(slot) = Arc::clone(&slots).acquire_owned().await else {
            break;
        };
        while let Some(finished) = running.try_join_next() {
            record_finished(finished, &mut outcomes, sink);
        }
        if cancel.is_cancelled() {
            finish(&mut outcomes, item.id, ItemOutcome::Cancelled);
            continue;
        }
        if cancel.is_item_stopped(item.id) {
            tracing::info!("stopped by the user before copying: {}", item.source);
            finish(&mut outcomes, item.id, ItemOutcome::Stopped);
            continue;
        }
        let (item, src, dst, cancel, sink) = (
            item.clone(),
            src.clone(),
            dst.clone(),
            cancel.clone(),
            Arc::clone(sink),
        );
        running.spawn(async move {
            // Holding the slot until the copy ends lets the next file start.
            let _slot = slot;
            let outcome = match action {
                FileAction::AlreadyThere => {
                    tracing::info!("already copied earlier: {}", item.dest);
                    finish_source(&item, &src, delete_source).await
                }
                FileAction::Copy | FileAction::Overwrite => {
                    let replace = action == FileAction::Overwrite;
                    copy_file(&item, &src, &dst, delete_source, replace, &cancel, &sink).await
                }
            };
            (item.id, outcome)
        });
    }
    while let Some(finished) = running.join_next().await {
        record_finished(finished, &mut outcomes, sink);
    }

    // 4. Folders last, deepest first: each is done once everything inside it is.
    for item in items
        .iter()
        .rev()
        .filter(|item| item.kind == ItemKind::Folder && !renamed[item.id])
    {
        let outcome = match &outcomes[item.id] {
            Some(failed) => failed.clone(),
            None => folder_outcome(item, &children[item.id], &outcomes, src, delete_source).await,
        };
        finish(&mut outcomes, item.id, outcome);
    }
    outcomes
        .into_iter()
        .map(|outcome| outcome.unwrap_or(ItemOutcome::Cancelled))
        .collect()
}

/// Move the job's sources that have nothing in their way at the destination by
/// renaming them, which is instant however big they are. A rename that fails (e.g.
/// between two disks) leaves the item to be copied and deleted instead.
///
/// # Arguments
///
/// * `children` - The items directly inside each item, by id
///
/// # Returns
///
/// The ids of every item moved: the renamed sources and everything inside them
async fn rename_into_place(
    items: &[PlanItem],
    children: &[Vec<usize>],
    src: &Endpoint,
    cancel: &CancelFlag,
) -> Vec<usize> {
    let mut moved = Vec::new();
    for item in items.iter().filter(|item| item.parent.is_none()) {
        if cancel.is_cancelled() {
            break;
        }
        if item.existing.is_some() || item.existing_is_dir || item.partial > 0 {
            continue;
        }
        if let Err(error) = src.rename(&item.source, &item.dest).await {
            tracing::debug!("{error}; copying instead");
            continue;
        }
        tracing::info!("moved {} -> {}", item.source, item.dest);
        let mut inside = vec![item.id];
        while let Some(id) = inside.pop() {
            moved.push(id);
            inside.extend(&children[id]);
        }
    }
    moved
}

/// Decide what to do with each file, asking about files already at the destination.
///
/// # Arguments
///
/// * `skip` - Items already dealt with, by id: they get no action
/// * `settled` - Called for files that need nothing more (skipped, failed, cancelled)
///
/// # Returns
///
/// The action for each file still to process, by id (`None` for settled files and
/// folders)
async fn settle_conflicts(
    job: &Job,
    items: &[PlanItem],
    skip: &[bool],
    cancel: &CancelFlag,
    sink: &Sink,
    settled: &mut impl FnMut(usize, ItemOutcome),
) -> Vec<Option<FileAction>> {
    let mut actions = vec![None; items.len()];
    let mut for_all: Option<ConflictChoice> = None;
    for item in items
        .iter()
        .filter(|item| item.kind == ItemKind::File && !skip[item.id])
    {
        if cancel.is_cancelled() {
            settled(item.id, ItemOutcome::Cancelled);
            continue;
        }
        if cancel.is_item_stopped(item.id) {
            settled(item.id, ItemOutcome::Stopped);
            continue;
        }
        if item.existing_is_dir {
            settled(
                item.id,
                ItemOutcome::Failed(format!("a folder named {} is already there", item.name)),
            );
            continue;
        }
        let Some(existing) = item.existing else {
            actions[item.id] = Some(FileAction::Copy);
            continue;
        };
        if job.resume && existing.same_as(&item.info()) {
            actions[item.id] = Some(FileAction::AlreadyThere);
            continue;
        }
        let choice = match for_all {
            Some(choice) => choice,
            None => {
                let answer = ask(item, existing, cancel, sink).await;
                if answer.apply_to_all {
                    for_all = Some(answer.choice);
                }
                answer.choice
            }
        };
        match choice {
            ConflictChoice::Overwrite => actions[item.id] = Some(FileAction::Overwrite),
            ConflictChoice::Skip => {
                tracing::info!("not overwriting {}", item.dest);
                settled(item.id, ItemOutcome::Skipped);
            }
            ConflictChoice::CancelJob => {
                tracing::info!("cancelled at the conflict over {}", item.dest);
                cancel.cancel();
                settled(item.id, ItemOutcome::Cancelled);
            }
        }
    }
    actions
}

/// Ask the front end what to do about a file already at the destination, and wait for
/// the answer. Cancelling the job, or an answer that never comes, cancels.
async fn ask(
    item: &PlanItem,
    existing: super::FileInfo,
    cancel: &CancelFlag,
    sink: &Sink,
) -> ConflictAnswer {
    let cancelled = ConflictAnswer {
        choice: ConflictChoice::CancelJob,
        apply_to_all: false,
    };
    let (sender, mut answer) = oneshot::channel();
    sink(TransferEvent::Conflict {
        id: item.id,
        existing,
        incoming: item.info(),
        reply: ConflictReply::new(sender),
    });
    loop {
        match tokio::time::timeout(ANSWER_POLL, &mut answer).await {
            Ok(Ok(answer)) => return answer,
            // The question was dropped unanswered.
            Ok(Err(_)) => return cancelled,
            Err(_) if cancel.is_cancelled() => return cancelled,
            Err(_) => {}
        }
    }
}

/// How a folder of a copy job ended, from the items inside it. A moved folder is
/// deleted once everything in it was moved.
async fn folder_outcome(
    folder: &PlanItem,
    children: &[usize],
    outcomes: &[Option<ItemOutcome>],
    src: &Endpoint,
    delete_source: bool,
) -> ItemOutcome {
    let inside = || children.iter().map(|&child| outcomes[child].as_ref());
    if inside().all(|outcome| outcome == Some(&ItemOutcome::Done)) {
        if !delete_source {
            return ItemOutcome::Done;
        }
        // Only an empty folder can be removed, so anything added to it since the job
        // was planned is never lost.
        return src
            .remove_dir(&folder.source)
            .await
            .map_or_else(ItemOutcome::Failed, |()| ItemOutcome::Done);
    }
    if inside().any(|outcome| matches!(outcome, Some(ItemOutcome::Failed(_)))) {
        ItemOutcome::Failed(String::from("some items inside it failed"))
    } else if inside().any(|outcome| matches!(outcome, Some(ItemOutcome::Cancelled) | None)) {
        ItemOutcome::Cancelled
    } else if inside().any(|outcome| outcome == Some(&ItemOutcome::Stopped)) {
        // Stopped files are not there (or, when moving, still here with this folder).
        ItemOutcome::Stopped
    } else if delete_source {
        // Some files were skipped, so their sources (and this folder) stay.
        ItemOutcome::Skipped
    } else {
        ItemOutcome::Done
    }
}

/// The items directly inside each item, by id.
pub(super) fn children_of(items: &[PlanItem]) -> Vec<Vec<usize>> {
    let mut children = vec![Vec::new(); items.len()];
    for item in items {
        if let Some(parent) = item.parent {
            children[parent].push(item.id);
        }
    }
    children
}

/// Record one finished file task and report it.
fn record_finished(
    finished: Result<(usize, ItemOutcome), tokio::task::JoinError>,
    outcomes: &mut [Option<ItemOutcome>],
    sink: &Sink,
) {
    match finished {
        Ok((id, outcome)) => {
            sink(TransferEvent::ItemFinished {
                id,
                outcome: outcome.clone(),
            });
            outcomes[id] = Some(outcome);
        }
        // A copy task panicked: its item stays without an outcome and counts as not done.
        Err(error) => tracing::error!("file copy task failed: {error}"),
    }
}

/// A file whose copy is complete: delete the source if moving.
async fn finish_source(item: &PlanItem, src: &Endpoint, delete_source: bool) -> ItemOutcome {
    if delete_source && let Err(error) = src.remove_file(&item.source).await {
        tracing::warn!("{error}");
        return ItemOutcome::Failed(format!("copied, but {error}"));
    }
    ItemOutcome::Done
}

/// Copy one file through its partial file, continuing an earlier partial copy when
/// there is one, then give it its real name (replacing the existing file if `replace`)
/// and delete the source if moving.
async fn copy_file(
    item: &PlanItem,
    src: &Endpoint,
    dst: &Endpoint,
    delete_source: bool,
    replace: bool,
    cancel: &CancelFlag,
    sink: &Sink,
) -> ItemOutcome {
    let started = Instant::now();
    let partial = partial_path(&item.dest);
    let offset = resume_offset(item, src, dst, &partial).await;
    if offset > 0 {
        tracing::info!("resuming {} at {offset} of {} bytes", item.dest, item.size);
    } else {
        tracing::debug!("copying {} to {}", item.source, item.dest);
    }
    match copy_contents(item, src, dst, &partial, offset, cancel, sink).await {
        Ok(()) => {}
        Err(outcome @ (ItemOutcome::Cancelled | ItemOutcome::Stopped)) => {
            // Cancelling (or stopping this file) means the user wants the copy gone,
            // not kept for later.
            let _ = dst.remove_file(&partial).await;
            if outcome == ItemOutcome::Stopped {
                tracing::info!("stopped by the user: {}", item.source);
            }
            return outcome;
        }
        Err(outcome) => {
            if let ItemOutcome::Failed(error) = &outcome {
                tracing::warn!("{error}; the partial copy is kept for resuming");
            }
            return outcome;
        }
    }
    if replace && let Err(error) = dst.remove_file(&item.dest).await {
        return ItemOutcome::Failed(error);
    }
    if let Err(error) = dst.rename(&partial, &item.dest).await {
        return ItemOutcome::Failed(error);
    }
    if let Some(modified) = item.modified
        && let Err(error) = dst.set_modified(&item.dest, modified).await
    {
        // The copy itself is fine; only its date differs.
        tracing::debug!("{error}");
    }
    let outcome = finish_source(item, src, delete_source).await;
    if outcome == ItemOutcome::Done {
        log_copied(item, offset, started.elapsed(), delete_source);
    }
    outcome
}

/// Record a finished file copy in the log: what went where, how big, how fast.
///
/// # Arguments
///
/// * `offset` - Bytes copied by an earlier, interrupted attempt
/// * `took` - How long this attempt took
/// * `moved` - The source was deleted afterwards
fn log_copied(item: &PlanItem, offset: u64, took: Duration, moved: bool) {
    let sent = item.size.saturating_sub(offset);
    let seconds = took.as_secs_f64();
    let speed = if seconds > 0.0 {
        // Rounded to whole bytes for display.
        format!(", {}/s", format_size((sent as f64 / seconds) as u64))
    } else {
        String::new()
    };
    let resumed = if offset > 0 {
        format!(", resumed at {}", format_size(offset))
    } else {
        String::new()
    };
    tracing::info!(
        "{} {} -> {}: {} in {seconds:.1} s{speed}{resumed}",
        if moved { "moved" } else { "copied" },
        item.source,
        item.dest,
        format_size(item.size),
    );
}

/// Where to continue an earlier partial copy: its size, if it is not longer than the
/// source and its last bytes match the source's; otherwise 0 (start over).
async fn resume_offset(item: &PlanItem, src: &Endpoint, dst: &Endpoint, partial: &SidePath) -> u64 {
    let offset = item.partial;
    if offset == 0 || offset > item.size {
        return 0;
    }
    let check = RESUME_CHECK_BYTES.min(offset);
    // Bounded by RESUME_CHECK_BYTES, so it always fits a `usize`.
    let len = usize::try_from(check).unwrap_or(0);
    let start = offset - check;
    match (
        src.read_range(&item.source, start, len).await,
        dst.read_range(partial, start, len).await,
    ) {
        (Ok(source), Ok(copy)) if source == copy => offset,
        _ => {
            tracing::info!("{} does not match its source; starting over", partial);
            0
        }
    }
}

/// Stream a file's bytes from `src` to `partial` starting at `offset`, reporting
/// progress.
///
/// # Errors
///
/// Returns the outcome to report when the copy did not complete
async fn copy_contents(
    item: &PlanItem,
    src: &Endpoint,
    dst: &Endpoint,
    partial: &SidePath,
    offset: u64,
    cancel: &CancelFlag,
    sink: &Sink,
) -> Result<(), ItemOutcome> {
    let failed = ItemOutcome::Failed;
    let mut reader = src.open_at(&item.source, offset).await.map_err(failed)?;
    let mut writer = dst.open_write_at(partial, offset).await.map_err(failed)?;
    let mut buffer = vec![0; CHUNK_BYTES];
    let mut done = offset;
    if offset > 0 {
        sink(TransferEvent::Resumed {
            id: item.id,
            from: offset,
        });
    }
    let mut reported = Instant::now();
    loop {
        if cancel.is_cancelled() {
            return Err(ItemOutcome::Cancelled);
        }
        if cancel.is_item_stopped(item.id) {
            return Err(ItemOutcome::Stopped);
        }
        let read = reader.read(&mut buffer).await.map_err(|error| {
            ItemOutcome::Failed(format!("cannot read {}: {error}", item.source))
        })?;
        if read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..read])
            .await
            .map_err(|error| ItemOutcome::Failed(format!("cannot write {}: {error}", partial)))?;
        done += read as u64;
        if reported.elapsed() >= PROGRESS_INTERVAL {
            sink(TransferEvent::Progress { id: item.id, done });
            reported = Instant::now();
        }
    }
    // Waits for the last writes to be acknowledged and closes the file.
    writer
        .shutdown()
        .await
        .map_err(|error| ItemOutcome::Failed(format!("cannot finish {}: {error}", partial)))?;
    sink(TransferEvent::Progress { id: item.id, done });
    Ok(())
}

/// Delete every planned item: contents before the folders holding them.
///
/// # Returns
///
/// Each item's outcome, by id
pub(super) async fn delete_items(
    items: &[PlanItem],
    src: &Endpoint,
    cancel: &CancelFlag,
    sink: &Sink,
) -> Vec<ItemOutcome> {
    let mut outcomes = vec![ItemOutcome::Cancelled; items.len()];
    let children = children_of(items);
    // Children always come after their parent, so going backwards empties each folder
    // before it is removed.
    for item in items.iter().rev() {
        let inside = || children[item.id].iter().map(|&child| &outcomes[child]);
        let outcome = if cancel.is_cancelled() {
            ItemOutcome::Cancelled
        } else if cancel.is_item_stopped(item.id)
            || (item.kind == ItemKind::Folder
                && inside().any(|outcome| *outcome == ItemOutcome::Stopped)
                && inside()
                    .all(|outcome| matches!(outcome, ItemOutcome::Done | ItemOutcome::Stopped)))
        {
            // Left as it is, with whatever the user stopped inside it.
            ItemOutcome::Stopped
        } else {
            match item.kind {
                ItemKind::File => src.remove_file(&item.source).await,
                ItemKind::Folder if inside().all(|outcome| *outcome == ItemOutcome::Done) => {
                    src.remove_dir(&item.source).await
                }
                ItemKind::Folder => Err(String::from("some items inside it were not deleted")),
            }
            .map_or_else(ItemOutcome::Failed, |()| ItemOutcome::Done)
        };
        match &outcome {
            ItemOutcome::Done => tracing::info!(
                "deleted {}{}",
                if item.kind == ItemKind::Folder {
                    "folder "
                } else {
                    ""
                },
                item.source
            ),
            ItemOutcome::Failed(error) => tracing::warn!("{error}"),
            ItemOutcome::Stopped => tracing::info!("not deleted (stopped): {}", item.source),
            ItemOutcome::Skipped | ItemOutcome::Cancelled => {}
        }
        sink(TransferEvent::ItemFinished {
            id: item.id,
            outcome: outcome.clone(),
        });
        outcomes[item.id] = outcome;
    }
    outcomes
}
