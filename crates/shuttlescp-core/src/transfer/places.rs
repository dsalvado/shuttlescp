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
}

/// A place in use.
#[derive(Debug)]
struct Running {
    place: u64,
    ticket: u64,
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
        let last = self.running.iter().rev().find(|running| {
            !running.held.load(Ordering::Relaxed) && Some(running.ticket) != except
        });
        last.is_some_and(|running| {
            running.held.store(true, Ordering::Relaxed);
            true
        })
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
        let room = state.limit.is_none_or(|limit| state.running.len() < limit);
        let older_waiting = state.waiting.range(..(position, 0)).next().is_some();
        if !room || older_waiting {
            state.waiting.insert((position, item));
            return None;
        }
        let place = state.next_place;
        state.next_place += 1;
        let held = Arc::new(AtomicBool::new(false));
        state.running.push(Running {
            place,
            ticket: self.id,
            held: Arc::clone(&held),
        });
        Some(Place {
            places: Arc::clone(&self.places),
            id: place,
            held,
        })
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
}
