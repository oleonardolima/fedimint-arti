//! Help the guard manager (and other crates) deal with "pending
//! information".
//!
//! There are two kinds of pending information to deal with.  First,
//! every guard that we hand out needs to be marked as succeeded or
//! failed. Second, if a guard is given out on an exploratory basis,
//! then the circuit manager can't know whether to use a circuit built
//! through that guard until the guard manager tells it.  This is
//! handled via [`GuardUsable`].
use crate::{daemon, GuardId};

use futures::{channel::oneshot, Future};
use pin_project::pin_project;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Instant;

/// A future used to see if we have "permission" to use a guard.
///
/// For efficiency, the [`crate::GuardMgr`] implementation sometimes gives
/// out lower-priority guards when it is not certain whether
/// higher-priority guards are running.  After having built a circuit
/// with such a guard, the caller must wait on this future to see whether
/// the circuit is usable or not.
///
/// The circuit may be usable immediately (as happens if the guard was
/// of sufficient priority, or if all higher-priority guards are
/// _known_ to be down).  It may eventually _become_ usable (if all of
/// the higher-priority guards are _discovered_ to be down).  Or it may
/// eventually become unusable (if we find a higher-priority guard
/// that works).
///
/// Any [`crate::GuardRestriction`]s that were used to select this guard
/// may influence whether it is usable: if higher priority guards were
/// ignored because of a restriction, then we might use a guard that we
/// otherwise wouldn't.
#[pin_project]
pub struct GuardUsable {
    /// If present, then this is a future to wait on to see whether the
    /// guard is usable.
    ///
    /// If absent, then the guard is ready immediately and no waiting
    /// is needed.
    #[pin]
    u: Option<oneshot::Receiver<bool>>,
}

impl Future for GuardUsable {
    type Output = Result<bool, oneshot::Canceled>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().u.as_pin_mut() {
            None => Poll::Ready(Ok(true)),
            Some(u) => u.poll(cx),
        }
    }
}

impl GuardUsable {
    /// Create a new GuardUsable for a primary guard.
    ///
    /// (Circuits built through primary guards are usable immediately,
    /// so we don't need a way to report that this guard is usable.)
    pub(crate) fn new_primary() -> Self {
        GuardUsable { u: None }
    }

    /// Create a new GuardUsable for a guard with undecided usability
    /// status.
    pub(crate) fn new_uncertain() -> (Self, oneshot::Sender<bool>) {
        let (snd, rcv) = oneshot::channel();
        (GuardUsable { u: Some(rcv) }, snd)
    }
}

/// A message that we can get back from the circuit manager who asked
/// for a guard.
#[derive(Copy, Clone, Debug)]
pub(crate) enum GuardStatusMsg {
    /// The guard was used successfully.
    Success,
    /// The guard was used unsuccessfuly.
    Failure,
    /// Our attempt to use the guard didn't get far enough to be sure
    /// whether the guard is usable or not.
    AttemptAbandoned,
}

/// An object used to tell the [`crate::GuardMgr`] about the result of
/// trying to build a circuit through a guard.
///
/// The `GuardMgr` needs to know about these statuses, so that it can tell
/// whether the guard is running or not.
#[must_use = "You need to report the status of any guard that you asked for"]
pub struct GuardMonitor {
    /// The Id that we're going to report about.
    id: RequestId,
    /// A sender that needs to get told when the attempt to use the guard is
    /// finished or abandoned.
    snd: Option<oneshot::Sender<daemon::Msg>>,
}

impl GuardMonitor {
    /// Create a new GuardMonitor object.
    pub(crate) fn new(id: RequestId) -> (Self, oneshot::Receiver<daemon::Msg>) {
        let (snd, rcv) = oneshot::channel();
        (GuardMonitor { id, snd: Some(snd) }, rcv)
    }

    /// Report that a circuit was successfully built in a way that
    /// indicates that the guard is working.
    ///
    /// Note that this doesn't necessarily mean that the circuit
    /// succeeded. For example, we might decide that extending to a
    /// second hop means that a guard is usable, even if the circuit
    /// stalled at the third hop.
    pub fn succeeded(mut self) {
        let _ignore = self
            .snd
            .take()
            .expect("GuardMonitor initialized with no sender")
            .send(daemon::Msg::Status(self.id, GuardStatusMsg::Success));
    }

