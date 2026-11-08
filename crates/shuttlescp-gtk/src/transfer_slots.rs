//! The limit on transfer jobs running at once, shared by every session.
//!
//! Each session's transfer panel asks here before starting a job. Within the limit the
//! job starts; beyond it, it waits in line (oldest first) until a running job ends.
//! Raising the limit starts waiting jobs; lowering it holds back the jobs started last,
//! which go back to the front of the line and continue first when there is room again.
//! "Start now" puts a job ahead: at the limit, the job started last steps back the same
//! way.
//!
//! This is only the bookkeeping: it says which jobs to start or hold back, and the
//! panels do it.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use relm4::Sender;

use crate::session::SessionId;
use crate::transfer_panel::TransferPanelMsg;

/// A transfer job, wherever it is: the session it belongs to and its id there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobRef {
    /// The session whose transfer panel has the job.
    pub session: SessionId,
    /// The job's id in that panel.
    pub job: u64,
}

/// What a change of the limit asks of the panels.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// Jobs to start (or continue) now.
    pub start: Vec<JobRef>,
    /// Running jobs to hold back: pause them, to continue later.
    pub hold: Vec<JobRef>,
}

/// The running and waiting jobs of every session, and the limit.
#[derive(Debug, Default)]
pub struct Slots {
    /// Most jobs running at once; `None` for no limit.
    limit: Option<usize>,
    /// Jobs running, in the order they started (the last one started is last).
    running: Vec<JobRef>,
    /// Jobs waiting for room, the next to start first.
    waiting: VecDeque<JobRef>,
}

impl Slots {
    /// No job yet, with this limit.
    ///
    /// # Arguments
    ///
    /// * `limit` - Most jobs running at once; `None` for no limit
    pub fn new(limit: Option<usize>) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }

    /// The limit; `None` when there is none.
    pub fn limit(&self) -> Option<usize> {
        self.limit
    }

    /// How many jobs are running.
    pub fn running(&self) -> usize {
        self.running.len()
    }

    /// How many jobs are waiting for room.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Whether `job` is waiting for room.
    #[cfg(test)]
    pub fn is_waiting(&self, job: JobRef) -> bool {
        self.waiting.contains(&job)
    }

    /// A new job wants to run.
    ///
    /// # Arguments
    ///
    /// * `job` - The job
    ///
    /// # Returns
    ///
    /// `true` if it can start now; otherwise it waits in line, and is named in a later
    /// [`Slots::release`] or [`Slots::set_limit`] when its turn comes
    pub fn request(&mut self, job: JobRef) -> bool {
        if self.has_room() {
            self.running.push(job);
            true
        } else {
            self.waiting.push_back(job);
            false
        }
    }

    /// A job no longer needs its place: it ended (done, failed or cancelled), the user
    /// paused it, or it was cancelled while waiting.
    ///
    /// # Arguments
    ///
    /// * `job` - The job
    ///
    /// # Returns
    ///
    /// The waiting jobs that can start now, in the room it leaves
    pub fn release(&mut self, job: JobRef) -> Vec<JobRef> {
        self.running.retain(|running| *running != job);
        self.waiting.retain(|waiting| *waiting != job);
        self.fill()
    }

    /// A job carries on under a new id (a paused job is continued as a new job): it
    /// keeps its place, running or waiting.
    ///
    /// # Arguments
    ///
    /// * `old` - The job's id until now
    /// * `new` - Its id from now on
    pub fn replace(&mut self, old: JobRef, new: JobRef) {
        for place in self.running.iter_mut().chain(self.waiting.iter_mut()) {
            if *place == old {
                *place = new;
            }
        }
    }

    /// A session closed: its jobs give up their places.
    ///
    /// # Arguments
    ///
    /// * `session` - The session
    ///
    /// # Returns
    ///
    /// The waiting jobs (of other sessions) that can start now
    pub fn release_session(&mut self, session: SessionId) -> Vec<JobRef> {
        self.running.retain(|job| job.session != session);
        self.waiting.retain(|job| job.session != session);
        self.fill()
    }

    /// Change the limit.
    ///
    /// # Arguments
    ///
    /// * `limit` - Most jobs running at once; `None` for no limit
    ///
    /// # Returns
    ///
    /// Raised: the waiting jobs to start. Lowered below the jobs running: the ones
    /// started last, to hold back (the last started first); they wait at the front of
    /// the line in the order they started, so the earliest of them continues first.
    pub fn set_limit(&mut self, limit: Option<usize>) -> Changes {
        self.limit = limit;
        let mut hold = Vec::new();
        if let Some(limit) = limit {
            while self.running.len() > limit {
                if let Some(last) = self.running.pop() {
                    hold.push(last);
                }
            }
        }
        // `hold` is newest first; put them back at the front of the line oldest first.
        for job in &hold {
            self.waiting.push_front(*job);
        }
        Changes {
            start: self.fill(),
            hold,
        }
    }

    /// Start `job` now, ahead of the line (the user's "Start now"): it leaves the line
    /// and runs. At the limit, the job started last steps back to the front of the line,
    /// to continue first when there is room.
    ///
    /// # Returns
    ///
    /// `job` to start, and the job to hold back if one had to make room (nothing if `job`
    /// is already running)
    pub fn start_now(&mut self, job: JobRef) -> Changes {
        if self.running.contains(&job) {
            return Changes::default();
        }
        self.waiting.retain(|waiting| *waiting != job);
        let mut hold = Vec::new();
        if !self.has_room()
            && let Some(last) = self.running.pop()
        {
            self.waiting.push_front(last);
            hold.push(last);
        }
        self.running.push(job);
        Changes {
            start: vec![job],
            hold,
        }
    }

    /// Whether another job can run.
    fn has_room(&self) -> bool {
        self.limit.is_none_or(|limit| self.running.len() < limit)
    }

    /// Start waiting jobs, in turn, while there is room.
    fn fill(&mut self) -> Vec<JobRef> {
        let mut started = Vec::new();
        while self.has_room() {
            let Some(next) = self.waiting.pop_front() else {
                break;
            };
            self.running.push(next);
            started.push(next);
        }
        started
    }
}

