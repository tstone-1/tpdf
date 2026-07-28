//! Which render requests are outstanding, and which have been withdrawn.
//!
//! Renders run on the render service's own threads and requests arrive from
//! another, so a withdrawal races the render it is withdrawing. Dequeue order is
//! FIFO --- one channel --- but on the worker backend `pool + 2` threads take
//! from it and several renders overlap, so the race is against *any* of them
//! rather than against one. That is why `inflight` below is a map; see the note
//! under it, which is the same fact arrived at the expensive way. This is the
//! state machine that
//! makes that race harmless, and it is deliberately separate from the rendering:
//! it holds nothing but integers and a flag, touches no PDFium, and can
//! therefore be tested by driving the orderings directly rather than by trying
//! to provoke them.
//!
//! A request is in exactly one state at a time:
//!
//! ```text
//!   enqueue ──▶ queued ──┬── claim ──▶ in flight ── release ──▶ gone
//!                        │                  ▲
//!                    withdraw           withdraw
//!                        │              (cancels)
//!                        ▼
//!                    claim ──▶ gone (never rendered)
//! ```
//!
//! Both halves of a withdrawal --- finding the request and stopping it --- happen
//! under one lock, which is why [`SharedQueue`] hands out a `&mut Queue` for the
//! duration of a call rather than a guard the caller could split a decision
//! across.
//!
//! **Several requests are in flight at once, and that is why `inflight` is a map.**
//! It was an `Option<(u64, CancelToken)>` first, written when the render service
//! was one FIFO thread and only one render could exist. The worker backend serves
//! the same queue from `pool + 2` threads (`render.rs`), so a second claim
//! overwrote the first --- and a withdrawal naming the first then matched nothing
//! in `inflight` and nothing in `queued` either, because its own claim had already
//! removed it. It was a silent no-op, on the *older* of two concurrent renders,
//! which is exactly the one a scrolling viewport wants to withdraw. The worker's
//! own copy of this queue still cancelled the render, so nothing looked broken ---
//! a safety net that cannot fire, which `AGENTS.md` records twice as the shape to
//! watch for.
//!
//! Request id **zero means "not withdrawable"** and is not tracked at all. That
//! is not an optimisation: an untracked id must never become the in-flight
//! registration, or a withdrawal naming zero would cancel whatever unrelated
//! render happened to be running.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::progressive::CancelToken;

/// What claiming a request produced.
#[derive(Debug)]
pub enum Claim {
    /// Start rendering, stopping when this token is set.
    Start(CancelToken),
    /// Withdrawn before it ever started. The whole render is saved rather than
    /// merely interrupted, which on a page costing a second a tile is the larger
    /// of the two savings.
    Withdrawn,
}

/// The outstanding-request table. See the module docs.
#[derive(Default, Debug)]
pub struct Queue {
    /// Handed to the render thread but not yet started.
    queued: HashSet<u64>,
    /// Withdrawn while still queued. A subset of `queued` by construction, so it
    /// drains with it: a withdrawal naming a request that already finished is
    /// dropped rather than remembered, which is what keeps this bounded.
    cancelled: HashSet<u64>,
    /// Every render currently running, and the token that stops each one.
    ///
    /// A map rather than a single slot because the worker backend renders several
    /// tiles of a document at once --- see the module note. Bounded by the number
    /// of service threads, and drained by [`Queue::release`], which every claim
    /// path pairs with.
    inflight: HashMap<u64, CancelToken>,
}

impl Queue {
    /// Records a request as handed to the render thread.
    pub fn enqueue(&mut self, rid: u64) {
        if rid != 0 {
            self.queued.insert(rid);
        }
    }

    /// Drops a request that never reached the render thread.
    ///
    /// Without this a failed send would leave the id outstanding forever, since
    /// nothing will ever claim it.
    pub fn forget(&mut self, rid: u64) {
        self.queued.remove(&rid);
        self.cancelled.remove(&rid);
    }

    /// Withdraws a request, wherever it is.
    ///
    /// An id that is neither queued nor in flight is ignored rather than
    /// remembered --- it has already finished, and its reply is on the way.
    /// Remembering it is what would let this grow without bound.
    ///
    /// Zero needs no special case here, and deliberately does not have one:
    /// [`enqueue`](Self::enqueue) keeps it out of `queued` and
    /// [`claim`](Self::claim) keeps it out of `inflight`, so it matches nothing
    /// and falls out as a no-op. An early return for it was written first and
    /// then deleted --- no mutation of it could fail a test, because those two
    /// guards already made it unreachable, and a check nothing can pin is a
    /// check that will one day be wrong without anyone noticing.
    pub fn withdraw(&mut self, rid: u64) {
        if let Some(token) = self.inflight.get(&rid) {
            token.cancel();
            return;
        }
        if self.queued.contains(&rid) {
            self.cancelled.insert(rid);
        }
    }