    /// Report that the circuit could not be built successfully, in
    /// a way that indicates that the guard isn't working.
    ///
    /// (This either be because of a network failure, a timeout, or
    /// something else.)
    pub fn failed(mut self) {
        let _ignore = self
            .snd
            .take()
            .expect("GuardMonitor initialized with no sender")
            .send(daemon::Msg::Status(self.id, GuardStatusMsg::Failure));
    }

    /// Report that we did not try to build a circuit using the guard,
    /// or that we can't tell whether the guard is working.
    ///
    /// Dropping a `GuardMonitor` is without calling `succeeded` or
    /// `failed` is equivalent to calling this function.
    pub fn attempt_abandoned(self) {
        drop(self);
    }
}

impl Drop for GuardMonitor {
    fn drop(&mut self) {
        if let Some(snd) = self.snd.take() {
            let _ignore = snd.send(daemon::Msg::Status(
                self.id,
                GuardStatusMsg::AttemptAbandoned,
            ));
        }
    }
}

/// Internal unique identifier used to tell PendingRequest objects apart.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub(crate) struct RequestId {
    /// The value of the identifier.
    id: u64,
}

impl RequestId {
    /// Create a new, never-before-used RequestId.
    ///
    /// # Panics
    ///
    /// Panics if we have somehow exhausted a 64-bit space of request IDs.
    pub(crate) fn next() -> RequestId {
        /// The next identifier in sequence we'll give out.
        static NEXT_VAL: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_VAL.fetch_add(1, Ordering::Relaxed);
        assert!(id != 0, "Exhausted guard request Id space.");
        RequestId { id }
    }
}

/// Pending information about a guard that we handed out in response to
/// some request, but where we have not yet reported whether the guard
/// is usable.
///
/// We create one of these whenever we give out a guard with an
/// uncertain usability status via [`GuardUsable::new_uncertain`].
#[derive(Debug)]
pub(crate) struct PendingRequest {
    /// Identity of the guard that we gave out.
    guard_id: GuardId,
    /// The usage for which this guard was requested.
    ///
    /// We need this information because, if we find that a better guard
    /// than this one might be usable, we should only give it precedence
    /// if that guard is also allowable _for this usage_.
    usage: crate::GuardUsage,
    /// A oneshot channel used to tell the circuit manager that a circuit
    /// built through this guard can be used.
    ///
    /// (This is an option so that we can safely make reply() once-only.
    /// Otherwise we run into lifetime isseus elsewhere.)
    usable: Option<oneshot::Sender<bool>>,
    /// The time when we gave out this guard.
    started_at: Instant,
    /// The time at which the circuit manager told us that this guard was
    /// successful.
    waiting_since: Option<Instant>,
}

impl PendingRequest {
    /// Create a new PendingRequest.
    pub(crate) fn new(
        guard_id: GuardId,
        usage: crate::GuardUsage,
        usable: Option<oneshot::Sender<bool>>,
        started_at: Instant,
    ) -> Self {
        PendingRequest {
            guard_id,
            usage,
            usable,
            started_at,
            waiting_since: None,
        }
    }

    /// Return the Id of the guard we gave out.
    pub(crate) fn guard_id(&self) -> &GuardId {
        &self.guard_id
    }

    /// Return the usage for which we gave out the guard.
    pub(crate) fn usage(&self) -> &crate::GuardUsage {
        &self.usage
    }

    /// Return the time (if any) when we were told that the guard
    /// was successful.
    pub(crate) fn waiting_since(&self) -> Option<Instant> {
        self.waiting_since
    }

    /// Tell the circuit manager that the guard is usable (or unusable),
    /// depending on the argument.
    ///
    /// Does nothing if reply() has already been called.
    pub(crate) fn reply(&mut self, usable: bool) {
        if let Some(sender) = self.usable.take() {
            // If this gives us an error, then the circuit manager doesn't
            // care about this circuit any more.
            let _ignore = sender.send(usable);
        }
    }

    /// Mark this request as "waiting" since the time `now`.
    ///
    /// This function should only be called once per request.
    pub(crate) fn mark_waiting(&mut self, now: Instant) {
        debug_assert!(self.waiting_since.is_none());
        self.waiting_since = Some(now);
    }
}