//! Copying one large file in parts: byte ranges copied at the same time, each over a
//! connection of its own, into one copy (`name.fileparts`, see
//! [`super::super::PARTS_SUFFIX`]). Worth it where servers limit each connection's
//! speed, or one file uses only part of the network (see [`faster_in_parts`]).
//!
//! The copy is set to its full size first, and every part writes its range in place
//! ([`Endpoint::open_write_range`]), so nothing is joined afterwards. Each part hashes
//! the bytes it copies, so the copy can be checked range by range without reading the
//! source again.
//!
//! How far each part got is recorded as it goes (see [`super::record`]), so a copy
//! paused, held back or interrupted continues where its parts were.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::record::{self, SplitRecord, SplitRecords};
use super::{
    CHUNK_BYTES, ExtraPart, FileEnd, FileTimes, PARTWAY_PAUSE, PARTWAY_TRIES, PROGRESS_INTERVAL,
    RESUME_CHECK_BYTES, Spare, Unfinished, ended,
};
use crate::checksum::Checksum;
use crate::path::SidePath;
use crate::transfer::endpoint::Endpoint;
use crate::transfer::{
    CancelFlag, ItemOutcome, PartProgress, Place, PlanItem, Sink, TransferEvent,
};

/// Files smaller than this are copied whole (and the menu offers copying in parts only
/// for files at least this large).
pub const SPLIT_FROM: u64 = 1 << 30;

/// One end of a copy, as far as copying in parts goes (see [`faster_in_parts`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartsHost {
    /// This computer.
    ThisComputer,
    /// An FTP server: it serves each connection separately, and can write a file only
    /// from its start (or by appending).
    Ftp,
    /// An SFTP server: it can write a part at any position.
    Sftp,
}

/// Whether copying a large file from `from` to `to` in parts, each over a connection
/// of its own, is faster than copying it whole (the destination must also be able to
/// write a part at any position, which an FTP server cannot):
///
/// * From an FTP server: servers often limit each connection's speed (measured on a
///   seedbox: about 1 MiB/s per connection, 3.3 MiB/s with four).
/// * From this computer to an SFTP server: one file moves at a fraction of the line's
///   speed (measured on a LAN: 14 to 26 MiB/s per file, 65 MiB/s for four at once).
///
/// Not yet from an SFTP server: whether parts help there is still to be measured.
pub fn faster_in_parts(from: PartsHost, to: PartsHost) -> bool {
    use PartsHost::{Ftp, Sftp, ThisComputer};
    matches!(
        (from, to),
        (Ftp, ThisComputer | Sftp) | (ThisComputer, Sftp)
    )
}
/// No part is smaller than this (so a 1 GiB file has at most 4).
pub(super) const MIN_PART: u64 = 256 << 20;
/// How often each part records how far it got. Writing the record costs little; what
/// it lags behind is copied again when continuing.
const RECORD_INTERVAL: Duration = Duration::from_secs(5);
/// How often a file copied in parts looks for a connection freed elsewhere in the job,
/// to join it late (see [`copy_parts`]).
const JOIN_POLL: Duration = Duration::from_secs(1);

/// A stretch of the copy whose bytes were hashed as they were copied: `start..end`
/// holds bytes with this SHA-256 checksum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Piece {
    pub(super) start: u64,
    pub(super) end: u64,
    pub(super) sha256: Checksum,
}

/// How many parts a file of `size` bytes is worth copying in, given `connections`
/// that could be used: one (whole) below [`SPLIT_FROM`], and no part under
/// [`MIN_PART`].
pub(super) fn parts_for(size: u64, connections: usize) -> usize {
    if size < SPLIT_FROM {
        return 1;
    }
    let by_size = usize::try_from(size / MIN_PART).unwrap_or(usize::MAX);
    connections.min(by_size).max(1)
}

/// Split `0..size` into `parts` ranges, equal but for the last, which takes the rest.
pub(super) fn ranges(size: u64, parts: usize) -> Vec<(u64, u64)> {
    let count = parts.max(1) as u64;
    let each = size / count;
    (0..count)
        .map(|part| {
            let start = part * each;
            let end = if part + 1 == count {
                size
            } else {
                start + each
            };
            (start, end)
        })
        .collect()
}

/// Check that `pieces` cover `0..size` exactly: no gap, no overlap.
///
/// # Errors
///
/// Returns where they do not
pub(super) fn cover_the_file(pieces: &[Piece], size: u64) -> Result<(), String> {
    let mut sorted: Vec<&Piece> = pieces.iter().collect();
    sorted.sort_by_key(|piece| piece.start);
    let mut at = 0;
    for piece in sorted {
        if piece.start != at {
            return Err(format!("the parts leave {at}..{} out", piece.start));
        }
        tracing::debug!(
            "copied the part {}..{}, SHA-256 {}",
            piece.start,
            piece.end,
            piece.sha256
        );
        at = piece.end;
    }
    if at == size {
        Ok(())
    } else {
        Err(format!("the parts end at {at}, not {size}"))
    }
}

/// One part of a file copied in parts: the range `start..end` it copies, and the
/// pieces of it done so far, in order from `start` (more than one when it was
/// continued). It has got to the end of the last one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Part {
    pub(super) start: u64,
    pub(super) end: u64,
    pub(super) pieces: Vec<Piece>,
}

impl Part {
    /// A part of `range` with nothing done yet.
    pub(super) fn new((start, end): (u64, u64)) -> Self {
        Self {
            start,
            end,
            pieces: Vec::new(),
        }
    }

    /// Where it has got to.
    pub(super) fn at(&self) -> u64 {
        self.pieces.last().map_or(self.start, |piece| piece.end)
    }

    /// Bytes of it done.
    pub(super) fn done(&self) -> u64 {
        self.at() - self.start
    }

    /// Whether all of it is done.
    pub(super) fn is_done(&self) -> bool {
        self.at() >= self.end
    }

    /// This part with one more piece: `from..at`, hashed by `hasher` (nothing, when it
    /// is empty).
    fn with_piece(&self, from: u64, at: u64, hasher: &Sha256) -> Self {
        let mut part = self.clone();
        if at > from {
            part.pieces.push(Piece {
                start: from,
                end: at,
                sha256: Checksum::from_hasher(hasher.clone()),
            });
        }
        part
    }
}

