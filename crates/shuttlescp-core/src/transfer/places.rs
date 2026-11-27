//! The limit on files copied at once, shared by every transfer job of every session.
//!
//! Each job joins the line and gets a [`Ticket`]; before copying a file it asks for a
//! place with it. A file gets one when the limit has room and no file of an older job is
//! waiting: jobs go oldest first, a job's files in its own order. Otherwise the file is
//! noted as waiting, and the job asks again shortly. A place is given back when it is
//! dropped, once its file's copy has ended.
//!
//! Lowering the limit, or Start now on a job, holds back the files that got their places
//! last: their places say so ([`Place::is_held`]), the copy stops them at the next chunk,
//! and they wait for a place again, to continue from their partial copies.
//!
//! Raising a file copied in parts above the others ([`Ticket::prioritize`]) holds back
//! as many other files as it can use, and reserves the places they free for its parts:
//! until it has taken them, no other file starts.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// The places, the limit, and the files waiting.
#[derive(Debug, Default)]
pub struct FilePlaces {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// Most files copied at once; `None` for no limit.
    limit: Option<usize>,
    /// Places in use, in the order they were given (the last given is last).
    running: Vec<Running>,
    /// Files waiting for a place: their job's position in line, and the item.
    waiting: BTreeSet<(i64, usize)>,
    /// Each ticket's position in line: lower goes first.
    positions: HashMap<u64, i64>,
    /// The next position for a new job (later than all), and for Start now (earlier
    /// than all).
    next_late: i64,
    next_early: i64,
    /// The next ticket and place ids.
    next_ticket: u64,
    next_place: u64,
    /// The file raised above the others, and the places still reserved for its parts.
    priority: Option<Priority>,
}

/// A file raised above the others (see [`Ticket::prioritize`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Priority {
    ticket: u64,
    item: usize,
    /// Places still reserved for its parts: no other file takes one while any are.
    reserved: usize,
}

/// A place in use.
#[derive(Debug)]
struct Running {
    place: u64,
    ticket: u64,
    /// The file it copies (or a part of).
    item: usize,
    /// Asked to step back.
    held: Arc<AtomicBool>,
}

impl State {
    /// Places in use that are not held back.
    fn active(&self) -> usize {
        self.running
            .iter()
            .filter(|running| !running.held.load(Ordering::Relaxed))
            .count()
    }

    /// Hold back the active place given last, other than `ticket`'s.
    ///
    /// # Returns
    ///
    /// Whether there was one
    fn hold_last(&mut self, except: Option<u64>) -> bool {
        self.hold_last_where(|running| Some(running.ticket) != except)
    }

    /// Hold back the active place given last of those `which` picks.
    ///
    /// # Returns
    ///
    /// Whether there was one
    fn hold_last_where(&mut self, which: impl Fn(&Running) -> bool) -> bool {
        let last = self
            .running
            .iter()
            .rev()
            .find(|running| !running.held.load(Ordering::Relaxed) && which(running));
        last.is_some_and(|running| {
            running.held.store(true, Ordering::Relaxed);
            true
        })
    }

    /// Places held for the file raised above the others.
    fn reserved(&self) -> usize {
        self.priority.map_or(0, |priority| priority.reserved)
    }
}

impl FilePlaces {
    /// No job yet, with this limit.
    ///
    /// # Arguments
    ///
    /// * `limit` - Most files copied at once; `None` for no limit
    pub fn new(limit: Option<usize>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                limit,
                ..State::default()
            }),
        })
    }

    /// The state, even if a thread panicked while holding it (it stays consistent:
    /// every change is complete before the lock is let go).
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A job joins the line, after every job already in it.
    pub fn join(self: &Arc<Self>) -> Ticket {
        let mut state = self.state();
        let ticket = state.next_ticket;
        state.next_ticket += 1;
        let position = state.next_late;
        state.next_late += 1;
        state.positions.insert(ticket, position);
        Ticket {
            places: Arc::clone(self),
            id: ticket,
        }
    }

    /// Change the limit. Lowered below the files being copied, the ones that got their
    /// places last are held back.
    ///
    /// # Arguments
    ///
    /// * `limit` - Most files copied at once; `None` for no limit
    pub fn set_limit(&self, limit: Option<usize>) {
        let mut state = self.state();
        state.limit = limit;
        if let Some(limit) = limit {
            while state.active() > limit && state.hold_last(None) {}
        }
    }

    /// The limit, how many files are being copied, and how many wait.
    pub fn counts(&self) -> (Option<usize>, usize, usize) {
        let state = self.state();
        (state.limit, state.running.len(), state.waiting.len())
    }
}