    /// Takes a request off the queue, either to render or to drop.
    pub fn claim(&mut self, rid: u64) -> Claim {
        self.queued.remove(&rid);
        if self.cancelled.remove(&rid) {
            return Claim::Withdrawn;
        }

        let token = CancelToken::new();
        if rid != 0 {
            self.inflight.insert(rid, token.clone());
        }
        Claim::Start(token)
    }

    /// Releases a claim once its render has ended.
    ///
    /// Keyed on the request, so a render finishing cannot clear a *different*
    /// one that is still running beside it. That was already the intent when
    /// there was one slot; with several threads claiming concurrently it is the
    /// difference between a working withdrawal and a silent no-op.
    pub fn release(&mut self, rid: u64) {
        self.inflight.remove(&rid);
    }

    /// Requests known to this queue, whether queued or running.
    ///
    /// Test-only, along with [`pending_withdrawals`](Self::pending_withdrawals):
    /// nothing in the viewer needs either, and they exist so the tables can be
    /// asserted empty rather than argued to be. Ungate them if a caller appears.
    #[cfg(test)]
    pub fn outstanding(&self) -> usize {
        self.queued.len() + self.inflight.len()
    }

    /// Withdrawals recorded against requests that have not been claimed yet.
    ///
    /// This must return to zero. A set that only ever grows is the failure mode
    /// a withdrawal table has, and it is invisible in every functional test.
    #[cfg(test)]
    pub fn pending_withdrawals(&self) -> usize {
        self.cancelled.len()
    }
}

/// A [`Queue`] shared between the requesting side and the render thread.
///
/// Cheap to clone; every clone is the same queue.
#[derive(Clone, Default)]
pub struct SharedQueue(Arc<Mutex<Queue>>);