/// What every part of one file's copy shares: the file, where it is copied from and
/// into (`copy`, its partial copy), and the job's requests and events.
pub(super) struct Copying<'a> {
    pub(super) item: &'a PlanItem,
    pub(super) src: &'a Endpoint,
    pub(super) dst: &'a Endpoint,
    pub(super) copy: &'a SidePath,
    pub(super) cancel: &'a CancelFlag,
    pub(super) sink: &'a Sink,
}

/// Keeps a file's record (see [`super::record`]) up to date as its parts go.
pub(super) struct Recorder<'a> {
    records: &'a SplitRecords,
    key: String,
    record: Mutex<SplitRecord>,
}

impl<'a> Recorder<'a> {
    /// Start recording `item`'s copy in `parts` in `records` (saved at once).
    pub(super) fn new(records: &'a SplitRecords, item: &PlanItem, parts: &[Part]) -> Self {
        let (key, record) = (record::key(item), SplitRecord::new(item, parts.to_vec()));
        records.set(&key, record.clone());
        Self {
            records,
            key,
            record: Mutex::new(record),
        }
    }

    /// Record the pieces part `index` has done, `part`'s (only its pieces: its end may
    /// have moved since, see [`Self::split`]).
    fn reached(&self, index: usize, part: &Part) {
        let Ok(mut record) = self.record.lock() else {
            return;
        };
        if let Some(recorded) = record.parts.get_mut(index) {
            recorded.pieces.clone_from(&part.pieces);
        }
        self.records.set(&self.key, record.clone());
    }

    /// Record that part `index` now ends at `at`, and a new part (the next number)
    /// copies from there to where it ended.
    fn split(&self, index: usize, at: u64) {
        let Ok(mut record) = self.record.lock() else {
            return;
        };
        let Some(part) = record.parts.get_mut(index) else {
            return;
        };
        let end = std::mem::replace(&mut part.end, at);
        record.parts.push(Part::new((at, end)));
        self.records.set(&self.key, record.clone());
    }

    /// Forget the record: the copy is done, or gone.
    pub(super) fn forget(&self) {
        self.records.forget(&self.key);
    }
}

/// The file's progress, summed over its parts, reported at most every
/// [`PROGRESS_INTERVAL`], with where each part has got to.
struct Progress<'a> {
    id: usize,
    done: AtomicU64,
    reported: Mutex<Instant>,
    sink: &'a Sink,
    /// The parts, reported with the progress ([`TransferEvent::Parts`]); `None` to not
    /// report them (a piece copied again: it is not how the file is split).
    spans: Option<&'a Mutex<Vec<Span>>>,
}

impl Progress<'_> {
    fn add(&self, bytes: u64) {
        let done = self.done.fetch_add(bytes, Ordering::Relaxed) + bytes;
        let Ok(mut reported) = self.reported.lock() else {
            return;
        };
        if reported.elapsed() >= PROGRESS_INTERVAL {
            *reported = Instant::now();
            drop(reported);
            (self.sink)(TransferEvent::Progress { id: self.id, done });
            self.report_parts();
        }
    }

    /// Report where each part has got to (if the parts are reported at all).
    fn report_parts(&self) {
        let Some(spans) = self.spans else {
            return;
        };
        let Ok(parts) = spans
            .lock()
            .map(|spans| spans.iter().map(Span::progress).collect())
        else {
            return;
        };
        (self.sink)(TransferEvent::Parts { id: self.id, parts });
    }
}

/// The connections a file is copied in parts over: its own (with its place in the
/// limit shared by every job), the extra ones it took, and where to take more.
pub(super) struct Connections<'a> {
    pub(super) own: Option<&'a Place>,
    pub(super) extra: Vec<ExtraPart>,
    /// The job's room to spare, to take connections that free up later; `None` to use
    /// only these.
    pub(super) spare: Option<&'a Spare>,
    /// A part is split for a connection joining late only if each half is at least
    /// this ([`MIN_PART`], smaller in tests).
    pub(super) min_part: u64,
}

impl Connections<'_> {
    /// Only the file's own connection.
    pub(super) fn own(place: Option<&Place>) -> Connections<'_> {
        Connections {
            own: place,
            extra: Vec::new(),
            spare: None,
            min_part: u64::MAX,
        }
    }
}

/// Copy the file in `parts`, over `connections`. A new copy (`fresh`) is first set to
/// its full size; a continued one has its parts' last bytes checked against the source
/// first (see [`still_matches`]).
///
/// # Arguments
///
/// * `recorder` - Where the parts record how far they got
///
/// # Returns
///
/// How long it took, and the pieces copied (in no particular order), to check the copy
/// by
///
/// # Errors
///
/// Returns why the copy did not complete. What was done is recorded, to continue it.
pub(super) async fn copy_in_parts(
    run: &Copying<'_>,
    parts: Vec<Part>,
    fresh: bool,
    connections: Connections<'_>,
    recorder: Option<&Recorder<'_>>,
) -> Result<(FileTimes, Vec<Piece>), Unfinished> {
    let (item, sink) = (run.item, run.sink);
    let opening = Instant::now();
    let parts = if fresh {
        run.dst
            .set_size(run.copy, item.size)
            .await
            .map_err(ItemOutcome::Failed)?;
        parts
    } else {
        let at_once = 1 + connections.extra.len();
        let checking = futures_util::stream::iter(parts).map(|part| still_matches(run, part));
        let parts: Vec<Part> = checking.buffered(at_once).collect().await;
        if let Some(recorder) = recorder {
            for (index, part) in parts.iter().enumerate() {
                recorder.reached(index, part);
            }
        }
        parts
    };
    let open = opening.elapsed();
    let moving = Instant::now();
    let done: u64 = parts.iter().map(Part::done).sum();
    let ranges = parts
        .iter()
        .map(|part| format!("{}..{} (at {})", part.start, part.end, part.at()))
        .collect::<Vec<_>>()
        .join(", ");
    let count = match parts.len() {
        1 => String::from("1 part"),
        many => format!("{many} parts"),
    };
    if done > 0 {
        tracing::info!(
            "resuming {} at {done} of {} bytes, in {count}: {ranges}",
            item.dest,
            item.size
        );
        sink(TransferEvent::Resumed {
            id: item.id,
            from: done,
        });
    } else {
        tracing::info!("copying {} in {count}: {ranges}", item.source);
    }
    let pieces = copy_parts(run, parts, connections, done, recorder, true).await?;
    Ok((
        FileTimes {
            open,
            data: moving.elapsed(),
            finish: std::time::Duration::ZERO,
        },
        pieces,
    ))
}