/// The limit for the whole app: [`Slots`], plus a way to tell each session's transfer
/// panel which of its jobs to start or hold back. Shared by the panels (`Rc`).
///
/// What it tells a panel arrives as a message, handled after the panel is done with
/// what it is doing, so a panel is never called back in the middle of asking.
#[derive(Default)]
pub struct TransferLimit {
    slots: RefCell<Slots>,
    /// Each session's transfer panel, to send [`TransferPanelMsg::SlotFree`] and
    /// [`TransferPanelMsg::Hold`] to.
    panels: RefCell<HashMap<SessionId, Sender<TransferPanelMsg>>>,
    /// Called after every change, to show the counts (the status bar).
    on_change: RefCell<Option<Box<dyn Fn()>>>,
}

impl TransferLimit {
    /// No panel yet, with this limit.
    ///
    /// # Arguments
    ///
    /// * `limit` - Most jobs running at once; `None` for no limit
    pub fn new(limit: Option<usize>) -> Rc<Self> {
        Rc::new(Self {
            slots: RefCell::new(Slots::new(limit)),
            panels: RefCell::default(),
            on_change: RefCell::default(),
        })
    }

    /// Call `on_change` after every change of the limit or of the jobs running and
    /// waiting.
    pub fn set_on_change(&self, on_change: impl Fn() + 'static) {
        *self.on_change.borrow_mut() = Some(Box::new(on_change));
    }

    /// A session's transfer panel opened.
    ///
    /// # Arguments
    ///
    /// * `session` - The session
    /// * `panel` - Its transfer panel's messages
    pub fn register(&self, session: SessionId, panel: Sender<TransferPanelMsg>) {
        self.panels.borrow_mut().insert(session, panel);
    }