impl SharedQueue {
    /// Runs `f` with exclusive access.
    ///
    /// Poisoning is ignored. A panic on the render thread would poison this, and
    /// refusing every later request because of it turns one failed tile into a
    /// dead viewer; the state behind it has no invariant a partial update can
    /// break.
    pub fn with<R>(&self, f: impl FnOnce(&mut Queue) -> R) -> R {
        let mut queue = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut queue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Claims `rid` and returns its token, failing the test if it was withdrawn.
    fn start(queue: &mut Queue, rid: u64) -> CancelToken {
        match queue.claim(rid) {
            Claim::Start(token) => token,
            Claim::Withdrawn => panic!("request {rid} was withdrawn, expected it to start"),
        }
    }

    #[test]
    fn a_claim_that_was_not_withdrawn_starts_and_stays_running() {
        // The control for the two tests below: without it, "withdrawn" could be
        // the only answer this queue ever gives and both would still pass.
        let mut queue = Queue::default();
        queue.enqueue(1);
        let token = start(&mut queue, 1);
        assert!(!token.is_cancelled());
    }

    #[test]
    fn withdrawing_before_the_claim_drops_the_render_entirely() {
        let mut queue = Queue::default();
        queue.enqueue(1);
        queue.withdraw(1);
        assert!(matches!(queue.claim(1), Claim::Withdrawn));
    }

    #[test]
    fn withdrawing_after_the_claim_cancels_the_running_render() {
        let mut queue = Queue::default();
        queue.enqueue(1);
        let token = start(&mut queue, 1);
        queue.withdraw(1);
        assert!(token.is_cancelled());
    }

    #[test]
    fn withdrawing_one_request_leaves_the_others_alone() {
        let mut queue = Queue::default();
        for rid in 1..=3 {
            queue.enqueue(rid);
        }
        queue.withdraw(2);

        assert!(matches!(queue.claim(1), Claim::Start(_)));
        queue.release(1);
        assert!(matches!(queue.claim(2), Claim::Withdrawn));
        assert!(matches!(queue.claim(3), Claim::Start(_)));
        queue.release(3);
    }

    #[test]
    fn a_withdrawal_that_loses_the_race_is_not_remembered() {
        // The bounding property. A withdrawal naming a request that already
        // finished has nothing to cancel, and keeping it would make this table
        // grow for the life of the process.
        let mut queue = Queue::default();
        queue.enqueue(1);
        start(&mut queue, 1);
        queue.release(1);

        queue.withdraw(1);
        assert_eq!(queue.pending_withdrawals(), 0);
        assert_eq!(queue.outstanding(), 0);
    }

    #[test]
    fn withdrawing_an_id_that_was_never_issued_is_ignored() {
        let mut queue = Queue::default();
        queue.withdraw(99);
        assert_eq!(queue.pending_withdrawals(), 0);
        assert_eq!(queue.outstanding(), 0);
    }

    #[test]
    fn claiming_drains_the_tables_it_read() {
        let mut queue = Queue::default();
        for rid in 1..=4 {
            queue.enqueue(rid);
        }
        queue.withdraw(3);
        assert_eq!(queue.outstanding(), 4);

        for rid in 1..=4 {
            queue.claim(rid);
            queue.release(rid);
        }
        assert_eq!(queue.outstanding(), 0);
        assert_eq!(queue.pending_withdrawals(), 0);
    }

    #[test]
    fn an_unwithdrawable_request_never_becomes_the_cancellation_target() {
        // The reason zero is untracked. If claiming it registered an in-flight
        // id of zero, a withdrawal naming zero -- which any caller that opted
        // out sends by accident -- would cancel it.
        let mut queue = Queue::default();
        queue.enqueue(0);
        assert_eq!(queue.outstanding(), 0);

        let token = start(&mut queue, 0);
        queue.withdraw(0);
        assert!(!token.is_cancelled());
    }

    #[test]
    fn an_unwithdrawable_request_does_not_displace_a_running_one() {
        let mut queue = Queue::default();
        queue.enqueue(1);
        let real = start(&mut queue, 1);

        // Interleaved rather than sequential on purpose: claiming zero must not
        // overwrite the in-flight registration that a later withdrawal needs.
        start(&mut queue, 0);
        queue.withdraw(1);
        assert!(real.is_cancelled());
    }

    #[test]
    fn withdrawing_the_older_of_two_concurrent_claims_still_cancels_it() {
        // The worker backend claims from `pool + 2` threads at once, so two
        // requests are genuinely in flight together. With a single `inflight`
        // slot the second claim evicted the first, and `withdraw(1)` then found
        // it in neither table --- `claim` having already taken it out of
        // `queued` --- so it cancelled nothing and said nothing.
        let mut queue = Queue::default();
        queue.enqueue(1);
        queue.enqueue(2);
        let first = start(&mut queue, 1);
        let second = start(&mut queue, 2);

        queue.withdraw(1);
        assert!(first.is_cancelled(), "the older claim was not cancelled");
        // The control: withdrawing one must not cancel the other, or a queue
        // that cancels everything would pass the assertion above.
        assert!(!second.is_cancelled(), "the newer claim was cancelled too");
    }

    #[test]
    fn several_claims_are_tracked_at_once_and_each_drains_on_release() {
        // The bounding property, now that the table can hold more than one. A
        // release keyed on the wrong request would leave entries behind, and
        // `inflight` would grow for the life of the process.
        let mut queue = Queue::default();
        for rid in 1..=4 {
            queue.enqueue(rid);
            start(&mut queue, rid);
        }
        assert_eq!(queue.outstanding(), 4);

        // Out of order on purpose: renders finish in whatever order the pool
        // gets to them, which is not the order they were claimed in.
        for rid in [3, 1, 4, 2] {
            queue.release(rid);
        }
        assert_eq!(queue.outstanding(), 0);
        assert_eq!(queue.pending_withdrawals(), 0);
    }

    #[test]
    fn releasing_a_finished_request_does_not_clear_a_newer_claim() {
        let mut queue = Queue::default();
        queue.enqueue(1);
        queue.enqueue(2);
        start(&mut queue, 1);
        let second = start(&mut queue, 2);

        queue.release(1);
        queue.withdraw(2);
        assert!(second.is_cancelled());
    }

    #[test]
    fn a_request_that_never_reached_the_renderer_is_forgotten() {
        let mut queue = Queue::default();
        queue.enqueue(1);
        queue.withdraw(1);
        queue.forget(1);
        assert_eq!(queue.outstanding(), 0);
        assert_eq!(queue.pending_withdrawals(), 0);
    }

    #[test]
    fn the_shared_queue_is_one_queue() {
        let queue = SharedQueue::default();
        let other = queue.clone();
        queue.with(|q| q.enqueue(1));
        assert_eq!(other.with(|q| q.outstanding()), 1);
    }
}