/// A part being continued, as it can be: unchanged if its last bytes (up to
/// [`RESUME_CHECK_BYTES`]) are still the source's, else started over (only that part).
/// Given up (unchanged) if the user ends the file meanwhile: the copy then ends at once.
async fn still_matches(run: &Copying<'_>, part: Part) -> Part {
    let at = part.at();
    if at == part.start {
        return part;
    }
    let check = RESUME_CHECK_BYTES.min(at - part.start);
    // Bounded by RESUME_CHECK_BYTES, so it always fits a `usize`.
    let len = usize::try_from(check).unwrap_or(0);
    let reading = async {
        tokio::join!(
            run.src.read_range(&run.item.source, at - check, len),
            run.dst.read_range(run.copy, at - check, len),
        )
    };
    match run.cancel.unless_ended(run.item.id, reading).await {
        None => part,
        Some((Ok(source), Ok(copy))) if source == copy => part,
        Some(_) => {
            tracing::info!(
                "{}..{} of {} does not match its source; that part starts over",
                part.start,
                at,
                run.copy
            );
            Part::new((part.start, part.end))
        }
    }
}

/// Where a part is and where it ends, shared between the part copying it and
/// [`copy_parts`], which may move the end back for a connection joining late.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: u64,
    at: u64,
    end: u64,
}

impl Span {
    fn progress(&self) -> PartProgress {
        PartProgress {
            start: self.start,
            at: self.at,
            end: self.end,
        }
    }
}

/// The part to halve for a connection joining late: the one with the most left, if
/// each half would be at least `min_part`.
///
/// Also never less than a chunk: a part may have read up to a chunk past where it last
/// said it was, and the new end must lie beyond that.
///
/// # Returns
///
/// The part's number, and where its second half starts
fn split_point(spans: &[Span], min_part: u64) -> Option<(usize, u64)> {
    let least = min_part.max(CHUNK_BYTES as u64);
    let left = |span: &Span| span.end.saturating_sub(span.at);
    let (index, span) = spans
        .iter()
        .enumerate()
        .max_by_key(|(_, span)| left(span))?;
    let half = left(span) / 2;
    (half >= least).then_some((index, span.at + half))
}

/// Copy what is left of `parts` (already full-sized in the copy) over `connections`.
///
/// Every free connection gets work: first a part not started yet, else the second half
/// of the part with the most left (late joining, see [`split_point`]); with none for
/// it, an extra connection is given back to the job's other files. Every
/// [`JOIN_POLL`], a connection freed elsewhere in the job is taken to join too, when
/// no whole file of the job waits for it.
///
/// # Arguments
///
/// * `done_before` - Bytes of the file already copied, where its progress starts
/// * `show_parts` - Report where the parts are ([`TransferEvent::Parts`]): at the
///   start, with progress, after each split and at the end
///
/// # Returns
///
/// The pieces of every part, in no particular order
///
/// # Errors
///
/// Returns why the copy did not complete
async fn copy_parts(
    run: &Copying<'_>,
    parts: Vec<Part>,
    connections: Connections<'_>,
    done_before: u64,
    recorder: Option<&Recorder<'_>>,
    show_parts: bool,
) -> Result<Vec<Piece>, Unfinished> {
    let Connections {
        own,
        extra,
        spare,
        min_part,
    } = connections;
    let extra = Mutex::new(extra);
    // When any connection's place is held back, the whole file steps back.
    let held = || {
        own.is_some_and(Place::is_held)
            || extra.lock().is_ok_and(|extra| {
                extra
                    .iter()
                    .any(|part| part.place.as_ref().is_some_and(Place::is_held))
            })
    };
    let spans: Vec<Span> = parts
        .iter()
        .map(|part| Span {
            start: part.start,
            at: part.at(),
            end: part.end,
        })
        .collect();
    let spans = Mutex::new(spans);
    let progress = Progress {
        id: run.item.id,
        done: AtomicU64::new(done_before),
        reported: Mutex::new(Instant::now()),
        sink: run.sink,
        spans: show_parts.then_some(&spans),
    };
    progress.report_parts();
    let mut pieces = Vec::new();
    let mut waiting = VecDeque::new();
    for (index, part) in parts.into_iter().enumerate() {
        if part.is_done() {
            pieces.extend(part.pieces);
        } else {
            waiting.push_back((index, part));
        }
    }
    let can_split = || {
        spans
            .lock()
            .is_ok_and(|spans| split_point(&spans, min_part).is_some())
    };
    let mut running = FuturesUnordered::new();
    loop {
        // Parts run within this task, so nothing moves while this looks. A connection
        // given back is not taken again in the same round, so this always ends.
        let mut gave_back = false;
        loop {
            let connections = 1 + extra.lock().map_or(0, |extra| extra.len());
            if running.len() < connections {
                let next = waiting.pop_front().or_else(|| {
                    join_late(run, &spans, min_part, recorder).inspect(|_| progress.report_parts())
                });
                if let Some((index, part)) = next {
                    running.push(copy_part(
                        run, index, part, &spans, &held, &progress, recorder,
                    ));
                    continue;
                }
                // Nothing for this connection: an extra one goes back to the job.
                if !running.is_empty()
                    && let Ok(mut extra) = extra.lock()
                    && extra.pop().is_some()
                {
                    gave_back = true;
                    continue;
                }
            } else if !gave_back
                && waiting.is_empty()
                && can_split()
                && let Some(spare) = spare
                && spare.nobody_waiting()
                && let Some(more) = spare.take(1).pop()
                && let Ok(mut extra) = extra.lock()
            {
                extra.push(more);
                continue;
            }
            break;
        }
        if running.is_empty() {
            break;
        }
        tokio::select! {
            ended = running.next() => match ended {
                Some(Ok(part)) => pieces.extend(part.pieces),
                // One part failing (or the user ending the copy) ends them all: dropping
                // the others stops them where they are (each recorded how far it got).
                Some(Err(unfinished)) => return Err(unfinished),
                None => {}
            },
            () = tokio::time::sleep(JOIN_POLL), if spare.is_some() => {}
        }
    }
    let done = progress.done.load(Ordering::Relaxed);
    run.sink.as_ref()(TransferEvent::Progress {
        id: run.item.id,
        done,
    });
    progress.report_parts();
    Ok(pieces)
}

