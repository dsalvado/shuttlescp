//! Copying one large file in parts: byte ranges copied at the same time, each over a
//! connection of its own, into one copy (`name.fileparts`, see
//! [`super::super::PARTS_SUFFIX`]). Worth it where servers limit each connection's
//! speed (see `Remote::reads_faster_in_parts`).
//!
//! The copy is set to its full size first, and every part writes its range in place
//! ([`Endpoint::open_write_range`]), so nothing is joined afterwards. Each part hashes
//! the bytes it copies, so the copy can be checked range by range without reading the
//! source again.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{
    CHUNK_BYTES, FileEnd, FileTimes, PARTWAY_PAUSE, PARTWAY_TRIES, PROGRESS_INTERVAL, Unfinished,
    ended,
};
use crate::checksum::Checksum;
use crate::path::SidePath;
use crate::transfer::endpoint::Endpoint;
use crate::transfer::{CancelFlag, ItemOutcome, Place, PlanItem, Sink, TransferEvent};

/// Files smaller than this are copied whole.
pub(in crate::transfer) const SPLIT_FROM: u64 = 1 << 30;
/// No part is smaller than this (so a 1 GiB file has at most 4).
pub(super) const MIN_PART: u64 = 256 << 20;

/// A stretch of the copy whose bytes were hashed as they were copied: `start..end`
/// holds bytes with this SHA-256 checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// The file's progress, summed over its parts, reported at most every
/// [`PROGRESS_INTERVAL`].
struct Progress<'a> {
    id: usize,
    done: AtomicU64,
    reported: Mutex<Instant>,
    sink: &'a Sink,
}

impl Progress<'_> {
    fn add(&self, bytes: u64) {
        let done = self.done.fetch_add(bytes, Ordering::Relaxed) + bytes;
        let Ok(mut reported) = self.reported.lock() else {
            return;
        };
        if reported.elapsed() >= PROGRESS_INTERVAL {
            *reported = Instant::now();
            (self.sink)(TransferEvent::Progress { id: self.id, done });
        }
    }
}