    /// A session's transfer panel closed: its jobs give up their places.
    pub fn unregister(&self, session: SessionId) {
        self.panels.borrow_mut().remove(&session);
        let start = self.slots.borrow_mut().release_session(session);
        self.tell(&Changes {
            start,
            hold: Vec::new(),
        });
    }

    /// See [`Slots::request`].
    pub fn request(&self, job: JobRef) -> bool {
        let can_run = self.slots.borrow_mut().request(job);
        self.changed();
        can_run
    }

    /// See [`Slots::release`]; the panels are told which jobs to start.
    pub fn release(&self, job: JobRef) {
        let start = self.slots.borrow_mut().release(job);
        self.tell(&Changes {
            start,
            hold: Vec::new(),
        });
    }

    /// See [`Slots::replace`].
    pub fn replace(&self, old: JobRef, new: JobRef) {
        self.slots.borrow_mut().replace(old, new);
    }

    /// See [`Slots::set_limit`]; the panels are told which jobs to start or hold back.
    pub fn set_limit(&self, limit: Option<usize>) {
        let changes = self.slots.borrow_mut().set_limit(limit);
        self.tell(&changes);
    }

    /// See [`Slots::start_now`]; the panels are told which job to start and which to
    /// hold back.
    pub fn start_now(&self, job: JobRef) {
        let changes = self.slots.borrow_mut().start_now(job);
        self.tell(&changes);
    }

    /// The limit, and how many jobs are running and waiting.
    pub fn counts(&self) -> (Option<usize>, usize, usize) {
        let slots = self.slots.borrow();
        (slots.limit(), slots.running(), slots.waiting())
    }

    /// Tell whoever shows the counts.
    fn changed(&self) {
        if let Some(on_change) = self.on_change.borrow().as_ref() {
            on_change();
        }
    }