/// Halve the part with the most left (see [`split_point`]) for a connection joining
/// late: its end moves back to the middle of what is left, and a new part copies from
/// there to where it ended.
///
/// # Returns
///
/// The new part and its number, or `None` if no part is worth halving
fn join_late(
    run: &Copying<'_>,
    spans: &Mutex<Vec<Span>>,
    min_part: u64,
    recorder: Option<&Recorder<'_>>,
) -> Option<(usize, Part)> {
    let mut spans = spans.lock().ok()?;
    let (index, at) = split_point(&spans, min_part)?;
    let end = std::mem::replace(&mut spans[index].end, at);
    spans.push(Span { start: at, at, end });
    if let Some(recorder) = recorder {
        recorder.split(index, at);
    }
    tracing::info!(
        "another connection joins in copying {}: {at}..{end} becomes a part of its own",
        run.item.source
    );
    Some((spans.len() - 1, Part::new((at, end))))
}

/// Copy the `bad` pieces of the file again, one after the other over the file's own
/// connection (the extra ones were given back), replacing them in `pieces`.
///
/// # Errors
///
/// Returns why the copy did not complete
pub(super) async fn copy_again(
    run: &Copying<'_>,
    bad: &[Piece],
    pieces: &mut Vec<Piece>,
    place: Option<&Place>,
) -> Result<(), Unfinished> {
    for piece in bad {
        let range = (piece.start, piece.end);
        let done_before = run.item.size - (piece.end - piece.start);
        tracing::warn!(
            "copying {}..{} of {} again",
            piece.start,
            piece.end,
            run.item.source
        );
        let part = vec![Part::new(range)];
        let own = Connections::own(place);
        let fresh = copy_parts(run, part, own, done_before, None, false).await?;
        pieces.retain(|kept| (kept.start, kept.end) != range);
        pieces.extend(fresh);
    }
    Ok(())
}