/// Copy `item` into `copy` in the parts `ranges`, all at once.
///
/// # Arguments
///
/// * `places` - The places the parts hold in the limit shared by every job; when one is
///   held back, the whole file steps back
///
/// # Returns
///
/// How long it took, and the pieces copied (in no particular order), to check the copy
/// by
///
/// # Errors
///
/// Returns why the copy did not complete. Then the copy holds gaps: whatever ended it,
/// it cannot be continued as a whole-file partial copy (resuming parts comes later).
#[allow(clippy::too_many_arguments)]
pub(super) async fn copy_in_parts(
    item: &PlanItem,
    src: &Endpoint,
    dst: &Endpoint,
    copy: &SidePath,
    ranges: &[(u64, u64)],
    places: &[&Place],
    cancel: &CancelFlag,
    sink: &Sink,
) -> Result<(FileTimes, Vec<Piece>), Unfinished> {
    let opening = Instant::now();
    dst.set_size(copy, item.size)
        .await
        .map_err(ItemOutcome::Failed)?;
    let open = opening.elapsed();
    let moving = Instant::now();
    sink(TransferEvent::InParts {
        id: item.id,
        ranges: ranges.to_vec(),
    });
    tracing::info!(
        "copying {} in {} parts: {}",
        item.source,
        ranges.len(),
        ranges
            .iter()
            .map(|(start, end)| format!("{start}..{end}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let pieces = copy_ranges(item, src, dst, copy, ranges, places, 0, cancel, sink).await?;
    Ok((
        FileTimes {
            open,
            data: moving.elapsed(),
            finish: std::time::Duration::ZERO,
        },
        pieces,
    ))
}

/// Copy the `ranges` of `item` into `copy` (already its full size), all at once.
///
/// # Arguments
///
/// * `done_before` - Bytes of the file already copied, where its progress starts
///
/// # Returns
///
/// The pieces copied, in no particular order
///
/// # Errors
///
/// Returns why the copy did not complete
#[allow(clippy::too_many_arguments)]
async fn copy_ranges(
    item: &PlanItem,
    src: &Endpoint,
    dst: &Endpoint,
    copy: &SidePath,
    ranges: &[(u64, u64)],
    places: &[&Place],
    done_before: u64,
    cancel: &CancelFlag,
    sink: &Sink,
) -> Result<Vec<Piece>, Unfinished> {
    let progress = Progress {
        id: item.id,
        done: AtomicU64::new(done_before),
        reported: Mutex::new(Instant::now()),
        sink,
    };
    let held = || places.iter().any(|place| place.is_held());
    let mut running: FuturesUnordered<_> = ranges
        .iter()
        .map(|&(start, end)| copy_part(item, src, dst, copy, start, end, cancel, &held, &progress))
        .collect();
    let mut pieces = Vec::with_capacity(ranges.len());
    while let Some(part) = running.next().await {
        match part {
            Ok(piece) => pieces.push(piece),
            // One part failing (or the user ending the copy) ends them all: dropping the
            // others stops them where they are.
            Err(unfinished) => return Err(unfinished),
        }
    }
    let done = progress.done.load(Ordering::Relaxed);
    sink(TransferEvent::Progress { id: item.id, done });
    Ok(pieces)
}

/// Copy the `bad` pieces of `item` into `copy` again, one after the other over the
/// file's own connection (the extra ones were given back), replacing them in `pieces`.
///
/// # Errors
///
/// Returns why the copy did not complete
#[allow(clippy::too_many_arguments)]
pub(super) async fn copy_again(
    item: &PlanItem,
    src: &Endpoint,
    dst: &Endpoint,
    copy: &SidePath,
    bad: &[Piece],
    pieces: &mut Vec<Piece>,
    place: Option<&Place>,
    cancel: &CancelFlag,
    sink: &Sink,
) -> Result<(), Unfinished> {
    let places: Vec<&Place> = place.into_iter().collect();
    for piece in bad {
        let range = (piece.start, piece.end);
        let done_before = item.size - (piece.end - piece.start);
        tracing::warn!(
            "copying {}..{} of {} again",
            piece.start,
            piece.end,
            item.source
        );
        let fresh = copy_ranges(
            item,
            src,
            dst,
            copy,
            &[range],
            &places,
            done_before,
            cancel,
            sink,
        )
        .await?;
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

/// Copy the range `start..end` of `item` into `copy`, hashing the bytes as they go.
/// A connection lost partway is tried again from where the part had got to, as whole
/// files are ([`PARTWAY_TRIES`]).
#[allow(clippy::too_many_arguments)]
async fn copy_part(
    item: &PlanItem,
    src: &Endpoint,
    dst: &Endpoint,
    copy: &SidePath,
    start: u64,
    end: u64,
    cancel: &CancelFlag,
    held: &(dyn Fn() -> bool + Sync),
    progress: &Progress<'_>,
) -> Result<Piece, Unfinished> {
    let mut hasher = Sha256::new();
    let mut at = start;
    let mut tries = 0;
    loop {
        let stretch = copy_stretch(
            item,
            src,
            dst,
            copy,
            &mut at,
            end,
            &mut hasher,
            cancel,
            held,
            progress,
        );
        match stretch.await {
            Ok(()) => {
                return Ok(Piece {
                    start,
                    end,
                    sha256: Checksum::from_hasher(hasher),
                });
            }
            Err(Unfinished::Partway(error)) if tries < PARTWAY_TRIES => {
                tries += 1;
                tracing::warn!(
                    "{error}; continuing the part {start}..{end} at {at} ({tries} of \
                     {PARTWAY_TRIES})"
                );
                let pause = tokio::time::sleep(PARTWAY_PAUSE);
                if cancel.unless_ended(item.id, pause).await.is_none() {
                    return Err(ended(cancel, item, 0));
                }
            }
            Err(unfinished) => return Err(unfinished),
        }
    }
}

/// Copy from `*at` to `end` over one connection; `*at` and `hasher` move along with
/// every chunk written, so a retry continues after them.
#[allow(clippy::too_many_arguments)]
async fn copy_stretch(
    item: &PlanItem,
    src: &Endpoint,
    dst: &Endpoint,
    copy: &SidePath,
    at: &mut u64,
    end: u64,
    hasher: &mut Sha256,
    cancel: &CancelFlag,
    held: &(dyn Fn() -> bool + Sync),
    progress: &Progress<'_>,
) -> Result<(), Unfinished> {
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
    while *at < end {
        // However the user ends it, a copy in parts starts over next time (for now).
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
        progress.add(read as u64);
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
    fn keeping() -> (Sink, std::sync::Arc<Mutex<Vec<TransferEvent>>>) {
        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let kept = std::sync::Arc::clone(&events);
        let sink: Sink = std::sync::Arc::new(move |event| {
            if let Ok(mut events) = kept.lock() {
                events.push(event);
            }
        });
        (sink, events)
    }

    #[tokio::test]
    async fn parts_copied_at_once_make_the_file_and_hash_their_ranges() {
        // Arrange: a file not evenly divisible by its 3 parts.
        let temp = tempfile::tempdir().expect("temp dir");
        let whole = content(1_000_003);
        let (src, dst) = (temp.path().join("src"), temp.path().join("dst"));
        std::fs::write(&src, &whole).expect("write");
        let item = item(&src, &dst, whole.len() as u64);
        let copy = SidePath::Local(temp.path().join("dst.fileparts"));
        let ranges = ranges(item.size, 3);
        let (sink, events) = keeping();

        // Act
        let (_, pieces) = copy_in_parts(
            &item,
            &Endpoint::Local,
            &Endpoint::Local,
            &copy,
            &ranges,
            &[],
            &CancelFlag::default(),
            &sink,
        )
        .await
        .unwrap_or_else(|_| panic!("the copy in parts failed"));

        // Assert: the bytes, each piece's checksum, and the parts announced.
        let SidePath::Local(copied) = &copy else {
            unreachable!("a local path")
        };
        assert_eq!(std::fs::read(copied).expect("read"), whole);
        assert!(cover_the_file(&pieces, item.size).is_ok());
        for piece in &pieces {
            let range = usize::try_from(piece.start).expect("small")
                ..usize::try_from(piece.end).expect("small");
            let mut hasher = Sha256::new();
            hasher.update(&whole[range]);
            assert_eq!(piece.sha256, Checksum::from_hasher(hasher));
        }
        let announced = events
            .lock()
            .expect("events")
            .iter()
            .find_map(|event| match event {
                TransferEvent::InParts { ranges, .. } => Some(ranges.clone()),
                _ => None,
            });
        assert_eq!(announced, Some(ranges));
    }

    #[tokio::test]
    async fn a_stopped_file_ends_every_part() {
        let temp = tempfile::tempdir().expect("temp dir");
        let whole = content(300_000);
        let (src, dst) = (temp.path().join("src"), temp.path().join("dst"));
        std::fs::write(&src, &whole).expect("write");
        let item = item(&src, &dst, whole.len() as u64);
        let copy = SidePath::Local(temp.path().join("dst.fileparts"));
        let cancel = CancelFlag::default();
        cancel.stop_item(item.id);
        let (sink, _) = keeping();

        let copied = copy_in_parts(
            &item,
            &Endpoint::Local,
            &Endpoint::Local,
            &copy,
            &ranges(item.size, 4),
            &[],
            &cancel,
            &sink,
        )
        .await;

        assert!(matches!(
            copied,
            Err(Unfinished::End(FileEnd::Outcome(ItemOutcome::Stopped)))
        ));
    }

    /// A file of `size` bytes copied in `parts` here; the source path, the copy, the
    /// file and its pieces.
    async fn copied_in_parts(
        temp: &tempfile::TempDir,
        size: usize,
        parts: usize,
    ) -> (PlanItem, SidePath, Vec<u8>, Vec<Piece>) {
        let whole = content(size);
        let (src, dst) = (temp.path().join("src"), temp.path().join("dst"));
        std::fs::write(&src, &whole).expect("write");
        let item = item(&src, &dst, whole.len() as u64);
        let copy = SidePath::Local(temp.path().join("dst.fileparts"));
        let (sink, _) = keeping();
        let (ranges, cancel) = (ranges(item.size, parts), CancelFlag::default());
        let copied = copy_in_parts(
            &item,
            &Endpoint::Local,
            &Endpoint::Local,
            &copy,
            &ranges,
            &[],
            &cancel,
            &sink,
        );
        let Ok((_, pieces)) = copied.await else {
            panic!("the copy in parts failed")
        };
        (item, copy, whole, pieces)
    }

    /// Change one byte of the local file `copy`, at `at`.
    fn damage(copy: &SidePath, at: usize) {
        let SidePath::Local(path) = copy else {
            unreachable!("a local path")
        };
        let mut bytes = std::fs::read(path).expect("read");
        bytes[at] ^= 0xff;
        std::fs::write(path, bytes).expect("write");
    }

    #[tokio::test]
    async fn a_right_copy_passes_its_check_and_the_check_shows() {
        let temp = tempfile::tempdir().expect("temp dir");
        let (item, copy, _, pieces) = copied_in_parts(&temp, 500_001, 3).await;
        let (sink, events) = keeping();

        let checked = check_pieces(
            &item,
            &Endpoint::Local,
            &copy,
            &pieces,
            &CancelFlag::default(),
            &sink,
        )
        .await;

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
        let temp = tempfile::tempdir().expect("temp dir");
        let (item, copy, whole, mut pieces) = copied_in_parts(&temp, 600_000, 3).await;
        damage(&copy, 250_000);
        let (cancel, (sink, _)) = (CancelFlag::default(), keeping());

        // Act: check, copy what differs again, and check again.
        let check = || check_pieces(&item, &Endpoint::Local, &copy, &pieces, &cancel, &sink);
        let Ok(bad) = check().await else {
            panic!("the check failed")
        };
        let again = copy_again(
            &item,
            &Endpoint::Local,
            &Endpoint::Local,
            &copy,
            &bad,
            &mut pieces,
            None,
            &cancel,
            &sink,
        );
        assert!(again.await.is_ok(), "copying again failed");
        let check = || check_pieces(&item, &Endpoint::Local, &copy, &pieces, &cancel, &sink);
        let rechecked = check().await;

        // Assert
        let found: Vec<(u64, u64)> = bad.iter().map(|piece| (piece.start, piece.end)).collect();
        assert_eq!(found, [(200_000, 400_000)]);
        assert!(matches!(rechecked, Ok(still) if still.is_empty()));
        assert!(cover_the_file(&pieces, item.size).is_ok());
        let SidePath::Local(path) = &copy else {
            unreachable!("a local path")
        };
        assert_eq!(std::fs::read(path).expect("read"), whole);
    }

    #[tokio::test]
    async fn stopping_the_file_ends_its_check_but_pausing_it_does_not() {
        let temp = tempfile::tempdir().expect("temp dir");
        let (item, copy, _, pieces) = copied_in_parts(&temp, 300_000, 2).await;
        let (sink, _) = keeping();
        let paused = CancelFlag::default();
        paused.pause_item(item.id);
        let stopped = CancelFlag::default();
        stopped.stop_item(item.id);

        let when_paused = check_pieces(&item, &Endpoint::Local, &copy, &pieces, &paused, &sink);
        let when_stopped = check_pieces(&item, &Endpoint::Local, &copy, &pieces, &stopped, &sink);

        assert!(matches!(when_paused.await, Ok(bad) if bad.is_empty()));
        assert!(matches!(
            when_stopped.await,
            Err(Unfinished::End(FileEnd::Outcome(ItemOutcome::Stopped)))
        ));
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