/// A job's place in line, for asking for places. Dropping it (the job ended) takes its
/// waiting files out of the line.
#[derive(Debug)]
pub struct Ticket {
    places: Arc<FilePlaces>,
    id: u64,
}

impl Ticket {
    /// Ask for a place to copy `item`.
    ///
    /// # Returns
    ///
    /// The place, if the limit has room and no older job's file is waiting; otherwise
    /// `None`, and the file waits (ask again later)
    pub fn try_take(&self, item: usize) -> Option<Place> {
        let mut state = self.places.state();
        let position = state.positions.get(&self.id).copied().unwrap_or(i64::MAX);
        state.waiting.remove(&(position, item));
        // A file raised above the others takes its reserved place before anyone in line;
        // until the file held back for it has stopped, it waits out of line.
        if let Some(raised) = state
            .priority
            .filter(|raised| raised.ticket == self.id && raised.item == item && raised.reserved > 0)
        {
            if state
                .limit
                .is_some_and(|limit| state.running.len() >= limit)
            {
                return None;
            }
            let reserved = raised.reserved - 1;
            state.priority = (reserved > 0).then_some(Priority { reserved, ..raised });
            return Some(self.give(&mut state, item));
        }
        let room = state
            .limit
            .is_none_or(|limit| state.running.len() + state.reserved() < limit);
        let older_waiting = state.waiting.range(..(position, 0)).next().is_some();
        // While places are reserved for a file of this job, the job starts no other; its
        // files wait out of line, so they hold up no other job's.
        let own_raised = state
            .priority
            .is_some_and(|raised| raised.ticket == self.id && raised.reserved > 0);
        if own_raised {
            return None;
        }
        if !room || older_waiting {
            state.waiting.insert((position, item));
            return None;
        }
        Some(self.give(&mut state, item))
    }

    /// A place for one more part of `item`, a file already being copied, only if one is
    /// free right now: unlike [`Ticket::try_take`], a part never waits in line, so it
    /// never holds up another job's files. Copying in parts is a way to use idle places,
    /// not a claim on them; except for a file raised above the others, which takes the
    /// places reserved for it.
    pub fn try_take_extra(&self, item: usize) -> Option<Place> {
        let mut state = self.places.state();
        let room = state.limit.is_none_or(|limit| state.running.len() < limit);
        let raised = state
            .priority
            .filter(|raised| raised.ticket == self.id && raised.item == item);
        if let Some(raised) = raised
            && raised.reserved > 0
        {
            if !room {
                // The files held back for it have not stopped yet.
                return None;
            }
            let reserved = raised.reserved - 1;
            state.priority = (reserved > 0).then_some(Priority { reserved, ..raised });
            return Some(self.give(&mut state, item));
        }
        // Anyone waiting at all, this job's own files included, goes first; so do the
        // places reserved for a file raised above the others.
        if !room || !state.waiting.is_empty() || state.reserved() > 0 {
            return None;
        }
        Some(self.give(&mut state, item))
    }

    /// Raise `item` above the others: a file being copied in parts, or one waiting for a
    /// place (held back, or not started). Hold back as many other files as it can use
    /// more connections (no more than the limit can give it), newest first, and reserve
    /// the places they free for it. This job's own other files are held back first, as many
    /// as it lacks free slots for; then, as many as the limit lacks room for, those of
    /// any job. Raising another file ends this one's reservation.
    ///
    /// # Arguments
    ///
    /// * `item` - The file
    /// * `wanted` - How many more connections it can use
    /// * `free_slots` - How many of its job's connections are free now
    ///
    /// # Returns
    ///
    /// How many files were held back
    pub fn prioritize(&self, item: usize, wanted: usize, free_slots: usize) -> usize {
        let mut state = self.places.state();
        // No more than the limit can ever give it, counting the places it has.
        let wanted = match state.limit {
            Some(limit) => {
                let has = state
                    .running
                    .iter()
                    .filter(|running| running.ticket == self.id && running.item == item)
                    .count();
                wanted.min(limit.saturating_sub(has))
            }
            None => wanted,
        };
        let others_of_job = |running: &Running| running.ticket == self.id && running.item != item;
        let mut held = 0;
        for _ in 0..wanted.saturating_sub(free_slots) {
            if !state.hold_last_where(others_of_job) {
                break;
            }
            held += 1;
        }
        if let Some(limit) = state.limit {
            // Room once every file held back has stopped.
            let room = limit.saturating_sub(state.active());
            for _ in 0..wanted.saturating_sub(room) {
                if !state
                    .hold_last_where(|running| !(running.ticket == self.id && running.item == item))
                {
                    break;
                }
                held += 1;
            }
        }
        state.priority = (wanted > 0).then_some(Priority {
            ticket: self.id,
            item,
            reserved: wanted,
        });
        held
    }