/// Check the copy in parts: hash each piece's range of `copy` where it is, and compare
/// it with what was hashed while copying (see the module's documentation). The pieces
/// are checked all at once; the user stopping the file (or the job) ends the check,
/// but pausing it does not: the copy is complete by then, and kept only once checked.
///
/// # Returns
///
/// The pieces that do not match (none when the copy is right)
///
/// # Errors
///
/// Returns why the copy could not be checked, or that the user ended it
pub(super) async fn check_pieces(
    item: &PlanItem,
    dst: &Endpoint,
    copy: &SidePath,
    pieces: &[Piece],
    cancel: &CancelFlag,
    sink: &Sink,
) -> Result<Vec<Piece>, Unfinished> {
    let started = Instant::now();
    sink(TransferEvent::Verifying { id: item.id });
    let shared = Arc::new(AtomicU64::new(0));
    let read = &shared;
    let mut checking: FuturesUnordered<_> = pieces
        .iter()
        .map(|piece| async move {
            let found = dst.range_sha256(copy, piece.start, piece.end, read).await;
            (piece, found)
        })
        .collect();
    let mut bad = Vec::new();
    let checked = loop {
        if cancel.is_cancelled() || cancel.is_item_stopped(item.id) {
            break Err(ended(cancel, item, 0));
        }
        match tokio::time::timeout(PROGRESS_INTERVAL, checking.next()).await {
            // Still checking: show how much was read (nothing, if the server computes).
            Err(_) => {
                let done = read.load(Ordering::Relaxed);
                if done > 0 {
                    sink(TransferEvent::VerifyProgress { id: item.id, done });
                }
            }
            Ok(None) => break Ok(()),
            Ok(Some((piece, Ok(found)))) if found == piece.sha256 => {}
            Ok(Some((piece, Ok(found)))) => {
                tracing::warn!(
                    "{}..{} of {} differs from what was copied: SHA-256 {found}, copied {}",
                    piece.start,
                    piece.end,
                    item.dest,
                    piece.sha256
                );
                bad.push(piece.clone());
            }
            Ok(Some((_, Err(error)))) => break Err(Unfinished::Partway(error)),
        }
    };
    sink(TransferEvent::PartsChecked { id: item.id });
    checked?;
    if bad.is_empty() {
        let mut sorted: Vec<&Piece> = pieces.iter().collect();
        sorted.sort_by_key(|piece| piece.start);
        tracing::info!(
            "verified {} in {} pieces ({:.1} s): {}",
            item.dest,
            pieces.len(),
            started.elapsed().as_secs_f64(),
            sorted
                .iter()
                .map(|piece| format!("{}..{} SHA-256 {}", piece.start, piece.end, piece.sha256))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(bad)
}

/// Copy what is left of `part` (number `index` of the file's), hashing the bytes as
/// they go into a new piece, and recording how far it got every [`RECORD_INTERVAL`]
/// and when it ends. Its end is read from `spans` as it goes: a connection joining late
/// may take its second half. A connection lost partway is tried again from where the
/// part had got to, as whole files are ([`PARTWAY_TRIES`]).
///
/// # Returns
///
/// The part, done (to where it ended up ending)
async fn copy_part(
    run: &Copying<'_>,
    index: usize,
    part: Part,
    spans: &Mutex<Vec<Span>>,
    held: &(dyn Fn() -> bool + Sync),
    progress: &Progress<'_>,
    recorder: Option<&Recorder<'_>>,
) -> Result<Part, Unfinished> {
    let from = part.at();
    let mut hasher = Sha256::new();
    let mut at = from;
    let record = |at: u64, hasher: &Sha256| {
        if let Some(recorder) = recorder {
            recorder.reached(index, &part.with_piece(from, at, hasher));
        }
    };
    let mut tries = 0;
    loop {
        let stretch = copy_stretch(
            run,
            &mut at,
            index,
            spans,
            &mut hasher,
            held,
            progress,
            &record,
        );
        match stretch.await {
            Ok(()) => {
                record(at, &hasher);
                let mut done = part.with_piece(from, at, &hasher);
                done.end = at;
                return Ok(done);
            }
            Err(Unfinished::Partway(error)) if tries < PARTWAY_TRIES => {
                tries += 1;
                tracing::warn!(
                    "{error}; continuing the part from {} at {at} ({tries} of {PARTWAY_TRIES})",
                    part.start
                );
                let pause = tokio::time::sleep(PARTWAY_PAUSE);
                if run.cancel.unless_ended(run.item.id, pause).await.is_none() {
                    record(at, &hasher);
                    return Err(ended(run.cancel, run.item, 0));
                }
            }
            Err(unfinished) => {
                record(at, &hasher);
                return Err(unfinished);
            }
        }
    }
}

/// Copy from `*at` to part `index`'s end (read from `spans` before each chunk) over
/// one connection; `*at`, its span and `hasher` move along with every chunk written,
/// so a retry continues after them. `record` is told how far it got every
/// [`RECORD_INTERVAL`].
#[allow(clippy::too_many_arguments)]
async fn copy_stretch(
    run: &Copying<'_>,
    at: &mut u64,
    index: usize,
    spans: &Mutex<Vec<Span>>,
    hasher: &mut Sha256,
    held: &(dyn Fn() -> bool + Sync),
    progress: &Progress<'_>,
    record: &(dyn Fn(u64, &Sha256) + Sync),
) -> Result<(), Unfinished> {
    let Copying {
        item,
        src,
        dst,
        copy,
        cancel,
        ..
    } = *run;
    let mut reader = cancel
        .unless_ended(item.id, src.open_at(&item.source, *at))
        .await
        .ok_or_else(|| ended(cancel, item, 0))?
        .map_err(Unfinished::Partway)?;
    let mut writer = cancel
        .unless_ended(item.id, dst.open_write_range(copy, *at))
        .await
        .ok_or_else(|| ended(cancel, item, 0))?
        .map_err(Unfinished::Partway)?;
    let mut buffer = vec![0; CHUNK_BYTES];
    let mut recorded = Instant::now();
    loop {
        // A poisoned lock (a panic elsewhere) ends the part here; the check that the
        // pieces cover the file then fails the copy rather than put it in place.
        let end = spans
            .lock()
            .ok()
            .and_then(|spans| spans.get(index).map(|span| span.end))
            .unwrap_or(*at);
        if *at >= end {
            break;
        }
        // Paused or held back, the copy is kept and continued from its record; stopped
        // or cancelled, it is deleted (unless the whole job was paused).
        if cancel.is_cancelled() {
            return Err(ItemOutcome::Cancelled.into());
        }
        if cancel.is_item_stopped(item.id) {
            return Err(ItemOutcome::Stopped.into());
        }
        if cancel.is_item_paused(item.id) {
            return Err(Unfinished::End(FileEnd::Paused(0)));
        }
        if held() {
            return Err(Unfinished::End(FileEnd::Held(0)));
        }
        // Only up to the end of this part; usize, as a chunk is small.
        let wanted = usize::try_from(end - *at).map_or(CHUNK_BYTES, |left| left.min(CHUNK_BYTES));
        let read = match cancel
            .unless_ended(item.id, reader.read(&mut buffer[..wanted]))
            .await
        {
            None => continue,
            Some(Ok(0)) => {
                let error = crate::tr!("error-source-ended-early", path = item.source.to_string());
                return Err(Unfinished::Partway(error));
            }
            Some(Ok(read)) => read,
            Some(Err(error)) => {
                return Err(Unfinished::Partway(crate::i18n::cannot(
                    "read",
                    &item.source,
                    &error,
                )));
            }
        };
        match cancel
            .unless_ended(item.id, writer.write_all(&buffer[..read]))
            .await
        {
            None => continue,
            Some(written) => written
                .map_err(|error| Unfinished::Partway(crate::i18n::cannot("write", copy, &error)))?,
        }
        hasher.update(&buffer[..read]);
        *at += read as u64;
        if let Ok(mut spans) = spans.lock()
            && let Some(span) = spans.get_mut(index)
        {
            span.at = *at;
        }
        progress.add(read as u64);
        if recorded.elapsed() >= RECORD_INTERVAL {
            // Written, though perhaps not yet on disk: a record a little behind is safe
            // (those bytes are copied again), and the check catches the rest.
            record(*at, hasher);
            recorded = Instant::now();
        }
    }
    // Stops reading here, before the source's end: for FTP that closes the connection,
    // as the server is still sending the rest of the file.
    drop(reader);
    writer
        .shutdown()
        .await
        .map_err(|error| Unfinished::Partway(crate::i18n::cannot("finish", copy, &error)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    /// A file of `size` bytes to copy from `src` to `dst` on this computer.
    fn item(src: &std::path::Path, dst: &std::path::Path, size: u64) -> PlanItem {
        PlanItem {
            id: 0,
            parent: None,
            name: "file".to_owned(),
            kind: crate::transfer::ItemKind::File,
            size,
            source: SidePath::Local(src.to_path_buf()),
            dest: SidePath::Local(dst.to_path_buf()),
            existing: None,
            modified: None,
            existing_is_dir: false,
            partial: 0,
            mode: None,
            is_link: false,
        }
    }

    /// Bytes that differ along the file, so a part in the wrong place shows.
    fn content(size: usize) -> Vec<u8> {
        (0..size).map(|at| (at % 251) as u8).collect()
    }

    /// A sink that keeps the events.
    fn keeping() -> (Sink, Arc<Mutex<Vec<TransferEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&events);
        let sink: Sink = Arc::new(move |event| {
            if let Ok(mut events) = kept.lock() {
                events.push(event);
            }
        });
        (sink, events)
    }

    /// The SHA-256 of `bytes`.
    fn sha256(bytes: &[u8]) -> Checksum {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Checksum::from_hasher(hasher)
    }

    /// A piece `start..end` of `whole`, as copying it would have hashed it.
    fn piece_of(whole: &[u8], start: u64, end: u64) -> Piece {
        let range = usize::try_from(start).expect("small")..usize::try_from(end).expect("small");
        Piece {
            start,
            end,
            sha256: sha256(&whole[range]),
        }
    }

    /// A file of `size` bytes here, to be copied into `dst.fileparts` beside it.
    struct Setup {
        _temp: tempfile::TempDir,
        item: PlanItem,
        src: std::path::PathBuf,
        copy: SidePath,
        whole: Vec<u8>,
    }

    impl Setup {
        fn new(size: usize) -> Self {
            let temp = tempfile::tempdir().expect("temp dir");
            let whole = content(size);
            let (src, dst) = (temp.path().join("src"), temp.path().join("dst"));
            std::fs::write(&src, &whole).expect("write");
            Self {
                item: item(&src, &dst, whole.len() as u64),
                copy: SidePath::Local(temp.path().join("dst.fileparts")),
                _temp: temp,
                src,
                whole,
            }
        }

        fn run<'a>(&'a self, cancel: &'a CancelFlag, sink: &'a Sink) -> Copying<'a> {
            Copying {
                item: &self.item,
                src: &Endpoint::Local,
                dst: &Endpoint::Local,
                copy: &self.copy,
                cancel,
                sink,
            }
        }

        /// Copy it afresh in `parts`, all at once.
        async fn copy_in(
            &self,
            parts: usize,
            recorder: Option<&Recorder<'_>>,
        ) -> Result<Vec<Piece>, Unfinished> {
            let (cancel, (sink, _)) = (CancelFlag::default(), keeping());
            let parts = ranges(self.item.size, parts)
                .into_iter()
                .map(Part::new)
                .collect();
            let run = self.run(&cancel, &sink);
            let copied = copy_in_parts(&run, parts, true, connections(4), recorder).await;
            copied.map(|(_, pieces)| pieces)
        }

        /// The copy's bytes.
        fn copied(&self) -> Vec<u8> {
            let SidePath::Local(path) = &self.copy else {
                unreachable!("a local path")
            };
            std::fs::read(path).expect("read")
        }

        /// Change one byte of the copy, at `at`.
        fn damage(&self, at: usize) {
            let SidePath::Local(path) = &self.copy else {
                unreachable!("a local path")
            };
            let mut bytes = std::fs::read(path).expect("read");
            bytes[at] ^= 0xff;
            std::fs::write(path, bytes).expect("write");
        }
    }

    #[tokio::test]
    async fn parts_copied_at_once_make_the_file_and_hash_their_ranges() {
        // Arrange: a file not evenly divisible by its 3 parts.
        let setup = Setup::new(1_000_003);
        let (cancel, (sink, events)) = (CancelFlag::default(), keeping());
        let ranges = ranges(setup.item.size, 3);
        let parts = ranges.iter().copied().map(Part::new).collect();

        // Act
        let run = setup.run(&cancel, &sink);
        let Ok((_, pieces)) = copy_in_parts(&run, parts, true, connections(3), None).await else {
            panic!("the copy in parts failed")
        };

        // Assert: the bytes, each piece's checksum, and the parts announced.
        assert_eq!(setup.copied(), setup.whole);
        assert!(cover_the_file(&pieces, setup.item.size).is_ok());
        for piece in &pieces {
            assert_eq!(*piece, piece_of(&setup.whole, piece.start, piece.end));
        }
        let announced = events
            .lock()
            .expect("events")
            .iter()
            .find_map(|event| match event {
                TransferEvent::Parts { parts, .. } => {
                    Some(parts.iter().map(|part| (part.start, part.end)).collect())
                }
                _ => None,
            });
        assert_eq!(announced, Some(ranges));
    }

    #[tokio::test]
    async fn a_stopped_file_ends_every_part() {
        let setup = Setup::new(300_000);
        let (cancel, (sink, _)) = (CancelFlag::default(), keeping());
        cancel.stop_item(setup.item.id);
        let parts = ranges(setup.item.size, 4)
            .into_iter()
            .map(Part::new)
            .collect();

        let run = setup.run(&cancel, &sink);
        let copied = copy_in_parts(&run, parts, true, connections(4), None).await;

        assert!(matches!(
            copied,
            Err(Unfinished::End(FileEnd::Outcome(ItemOutcome::Stopped)))
        ));
    }

    #[tokio::test]
    async fn the_record_follows_the_parts_to_the_end() {
        let setup = Setup::new(400_000);
        let records = SplitRecords::in_memory();
        let parts: Vec<Part> = ranges(setup.item.size, 2)
            .into_iter()
            .map(Part::new)
            .collect();
        let recorder = Recorder::new(&records, &setup.item, &parts);
        let key = record::key(&setup.item);
        let at_first = records.get(&key);

        let copied = setup.copy_in(2, Some(&recorder)).await;

        assert!(copied.is_ok());
        assert_eq!(at_first.map(|record| record.parts), Some(parts));
        let recorded = records.get(&key).expect("recorded").parts;
        assert!(recorded.iter().all(Part::is_done), "{recorded:?}");
        assert_eq!(
            recorded[1].pieces,
            [piece_of(&setup.whole, 200_000, 400_000)]
        );
        recorder.forget();
        assert!(records.get(&key).is_none());
    }

    #[tokio::test]
    async fn a_continued_copy_copies_only_what_was_not_done() {
        // Arrange: copied once; then recorded as if the first part were done and the
        // second half done. A byte of the source changed early in the first part
        // (outside what continuing compares) shows whether it is copied again.
        let mut setup = Setup::new(600_000);
        assert!(setup.copy_in(3, None).await.is_ok());
        let parts = vec![
            Part {
                start: 0,
                end: 200_000,
                pieces: vec![piece_of(&setup.whole, 0, 200_000)],
            },
            Part {
                start: 200_000,
                end: 400_000,
                pieces: vec![piece_of(&setup.whole, 200_000, 300_000)],
            },
            Part::new((400_000, 600_000)),
        ];
        setup.whole[10] ^= 0xff;
        std::fs::write(&setup.src, &setup.whole).expect("write");
        setup.damage(350_000);
        setup.damage(500_000);
        let (cancel, (sink, events)) = (CancelFlag::default(), keeping());

        // Act: over one connection, fewer than the parts left.
        let run = setup.run(&cancel, &sink);
        let Ok((_, pieces)) = copy_in_parts(&run, parts, false, connections(1), None).await else {
            panic!("the continued copy failed")
        };

        // Assert: the first part untouched, the rest copied, in 4 pieces.
        let copied = setup.copied();
        assert_eq!(
            copied[10],
            setup.whole[10] ^ 0xff,
            "the done part was not copied"
        );
        assert_eq!(copied[11..], setup.whole[11..]);
        assert!(cover_the_file(&pieces, setup.item.size).is_ok());
        assert_eq!(pieces.len(), 4);
        assert!(pieces.contains(&piece_of(&setup.whole, 300_000, 400_000)));
        let resumed = events
            .lock()
            .expect("events")
            .iter()
            .find_map(|event| match event {
                TransferEvent::Resumed { from, .. } => Some(*from),
                _ => None,
            });
        assert_eq!(resumed, Some(300_000));
    }

    #[tokio::test]
    async fn a_part_whose_last_bytes_differ_from_the_source_starts_over() {
        let setup = Setup::new(400_000);
        assert!(setup.copy_in(2, None).await.is_ok());
        let parts = vec![
            Part {
                start: 0,
                end: 200_000,
                pieces: vec![piece_of(&setup.whole, 0, 150_000)],
            },
            Part {
                start: 200_000,
                end: 400_000,
                pieces: vec![piece_of(&setup.whole, 200_000, 300_000)],
            },
        ];
        // Just before where the second part got to: continuing compares that.
        setup.damage(299_000);
        let (cancel, (sink, _)) = (CancelFlag::default(), keeping());

        let run = setup.run(&cancel, &sink);
        let Ok((_, pieces)) = copy_in_parts(&run, parts, false, connections(2), None).await else {
            panic!("the continued copy failed")
        };

        assert_eq!(setup.copied(), setup.whole);
        assert!(
            pieces.contains(&piece_of(&setup.whole, 200_000, 400_000)),
            "{pieces:?}"
        );
        assert!(
            pieces.contains(&piece_of(&setup.whole, 0, 150_000)),
            "kept: {pieces:?}"
        );
    }

    /// A file of `size` bytes copied in `parts` here, and its pieces.
    async fn copied_in_parts(size: usize, parts: usize) -> (Setup, Vec<Piece>) {
        let setup = Setup::new(size);
        let Ok(pieces) = setup.copy_in(parts, None).await else {
            panic!("the copy in parts failed")
        };
        (setup, pieces)
    }

    #[tokio::test]
    async fn a_right_copy_passes_its_check_and_the_check_shows() {
        let (setup, pieces) = copied_in_parts(500_001, 3).await;
        let (sink, events) = keeping();
        let (item, copy) = (&setup.item, &setup.copy);

        let cancel = CancelFlag::default();
        let checked = check_pieces(item, &Endpoint::Local, copy, &pieces, &cancel, &sink).await;

        assert!(matches!(checked, Ok(bad) if bad.is_empty()));
        let events = events.lock().expect("events");
        assert!(matches!(
            events.first(),
            Some(TransferEvent::Verifying { .. })
        ));
        assert!(matches!(
            events.last(),
            Some(TransferEvent::PartsChecked { .. })
        ));
    }

    #[tokio::test]
    async fn only_the_piece_holding_a_changed_byte_fails_and_copying_it_again_mends_it() {
        // Arrange: a byte changed in the copy's second part, after copying.
        let (setup, mut pieces) = copied_in_parts(600_000, 3).await;
        setup.damage(250_000);
        let (cancel, (sink, _)) = (CancelFlag::default(), keeping());
        let (item, copy) = (&setup.item, &setup.copy);

        // Act: check, copy what differs again, and check again.
        let local = &Endpoint::Local;
        let Ok(bad) = check_pieces(item, local, copy, &pieces, &cancel, &sink).await else {
            panic!("the check failed")
        };
        let run = setup.run(&cancel, &sink);
        assert!(copy_again(&run, &bad, &mut pieces, None).await.is_ok());
        let rechecked = check_pieces(item, local, copy, &pieces, &cancel, &sink).await;

        // Assert
        let found: Vec<(u64, u64)> = bad.iter().map(|piece| (piece.start, piece.end)).collect();
        assert_eq!(found, [(200_000, 400_000)]);
        assert!(matches!(rechecked, Ok(still) if still.is_empty()));
        assert!(cover_the_file(&pieces, item.size).is_ok());
        assert_eq!(setup.copied(), setup.whole);
    }

    #[tokio::test]
    async fn stopping_the_file_ends_its_check_but_pausing_it_does_not() {
        let (setup, pieces) = copied_in_parts(300_000, 2).await;
        let (sink, _) = keeping();
        let (item, copy) = (&setup.item, &setup.copy);
        let paused = CancelFlag::default();
        paused.pause_item(item.id);
        let stopped = CancelFlag::default();
        stopped.stop_item(item.id);

        let when_paused = check_pieces(item, &Endpoint::Local, copy, &pieces, &paused, &sink);
        let when_stopped = check_pieces(item, &Endpoint::Local, copy, &pieces, &stopped, &sink);

        assert!(matches!(when_paused.await, Ok(bad) if bad.is_empty()));
        assert!(matches!(
            when_stopped.await,
            Err(Unfinished::End(FileEnd::Outcome(ItemOutcome::Stopped)))
        ));
    }

    /// `count` connections (the file's own and extra ones), joining none late.
    fn connections(count: usize) -> Connections<'static> {
        Connections {
            extra: extra_parts(count - 1),
            ..Connections::own(None)
        }
    }

    /// `count` extra connections, from a limit of their own.
    fn extra_parts(count: usize) -> Vec<ExtraPart> {
        let slots = Arc::new(tokio::sync::Semaphore::new(count));
        (0..count)
            .map(|_| ExtraPart {
                _slot: Arc::clone(&slots).try_acquire_owned().expect("a slot"),
                place: None,
            })
            .collect()
    }

    /// Room to spare in a job with `free` slots and `waiting` files waiting for one.
    fn spare(free: usize, waiting: usize) -> Spare {
        Spare {
            slots: Arc::new(tokio::sync::Semaphore::new(free)),
            ticket: None,
            records: Arc::new(SplitRecords::in_memory()),
            files_waiting: Arc::new(std::sync::atomic::AtomicUsize::new(waiting)),
        }
    }

    /// Copy `setup`'s file as one part not started, over `connections`.
    /// Copy the file in one part over `connections`.
    ///
    /// # Returns
    ///
    /// The pieces, and where the parts were each time they were reported
    async fn one_part_over(
        setup: &Setup,
        connections: Connections<'_>,
    ) -> (Vec<Piece>, Vec<Vec<PartProgress>>) {
        let (cancel, (sink, events)) = (CancelFlag::default(), keeping());
        let run = setup.run(&cancel, &sink);
        let part = vec![Part::new((0, setup.item.size))];
        let Ok((_, pieces)) = copy_in_parts(&run, part, true, connections, None).await else {
            panic!("the copy in parts failed")
        };
        assert_eq!(setup.copied(), setup.whole);
        assert!(cover_the_file(&pieces, setup.item.size).is_ok());
        let reported = events
            .lock()
            .expect("events")
            .iter()
            .filter_map(|event| match event {
                TransferEvent::Parts { parts, .. } => Some(parts.clone()),
                _ => None,
            })
            .collect();
        (pieces, reported)
    }

    #[tokio::test]
    async fn a_free_connection_takes_the_second_half_of_the_part_with_most_left() {
        let setup = Setup::new(1_000_000);
        let connections = Connections {
            extra: extra_parts(1),
            min_part: 1,
            ..Connections::own(None)
        };

        let (pieces, reported) = one_part_over(&setup, connections).await;

        let mut found: Vec<(u64, u64)> = pieces
            .iter()
            .map(|piece| (piece.start, piece.end))
            .collect();
        found.sort_unstable();
        assert_eq!(found, [(0, 500_000), (500_000, 1_000_000)]);
        // Shown as one part, then two as it splits (before any byte moves), both done
        // at the end.
        let part = |start, at, end| PartProgress { start, at, end };
        assert_eq!(reported.first(), Some(&vec![part(0, 0, 1_000_000)]));
        let split = vec![part(0, 0, 500_000), part(500_000, 500_000, 1_000_000)];
        assert_eq!(reported.get(1), Some(&split), "{reported:?}");
        let done = vec![
            part(0, 500_000, 500_000),
            part(500_000, 1_000_000, 1_000_000),
        ];
        assert_eq!(reported.last(), Some(&done));
    }

    #[tokio::test]
    async fn a_connection_freed_in_the_job_joins_and_is_given_back() {
        let setup = Setup::new(1_000_000);
        let spare = spare(1, 0);
        let connections = Connections {
            spare: Some(&spare),
            min_part: 1,
            ..Connections::own(None)
        };

        let (pieces, _) = one_part_over(&setup, connections).await;

        assert_eq!(pieces.len(), 2, "{pieces:?}");
        assert_eq!(spare.slots.available_permits(), 1, "given back");
    }

    #[tokio::test]
    async fn whole_files_waiting_go_before_joining_late() {
        let setup = Setup::new(1_000_000);
        let spare = spare(1, 1);
        let connections = Connections {
            spare: Some(&spare),
            min_part: 1,
            ..Connections::own(None)
        };

        let (pieces, _) = one_part_over(&setup, connections).await;

        assert_eq!(pieces.len(), 1, "{pieces:?}");
    }

    #[test]
    fn the_part_with_most_left_is_halved_only_into_halves_large_enough() {
        let span = |at, end| Span { start: at, at, end };
        let spans = [span(0, 100), span(150, 1000), span(1000, 1000)];
        let chunk = CHUNK_BYTES as u64;

        assert_eq!(split_point(&spans, 1), None, "never under a chunk");
        let big = [span(0, 10 * chunk), span(10 * chunk, 30 * chunk)];
        assert_eq!(split_point(&big, 1), Some((1, 20 * chunk)));
        assert_eq!(split_point(&big, 10 * chunk), Some((1, 20 * chunk)));
        assert_eq!(split_point(&big, 10 * chunk + 1), None);
        assert_eq!(split_point(&[], 1), None);
    }

    #[test]
    fn parts_are_used_from_ftp_and_for_uploads_to_sftp() {
        use PartsHost::{Ftp, Sftp, ThisComputer};
        assert!(faster_in_parts(Ftp, ThisComputer));
        assert!(faster_in_parts(Ftp, Sftp));
        assert!(faster_in_parts(ThisComputer, Sftp));
        // An FTP server cannot write a part at any position.
        assert!(!faster_in_parts(ThisComputer, Ftp));
        assert!(!faster_in_parts(Ftp, Ftp));
        // Not measured yet.
        assert!(!faster_in_parts(Sftp, ThisComputer));
        assert!(!faster_in_parts(Sftp, Sftp));
        assert!(!faster_in_parts(ThisComputer, ThisComputer));
    }

    #[test]
    fn small_files_stay_whole_and_parts_stay_large() {
        assert_eq!(parts_for(GIB - 1, 4), 1);
        assert_eq!(parts_for(GIB, 4), 4);
        assert_eq!(parts_for(GIB, 10), 4, "no part under 256 MiB");
        assert_eq!(parts_for(12 * GIB, 4), 4);
        assert_eq!(parts_for(12 * GIB, 1), 1);
        assert_eq!(parts_for(12 * GIB, 0), 1);
    }

    #[test]
    fn pieces_must_cover_the_file_exactly() {
        let sha256 = Checksum::from_hasher(Sha256::new());
        let piece = |start, end| Piece { start, end, sha256 };
        let whole = [piece(5, 10), piece(0, 5)];
        assert!(cover_the_file(&whole, 10).is_ok(), "in any order");
        assert!(
            cover_the_file(&[piece(0, 4), piece(5, 10)], 10).is_err(),
            "a gap"
        );
        assert!(
            cover_the_file(&[piece(0, 6), piece(5, 10)], 10).is_err(),
            "an overlap"
        );
        assert!(cover_the_file(&[piece(0, 5)], 10).is_err(), "short");
    }

    #[test]
    fn ranges_cover_the_file_without_gaps() {
        for (size, parts) in [(10, 3), (12_376_578_944, 4), (7, 1), (5, 5)] {
            let ranges = ranges(size, parts);
            assert_eq!(ranges.len(), parts);
            assert_eq!(ranges[0].0, 0);
            assert_eq!(ranges.last().map(|range| range.1), Some(size));
            for pair in ranges.windows(2) {
                assert_eq!(pair[0].1, pair[1].0, "{size} in {parts}: {ranges:?}");
            }
        }
    }
}