    /// Send each job's panel what to do with it, then tell whoever shows the counts.
    fn tell(&self, changes: &Changes) {
        let panels = self.panels.borrow();
        let orders = changes
            .start
            .iter()
            .map(|job| (job, TransferPanelMsg::SlotFree(job.job)))
            .chain(
                changes
                    .hold
                    .iter()
                    .map(|job| (job, TransferPanelMsg::Hold(job.job))),
            );
        for (job, order) in orders {
            match panels.get(&job.session) {
                // Fails only if the panel is gone, when nobody needs the order.
                Some(panel) => {
                    let _ = panel.send(order);
                }
                None => tracing::warn!("no transfer panel for session {}", job.session),
            }
        }
        drop(panels);
        self.changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Job `job` of session 1.
    fn job(job: u64) -> JobRef {
        JobRef { session: 1, job }
    }

    #[test]
    fn without_a_limit_every_job_starts() {
        let mut slots = Slots::new(None);

        assert!((1..=20).all(|id| slots.request(job(id))));
        assert_eq!((slots.running(), slots.waiting()), (20, 0));
    }

    #[test]
    fn jobs_beyond_the_limit_wait_and_start_in_turn() {
        let mut slots = Slots::new(Some(2));

        assert!(slots.request(job(1)));
        assert!(slots.request(job(2)));
        assert!(!slots.request(job(3)));
        assert!(!slots.request(job(4)));
        assert!(slots.is_waiting(job(3)));

        // Job 1 ends: the oldest waiting job, 3, takes its place.
        assert_eq!(slots.release(job(1)), [job(3)]);
        assert_eq!((slots.running(), slots.waiting()), (2, 1));
    }

    #[test]
    fn a_job_cancelled_while_waiting_leaves_the_line() {
        let mut slots = Slots::new(Some(1));
        slots.request(job(1));
        slots.request(job(2));
        slots.request(job(3));

        // Nothing starts: the one running job is still running.
        assert!(slots.release(job(2)).is_empty());
        assert_eq!(slots.release(job(1)), [job(3)]);
    }

    #[test]
    fn raising_the_limit_starts_waiting_jobs() {
        // Your example: 20 jobs, 5 running and 15 waiting; one more place starts one.
        let mut slots = Slots::new(Some(5));
        for id in 1..=20 {
            slots.request(job(id));
        }

        let changes = slots.set_limit(Some(6));

        assert_eq!(changes.start, [job(6)]);
        assert!(changes.hold.is_empty());
        assert_eq!((slots.running(), slots.waiting()), (6, 14));
    }

    #[test]
    fn lowering_the_limit_holds_back_the_jobs_started_last() {
        let mut slots = Slots::new(Some(5));
        for id in 1..=7 {
            slots.request(job(id));
        }

        let changes = slots.set_limit(Some(3));

        // Jobs 5 and 4 started last: they step back, and continue before job 6.
        assert_eq!(changes.hold, [job(5), job(4)]);
        assert!(changes.start.is_empty());
        assert_eq!(slots.release(job(1)), [job(4)]);
        assert_eq!(slots.release(job(2)), [job(5)]);
        assert_eq!(slots.release(job(3)), [job(6)]);
    }

    #[test]
    fn unlimited_again_starts_everything() {
        let mut slots = Slots::new(Some(1));
        for id in 1..=4 {
            slots.request(job(id));
        }

        let changes = slots.set_limit(None);

        assert_eq!(changes.start, [job(2), job(3), job(4)]);
        assert_eq!(slots.waiting(), 0);
    }

    #[test]
    fn a_continued_job_keeps_its_place() {
        let mut slots = Slots::new(Some(1));
        slots.request(job(1));
        slots.request(job(2));

        slots.replace(job(2), job(9));

        assert!(slots.is_waiting(job(9)));
        assert_eq!(slots.release(job(1)), [job(9)]);
    }

    #[test]
    fn a_closed_session_gives_up_its_places() {
        let mut slots = Slots::new(Some(2));
        let other = JobRef { session: 2, job: 1 };
        slots.request(job(1));
        slots.request(job(2));
        slots.request(other);

        assert_eq!(slots.release_session(1), [other]);
        assert_eq!((slots.running(), slots.waiting()), (1, 0));
    }

    #[test]
    fn jobs_of_different_sessions_share_the_limit() {
        let mut slots = Slots::new(Some(1));
        let (left, right) = (JobRef { session: 1, job: 1 }, JobRef { session: 2, job: 1 });

        assert!(slots.request(left));
        assert!(!slots.request(right));
        assert_eq!(slots.release(left), [right]);
    }

    #[test]
    fn start_now_puts_a_waiting_job_ahead_and_the_last_started_steps_back() {
        // Two running (1, 2), two waiting (3, 4).
        let mut slots = Slots::new(Some(2));
        for id in 1..=4 {
            slots.request(job(id));
        }

        let changes = slots.start_now(job(4));

        // Job 4 runs; job 2, started last, steps back to the front of the line.
        assert_eq!(changes.start, [job(4)]);
        assert_eq!(changes.hold, [job(2)]);
        assert_eq!((slots.running(), slots.waiting()), (2, 2));
        // When there is room, job 2 continues before job 3.
        assert_eq!(slots.release(job(1)), [job(2)]);
        assert_eq!(slots.release(job(4)), [job(3)]);
    }

    #[test]
    fn start_now_with_room_holds_nothing_back() {
        let mut slots = Slots::new(None);
        slots.request(job(1));

        let changes = slots.start_now(job(2));

        assert_eq!(changes.start, [job(2)]);
        assert!(changes.hold.is_empty());
        assert_eq!(slots.running(), 2);
    }

    #[test]
    fn start_now_on_a_running_job_changes_nothing() {
        let mut slots = Slots::new(Some(1));
        slots.request(job(1));
        slots.request(job(2));

        assert_eq!(slots.start_now(job(1)), Changes::default());
        assert!(slots.is_waiting(job(2)));
    }
}