    /// `item` needs no more of the places reserved for it (it cannot use them, or its
    /// copy ended): every file goes in its usual order again.
    pub fn end_priority(&self, item: usize) {
        let mut state = self.places.state();
        if state
            .priority
            .is_some_and(|raised| raised.ticket == self.id && raised.item == item)
        {
            state.priority = None;
        }
    }

    /// A new place for this job's `item`, counted as running.
    fn give(&self, state: &mut State, item: usize) -> Place {
        let place = state.next_place;
        state.next_place += 1;
        let held = Arc::new(AtomicBool::new(false));
        state.running.push(Running {
            place,
            ticket: self.id,
            item,
            held: Arc::clone(&held),
        });
        Place {
            places: Arc::clone(&self.places),
            id: place,
            held,
        }
    }

    /// `item` no longer wants a place (stopped, cancelled or paused while waiting).
    pub fn forget(&self, item: usize) {
        let mut state = self.places.state();
        if let Some(&position) = state.positions.get(&self.id) {
            state.waiting.remove(&(position, item));
        }
    }

    /// Start now: the job goes to the front of the line, and if the limit has no room,
    /// the file of another job that got its place last is held back to make some.
    pub fn start_now(&self) {
        let mut state = self.places.state();
        let early = state.next_early - 1;
        state.next_early = early;
        let Some(old) = state.positions.insert(self.id, early) else {
            return;
        };
        let moved: Vec<(i64, usize)> = state
            .waiting
            .iter()
            .filter(|(position, _)| *position == old)
            .copied()
            .collect();
        for (_, item) in moved {
            state.waiting.remove(&(old, item));
            state.waiting.insert((early, item));
        }
        let full = state.limit.is_some_and(|limit| state.active() >= limit);
        if full {
            state.hold_last(Some(self.id));
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let mut state = self.places.state();
        if let Some(position) = state.positions.remove(&self.id) {
            state.waiting.retain(|(waiting, _)| *waiting != position);
        }
        if state
            .priority
            .is_some_and(|raised| raised.ticket == self.id)
        {
            state.priority = None;
        }
    }
}

/// A place to copy one file. Dropping it gives it back.
#[derive(Debug)]
pub struct Place {
    places: Arc<FilePlaces>,
    id: u64,
    held: Arc<AtomicBool>,
}

impl Place {
    /// Asked to step back (the limit was lowered, or another job started now): stop
    /// the file and wait for a place again.
    pub fn is_held(&self) -> bool {
        self.held.load(Ordering::Relaxed)
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let mut state = self.places.state();
        state.running.retain(|running| running.place != self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_jobs_files_wait_behind_the_limit() {
        // Your example: limit 2; job A has 1 file, job B (started after) has 5.
        let places = FilePlaces::new(Some(2));
        let (a, b) = (places.join(), places.join());

        let a1 = a.try_take(0);
        let b: Vec<_> = (0..5).map(|item| b.try_take(item)).collect();

        // A's file and B's first file run; B's other four wait.
        assert!(a1.is_some());
        assert!(b[0].is_some());
        assert!(b[1..].iter().all(Option::is_none));
        assert_eq!(places.counts(), (Some(2), 2, 4));
    }

    #[test]
    fn a_freed_place_goes_to_the_oldest_job_first() {
        let places = FilePlaces::new(Some(1));
        let (a, b) = (places.join(), places.join());
        let a1 = a.try_take(0).expect("room");
        assert!(a.try_take(1).is_none());
        assert!(b.try_take(0).is_none());

        drop(a1);

        // B asks first, but A's waiting file is older.
        assert!(b.try_take(0).is_none());
        assert!(a.try_take(1).is_some());
    }

    #[test]
    fn without_a_limit_every_file_runs() {
        let places = FilePlaces::new(None);
        let job = places.join();
        let taken: Vec<_> = (0..20).filter_map(|item| job.try_take(item)).collect();
        assert_eq!(taken.len(), 20);
    }

    #[test]
    fn lowering_the_limit_holds_back_the_files_started_last() {
        let places = FilePlaces::new(Some(3));
        let (a, b) = (places.join(), places.join());
        let a1 = a.try_take(0).expect("room");
        let b1 = b.try_take(0).expect("room");
        let b2 = b.try_take(1).expect("room");

        places.set_limit(Some(1));

        assert!(!a1.is_held());
        assert!(b1.is_held() && b2.is_held());
        // Until they stop, they keep their places: nothing new starts.
        assert!(a.try_take(1).is_none());
        drop((b1, b2));
        // B's files wait behind A's, the older job.
        assert!(b.try_take(0).is_none());
        drop(a1);
        assert!(a.try_take(1).is_some());
    }

    #[test]
    fn start_now_puts_a_job_first_and_holds_back_another_jobs_last_file() {
        let places = FilePlaces::new(Some(2));
        let (a, b, c) = (places.join(), places.join(), places.join());
        let a1 = a.try_take(0).expect("room");
        let b1 = b.try_take(0).expect("room");
        assert!(c.try_take(0).is_none());
        assert!(a.try_take(1).is_none());

        c.start_now();

        // B's file got its place last: it steps back; A's keeps running.
        assert!(b1.is_held());
        assert!(!a1.is_held());
        drop(b1);
        // C, now first in line, gets the freed place, before A's waiting file.
        assert!(a.try_take(1).is_none());
        let c1 = c.try_take(0);
        assert!(c1.is_some());
        // C's next file, waiting too (a job asks again and again), also goes first.
        assert!(c.try_take(1).is_none());
        drop(a1);
        assert!(a.try_take(1).is_none());
        assert!(c.try_take(1).is_some());
    }

    #[test]
    fn start_now_with_room_holds_nothing_back() {
        let places = FilePlaces::new(Some(2));
        let (a, b) = (places.join(), places.join());
        let a1 = a.try_take(0).expect("room");

        b.start_now();

        assert!(!a1.is_held());
        assert!(b.try_take(0).is_some());
    }

    #[test]
    fn an_ended_job_or_a_forgotten_file_leaves_the_line() {
        let places = FilePlaces::new(Some(1));
        let (a, b, c) = (places.join(), places.join(), places.join());
        let a1 = a.try_take(0).expect("room");
        assert!(b.try_take(0).is_none());
        assert!(b.try_take(1).is_none());
        assert!(c.try_take(0).is_none());

        b.forget(0);
        drop(b);
        drop(a1);

        // Nothing of B's is left in front of C.
        let c1 = c.try_take(0);
        assert!(c1.is_some());
        assert_eq!(places.counts(), (Some(1), 1, 0));
    }

    #[test]
    fn extra_parts_take_only_free_places_and_never_wait() {
        let places = FilePlaces::new(Some(3));
        let (a, b) = (places.join(), places.join());
        let _a1 = a.try_take(0).expect("room");

        // A's large file takes the two free places for its parts...
        let part1 = a.try_take_extra(0).expect("free");
        let _part2 = a.try_take_extra(0).expect("free");
        // ...and with none left, asking leaves nothing waiting behind.
        assert!(a.try_take_extra(0).is_none());
        assert_eq!(places.counts(), (Some(3), 3, 0));

        // A file waiting in line goes before any more parts.
        assert!(b.try_take(0).is_none());
        drop(part1);
        assert!(a.try_take_extra(0).is_none(), "B's file is waiting");
        assert!(b.try_take(0).is_some());
    }

    #[test]
    fn a_raised_file_gets_the_places_of_the_newest_files_and_keeps_them() {
        // Limit 4: A copies one file in two parts; B, a later job, copies two files.
        let places = FilePlaces::new(Some(4));
        let (a, b) = (places.join(), places.join());
        let a0 = a.try_take(0).expect("room");
        let a0_part = a.try_take_extra(0).expect("free");
        let b0 = b.try_take(0).expect("room");
        let b1 = b.try_take(1).expect("room");

        let held = a.prioritize(0, 2, 2);

        // B's two files step back; A's file and its part keep going.
        assert_eq!(held, 2);
        assert!(b0.is_held() && b1.is_held());
        assert!(!a0.is_held() && !a0_part.is_held());
        // Until they stop, nothing is free.
        assert!(a.try_take_extra(0).is_none());
        drop((b0, b1));
        // B's files ask again first, but the places are A's file's.
        assert!(b.try_take(0).is_none());
        assert!(b.try_take(1).is_none());
        let more = [a.try_take_extra(0), a.try_take_extra(0)];
        assert!(more.iter().all(Option::is_some));
        // Reserved no more: B's files wait for places as usual.
        drop(more);
        assert!(b.try_take(0).is_some());
    }

    #[test]
    fn a_raised_file_holds_back_only_as_many_files_as_it_can_use() {
        let places = FilePlaces::new(Some(3));
        let (a, b) = (places.join(), places.join());
        let _a0 = a.try_take(0).expect("room");
        let b0 = b.try_take(0).expect("room");
        let b1 = b.try_take(1).expect("room");

        let held = a.prioritize(0, 1, 1);

        // Only the newest steps back.
        assert_eq!(held, 1);
        assert!(b1.is_held());
        assert!(!b0.is_held());
    }

    #[test]
    fn a_raised_file_first_takes_its_own_jobs_connections() {
        // No limit, but the job has no free connection: its own newest files step back,
        // never the raised one, and the job starts no other file meanwhile.
        let places = FilePlaces::new(None);
        let (a, b) = (places.join(), places.join());
        let a0 = a.try_take(0).expect("room");
        let a1 = a.try_take(1).expect("room");
        let a2 = a.try_take(2).expect("room");
        let b0 = b.try_take(0).expect("room");

        let held = a.prioritize(0, 2, 0);

        assert_eq!(held, 2);
        assert!(a1.is_held() && a2.is_held());
        assert!(!a0.is_held() && !b0.is_held());
        assert!(a.try_take(3).is_none(), "the job waits for its raised file");
        assert!(
            b.try_take(1).is_some(),
            "other jobs, without a limit, go on"
        );
        let _more = [a.try_take_extra(0), a.try_take_extra(0)];
        assert!(a.try_take(3).is_some());
    }

    #[test]
    fn ending_a_raised_files_reservation_frees_its_places() {
        let places = FilePlaces::new(Some(2));
        let (a, b) = (places.join(), places.join());
        let _a0 = a.try_take(0).expect("room");
        let b0 = b.try_take(0).expect("room");
        a.prioritize(0, 1, 1);
        drop(b0);
        assert!(b.try_take(0).is_none(), "reserved");

        // The file could not use it after all (no part left large enough to halve).
        a.end_priority(0);

        assert!(b.try_take(0).is_some());
    }

    #[test]
    fn a_raised_file_reserves_no_more_than_the_limit_can_give() {
        // Limit 2: A's file in parts has one place, B's file the other. A could use 7
        // more, but only one more place exists.
        let places = FilePlaces::new(Some(2));
        let (a, b) = (places.join(), places.join());
        let _a0 = a.try_take(0).expect("room");
        let b0 = b.try_take(0).expect("room");

        let held = a.prioritize(0, 7, 8);

        assert_eq!(held, 1);
        drop(b0);
        let part = a.try_take_extra(0).expect("reserved");
        // Nothing more is reserved: once A's part ends, B's file gets its place back.
        assert!(b.try_take(0).is_none(), "full");
        drop(part);
        assert!(b.try_take(0).is_some());
    }

    #[test]
    fn a_file_waiting_raised_goes_first_and_the_raised_one_steps_back() {
        // Limit 2: A's file in parts, raised, holds both places; B's whole file waits.
        let places = FilePlaces::new(Some(2));
        let (a, b) = (places.join(), places.join());
        let a0 = a.try_take(0).expect("room");
        let a0_part = a.try_take_extra(0).expect("free");
        assert!(b.try_take(0).is_none());

        // Switching back: B's waiting file is raised.
        let held = b.prioritize(0, 1, 1);

        // A's newest place steps back (in a copy, its whole file then steps back).
        assert_eq!(held, 1);
        assert!(a0_part.is_held());
        drop((a0, a0_part));
        // B's file goes first; A's file gets the place left.
        assert!(b.try_take(0).is_some());
        assert!(a.try_take(0).is_some());
    }
}
