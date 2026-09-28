//! All-hart quiescence for the snapshot capture preflight.
//!
//! A capture reads the *whole* memory image and writes it to the block device,
//! so no other hart may run kernel code that mutates that image while it is
//! staged: the format cannot detect bytes that changed between the read and the
//! block write (phase 07 step 3). This module is the device-independent half of
//! the protocol — request every online hart to park at a safe point, collect a
//! per-hart acknowledgement, wait a *bounded* time, verify the all-parked
//! predicate, and release the harts again.
//!
//! # Fail-closed contract
//!
//! [`QuiesceState::acquire`] returns `Ok` only when every online hart other
//! than the requester has acknowledged the request *and* the predicate
//! re-verifies at that instant. It never reports success on a timeout, never
//! releases a hart it did not park, and never blocks the requester on itself
//! (the requester is excluded from the target set, so its own
//! acknowledgement can never be required). Every failure path calls the
//! idempotent [`Guard::release`], which restores the pre-request state — each
//! requested hart is released, including one that never acknowledged, whose
//! outstanding request is cancelled — and frees the single-flight claim so a
//! later capture can try again. A `Drop` impl runs the same release on the
//! unwind and on the success path, so the harts are unparked after the capture
//! finishes.
//!
//! The wait is bounded twice over: by [`QuiesceHarts::ack_budget_ticks`] in the
//! hart set's own clock units, and by a clock-independent poll limit so a tick
//! source that never advances cannot hang the caller. A previous phase burned a
//! full retry budget on every awaited invalidation (`memory::tlb_shootdown`);
//! this protocol does not retry inside the preflight for the same reason — the
//! next capture is the retry boundary, and a hart that cannot reach a safe
//! point is a condition to report, not to spin on.
//!
//! # The missing per-hart park hook
//!
//! A hart can only acknowledge a park request if something on that hart
//! observes the request, reaches a safe point and publishes the
//! acknowledgement. That hook needs the scheduler and the trap path, and this
//! slice deliberately does not touch either. The hook is therefore modelled
//! behind [`QuiesceHarts`] — the seam that a scheduler-side implementation must
//! fill — and [`KernelHarts::park_hook_available`] reports `false`, which makes
//! `acquire` refuse any multi-hart request *before* it waits for an
//! acknowledgement no hart can produce. A single-hart system needs no hook: the
//! protocol is then a no-op, because the requester is the only hart that could
//! mutate the image.

use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

/// Upper bound on the harts this protocol tracks. The kernel's own bound is
/// `task::smp::MAX_HARTS`; the protocol keeps headroom so a host test can model
/// a partial acknowledgement (two targets, one of them silent).
pub const MAX_QUIESCE_HARTS: usize = 8;

/// Clock-independent backstop on the bounded wait: a hart set whose
/// [`QuiesceHarts::now_ticks`] never advances (no cross-arch tick source is
/// wired into the kernel yet) still terminates instead of spinning forever.
/// The tick budget is the primary bound and trips first whenever the clock
/// moves.
const WAIT_POLL_LIMIT: usize = 1 << 28;

/// Why quiescence could not be established.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuiesceError {
    /// Another request owns the protocol right now. Only one capture may hold
    /// the memory image frozen.
    AlreadyRequested,
    /// The hart set has no per-hart park hook, so a multi-hart request could
    /// never be acknowledged. Refused immediately — never waited out.
    Unsupported,
    /// More online harts than [`MAX_QUIESCE_HARTS`], or a hart id outside that
    /// bound. Fail closed rather than track an unbounded set.
    TooManyHarts,
    /// The bounded wait expired with `pending` harts still un-acknowledged.
    /// Every requested hart has already been released when this is returned.
    Timeout { pending: usize },
}

impl QuiesceError {
    /// Stable short description for logs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlreadyRequested => "another request already owns the quiescence protocol",
            Self::Unsupported => "no per-hart park hook: a multi-hart request cannot be acknowledged",
            Self::TooManyHarts => "more online harts than the quiescence protocol tracks",
            Self::Timeout { .. } => "harts did not acknowledge the park request within the budget",
        }
    }
}

impl fmt::Display for QuiesceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout { pending } => write!(f, "{} ({pending} pending)", self.as_str()),
            _ => f.write_str(self.as_str()),
        }
    }
}

/// A bounded set of hart ids.
///
/// The bound is the *id* space: an id at or above [`MAX_QUIESCE_HARTS`] is
/// refused, which also makes an overflow of the backing array impossible.
#[derive(Clone, Copy)]
pub struct HartSet {
    ids: [usize; MAX_QUIESCE_HARTS],
    len: usize,
}

impl HartSet {
    /// An empty set.
    pub const fn new() -> Self {
        Self {
            ids: [0; MAX_QUIESCE_HARTS],
            len: 0,
        }
    }

    /// Add `hart`. Returns `Ok(false)` if it is already present, and
    /// [`QuiesceError::TooManyHarts`] if the id does not fit the bound.
    pub fn insert(&mut self, hart: usize) -> Result<bool, QuiesceError> {
        if hart >= MAX_QUIESCE_HARTS {
            return Err(QuiesceError::TooManyHarts);
        }
        if self.contains(hart) {
            return Ok(false);
        }
        self.ids[self.len] = hart;
        self.len += 1;
        Ok(true)
    }

    /// Is `hart` in the set?
    fn contains(&self, hart: usize) -> bool {
        self.ids[..self.len].contains(&hart)
    }

    /// Is the set empty?
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The ids in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.ids[..self.len].iter().copied()
    }
}

/// The board/hart side of the protocol.
///
/// This is the park-hook seam: `fake::FakeHarts` (host tests) and the live
/// kernel hart set both implement it, so the protocol's state machine has no
/// dependency on the scheduler or the trap path. A future phase fills the hook
/// in by making [`QuiesceHarts::park_hook_available`] return `true` and having
/// each hart publish its acknowledgement from a safe point.
pub trait QuiesceHarts {
    /// The hart executing this request. It is never asked to park: waiting on
    /// the requester's own acknowledgement would deadlock the capture.
    fn requester(&self) -> usize;

    /// Record every online hart, including the requester, into `out`.
    fn online_harts(&self, out: &mut HartSet) -> Result<(), QuiesceError>;

    /// Is a per-hart park hook implemented for this hart set?
    fn park_hook_available(&self) -> bool;

    /// Ask `hart` to park at its next safe point and return the epoch it must
    /// publish when it has parked.
    fn request_park(&self, hart: usize) -> usize;

    /// Has `hart` acknowledged `epoch`?
    fn park_acked(&self, hart: usize, epoch: usize) -> bool;

    /// Cancel `hart`'s outstanding park request and unpark it if parked.
    ///
    /// Must be idempotent and must accept a hart that never acknowledged: a
    /// release after a timeout has to restore the pre-request state, which
    /// means cancelling the request the hart may still be about to satisfy.
    fn release_park(&self, hart: usize);

    /// How many [`QuiesceHarts::now_ticks`] units a request waits before it is
    /// declared unacknowledged.
    fn ack_budget_ticks(&self) -> u64;

    /// Monotonic counter used only to bound the wait.
    fn now_ticks(&self) -> u64;
}

/// Single-flight protocol state.
///
/// One instance guards one memory image: the capture preflight holds a [`Guard`]
/// across the whole read-and-write, and a second concurrent request is refused
/// for as long as that guard lives.
pub struct QuiesceState {
    in_flight: AtomicBool,
}

impl QuiesceState {
    /// A state with no request outstanding.
    pub const fn new() -> Self {
        Self {
            in_flight: AtomicBool::new(false),
        }
    }

    /// Request that every online hart other than the requester park at an
    /// acknowledged safe point, and hold that state for as long as the returned
    /// guard lives.
    ///
    /// The guard releases the harts when it drops, so a successful acquisition
    /// covers exactly the capture and no longer. A refusal never leaves a hart
    /// parked: see the module docs for the fail-closed contract.
    #[must_use = "the harts are released as soon as the guard drops"]
    pub fn acquire<'a>(&'a self, harts: &'a dyn QuiesceHarts) -> Result<Guard<'a>, QuiesceError> {
        if self
            .in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(QuiesceError::AlreadyRequested);
        }
        self.request(harts)
    }

    fn request<'a>(&'a self, harts: &'a dyn QuiesceHarts) -> Result<Guard<'a>, QuiesceError> {
        let targets = match target_harts(harts) {
            Ok(targets) => targets,
            Err(err) => {
                self.in_flight.store(false, Ordering::Release);
                return Err(err);
            }
        };
        let mut guard = Guard {
            state: self,
            harts,
            requested: HartSet::new(),
            epochs: [0; MAX_QUIESCE_HARTS],
            released: false,
        };

        // One hart, no target: nothing can mutate the image, so there is no
        // request to make and no hook to need.
        if targets.is_empty() {
            return Ok(guard);
        }
        // A request no hart can acknowledge must be refused, not waited out.
        if !harts.park_hook_available() {
            guard.release();
            return Err(QuiesceError::Unsupported);
        }

        for hart in targets.iter() {
            guard.epochs[hart] = harts.request_park(hart);
        }
        guard.requested = targets;

        // Bounded wait: the tick budget is the primary bound, the poll limit
        // only guarantees termination when the clock does not move.
        let deadline = harts.now_ticks().saturating_add(harts.ack_budget_ticks());
        let mut polls = 0usize;
        while !guard.all_parked() {
            if polls >= WAIT_POLL_LIMIT || harts.now_ticks() > deadline {
                break;
            }
            polls += 1;
            core::hint::spin_loop();
        }

        // Verified predicate, not a remembered one: re-derive from the hart set
        // so an acknowledgement that lapsed while waiting cannot pass.
        if guard.all_parked() {
            return Ok(guard);
        }

        let pending = guard.pending_count();
        guard.release();
        Err(QuiesceError::Timeout { pending })
    }
}

/// Every online hart except the requester: the set that must park.
fn target_harts(harts: &dyn QuiesceHarts) -> Result<HartSet, QuiesceError> {
    let requester = harts.requester();
    let mut online = HartSet::new();
    harts.online_harts(&mut online)?;
    // The boot hart runs without publishing itself as "online" on RV64, so the
    // requester is unioned in rather than assumed present.
    online.insert(requester)?;
    let mut targets = HartSet::new();
    for hart in online.iter() {
        if hart != requester {
            targets.insert(hart)?;
        }
    }
    Ok(targets)
}

/// Proof that the harts were parked, releasing them when dropped.
pub struct Guard<'a> {
    state: &'a QuiesceState,
    harts: &'a dyn QuiesceHarts,
    /// The harts that were actually asked to park (empty on the no-op and the
    /// refusing paths, so `release` never touches a hart this protocol did not
    /// park).
    requested: HartSet,
    /// The epoch each requested hart must acknowledge, indexed by hart id.
    epochs: [usize; MAX_QUIESCE_HARTS],
    released: bool,
}

impl Guard<'_> {
    /// Re-derive the all-parked predicate from the hart set.
    pub fn all_parked(&self) -> bool {
        self.requested
            .iter()
            .all(|hart| self.harts.park_acked(hart, self.epochs[hart]))
    }

    /// How many requested harts have not acknowledged.
    pub fn pending_count(&self) -> usize {
        self.requested
            .iter()
            .filter(|hart| !self.harts.park_acked(*hart, self.epochs[*hart]))
            .count()
    }

    /// Restore the pre-request state: release every hart that was requested and
    /// free the single-flight claim. Idempotent.
    pub fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        for hart in self.requested.iter() {
            self.harts.release_park(hart);
        }
        self.state.in_flight.store(false, Ordering::Release);
    }
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.release();
    }
}

// ── The live kernel hart set ─────────────────────────────────────────────────

/// The kernel's own hart set.
pub struct KernelHarts;

impl QuiesceHarts for KernelHarts {
    fn requester(&self) -> usize {
        crate::task::hart_local::current_hart_id()
    }

    fn online_harts(&self, out: &mut HartSet) -> Result<(), QuiesceError> {
        out.insert(self.requester())?;
        for hart in crate::task::smp::online_harts() {
            out.insert(hart)?;
        }
        Ok(())
    }

    /// The per-hart park hook does not exist yet: it needs a scheduler-side
    /// observation point and an acknowledgement published from a safe point,
    /// neither of which this slice may add. Reporting `false` makes `acquire`
    /// refuse a multi-hart request before it waits, instead of burning a budget
    /// on harts that can never answer.
    fn park_hook_available(&self) -> bool {
        false
    }

    /// Unreachable while [`QuiesceHarts::park_hook_available`] is `false`; the
    /// stubs answer "never acknowledged" rather than panicking so a future
    /// mis-wiring still fails closed with a bounded timeout.
    fn request_park(&self, _hart: usize) -> usize {
        0
    }

    fn park_acked(&self, _hart: usize, _epoch: usize) -> bool {
        false
    }

    fn release_park(&self, _hart: usize) {}

    fn ack_budget_ticks(&self) -> u64 {
        #[cfg(target_arch = "riscv64")]
        {
            KERNEL_ACK_BUDGET_TICKS
        }
        #[cfg(not(target_arch = "riscv64"))]
        {
            0
        }
    }

    fn now_ticks(&self) -> u64 {
        // RV64 has the 10 MHz `mtime`; the other backends have no cross-arch
        // tick source wired in and their SMP is single-hart, so the no-op path
        // never reads a clock there (the poll limit still bounds the wait).
        #[cfg(target_arch = "riscv64")]
        {
            hal::common::timer::read_mtime()
        }
        #[cfg(not(target_arch = "riscv64"))]
        {
            0
        }
    }
}

/// How long the kernel waits for a park acknowledgement: 100 ms of `mtime`.
/// Not widened later — a hart that cannot reach a safe point is reported, and
/// the next capture is the retry boundary.
#[cfg(target_arch = "riscv64")]
const KERNEL_ACK_BUDGET_TICKS: u64 = 10 * hal::common::timer::TICKS_PER_10MS;

/// The kernel's single-flight protocol instance.
pub static KERNEL_STATE: QuiesceState = QuiesceState::new();

/// The kernel's hart set.
pub static KERNEL_HARTS: KernelHarts = KernelHarts;

// ── Host tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod fake {
    //! Deterministic hart set for the host tests, the counterpart of
    //! `snapshot::fake::FakeDisk`: no clock, no IPI, no scheduler. The clock
    //! advances one tick per [`QuiesceHarts::now_ticks`] read, so a bounded wait
    //! terminates in a fixed number of polls; acknowledgements are programmed
    //! per hart; every request and release is logged.

    use super::{HartSet, QuiesceError, QuiesceHarts};
    use alloc::collections::{BTreeMap, BTreeSet};
    use alloc::vec::Vec;
    use core::cell::RefCell;

    #[derive(Default)]
    struct State {
        online: Vec<usize>,
        /// Harts that acknowledge a park request as soon as it is made.
        acking: BTreeSet<usize>,
        /// hart -> epoch of the park request that is still outstanding.
        outstanding: BTreeMap<usize, usize>,
        /// hart -> epoch the hart currently acknowledges.
        acked: BTreeMap<usize, usize>,
        requests: Vec<(usize, usize)>,
        releases: Vec<usize>,
        /// `now_ticks()` reads; the value returned is the read count, so the
        /// clock is monotonic and advances exactly once per poll.
        clock: u64,
        next_epoch: usize,
    }

    pub struct FakeHarts {
        inner: RefCell<State>,
        requester: usize,
        hook: bool,
        budget: u64,
    }

    impl FakeHarts {
        /// A hart set with `online` harts and `requester`, with a park hook and
        /// an 8-tick budget. Nothing acknowledges until
        /// [`FakeHarts::ack_on_request`].
        pub fn new(online: &[usize], requester: usize) -> Self {
            Self {
                inner: RefCell::new(State {
                    online: online.to_vec(),
                    next_epoch: 1,
                    ..State::default()
                }),
                requester,
                hook: true,
                budget: 8,
            }
        }

        /// Model a hart set with no per-hart park hook.
        pub fn with_hook(mut self, hook: bool) -> Self {
            self.hook = hook;
            self
        }

        /// Override the acknowledgement budget in clock ticks.
        pub fn with_budget(mut self, budget: u64) -> Self {
            self.budget = budget;
            self
        }

        /// These harts acknowledge a park request as soon as it is made.
        pub fn ack_on_request(&self, harts: &[usize]) {
            let mut state = self.inner.borrow_mut();
            for hart in harts {
                state.acking.insert(*hart);
            }
        }

        /// The hart leaves the parked state (an interrupt, a fault) without a
        /// release, so the predicate must be re-derived rather than remembered.
        pub fn unack(&self, hart: usize) {
            self.inner.borrow_mut().acked.remove(&hart);
        }

        /// Every `(hart, epoch)` park request, in order.
        pub fn requests(&self) -> Vec<(usize, usize)> {
            self.inner.borrow().requests.clone()
        }

        /// Every hart passed to `release_park`, in order.
        pub fn releases(&self) -> Vec<usize> {
            self.inner.borrow().releases.clone()
        }

        /// Harts whose outstanding request is currently acknowledged.
        pub fn parked(&self) -> Vec<usize> {
            let state = self.inner.borrow();
            state
                .outstanding
                .keys()
                .filter(|hart| state.acked.get(hart) == state.outstanding.get(hart))
                .copied()
                .collect()
        }

        /// Harts with a park request that has not been released.
        pub fn outstanding(&self) -> Vec<usize> {
            self.inner.borrow().outstanding.keys().copied().collect()
        }

        /// How many times the clock was read (i.e. how many polls the bounded
        /// wait took).
        pub fn clock_ticks(&self) -> u64 {
            self.inner.borrow().clock
        }
    }

    impl QuiesceHarts for FakeHarts {
        fn requester(&self) -> usize {
            self.requester
        }

        fn online_harts(&self, out: &mut HartSet) -> Result<(), QuiesceError> {
            for hart in self.inner.borrow().online.iter().copied() {
                out.insert(hart)?;
            }
            Ok(())
        }

        fn park_hook_available(&self) -> bool {
            self.hook
        }

        fn request_park(&self, hart: usize) -> usize {
            let mut state = self.inner.borrow_mut();
            let epoch = state.next_epoch;
            state.next_epoch += 1;
            state.requests.push((hart, epoch));
            state.outstanding.insert(hart, epoch);
            if state.acking.contains(&hart) {
                state.acked.insert(hart, epoch);
            }
            epoch
        }

        fn park_acked(&self, hart: usize, epoch: usize) -> bool {
            self.inner.borrow().acked.get(&hart) == Some(&epoch)
        }

        fn release_park(&self, hart: usize) {
            let mut state = self.inner.borrow_mut();
            state.releases.push(hart);
            state.outstanding.remove(&hart);
            state.acked.remove(&hart);
        }

        fn ack_budget_ticks(&self) -> u64 {
            self.budget
        }

        fn now_ticks(&self) -> u64 {
            let mut state = self.inner.borrow_mut();
            state.clock += 1;
            state.clock
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeHarts;
    use super::*;

    /// Acquire, expecting a refusal. `Result<Guard, _>` has no `PartialEq`, so
    /// the refusal is unwrapped here and compared as a `QuiesceError`.
    fn acquire_err(state: &QuiesceState, harts: &dyn QuiesceHarts) -> QuiesceError {
        match state.acquire(harts) {
            Ok(guard) => panic!(
                "expected a refusal, got a guard (pending={})",
                guard.pending_count()
            ),
            Err(err) => err,
        }
    }

    #[test]
    fn quiesce_single_hart_is_a_no_op() {
        // No hook at all: with one hart there is nothing to park, so the
        // missing hook must not turn the no-op path into a refusal.
        let harts = FakeHarts::new(&[0], 0).with_hook(false);
        let state = QuiesceState::new();
        let guard = state.acquire(&harts).expect("one hart is a no-op");
        assert!(guard.all_parked(), "vacuous predicate holds with no targets");
        assert_eq!(guard.pending_count(), 0);
        assert!(harts.requests().is_empty(), "no hart may be asked to park");
        assert_eq!(harts.clock_ticks(), 0, "no wait may be entered");
        drop(guard);
        assert!(harts.releases().is_empty(), "nothing was parked to release");
        assert!(harts.outstanding().is_empty());
    }

    #[test]
    fn quiesce_all_acknowledged_harts_park_and_the_predicate_is_verified() {
        let harts = FakeHarts::new(&[0, 1, 2], 0);
        harts.ack_on_request(&[1, 2]);
        let state = QuiesceState::new();
        let mut guard = state.acquire(&harts).expect("both targets acknowledge");
        assert!(guard.all_parked());
        assert_eq!(guard.pending_count(), 0);
        assert_eq!(harts.parked(), vec![1, 2]);
        assert_eq!(
            harts.requests().len(),
            2,
            "exactly the two target harts are asked"
        );
        assert!(harts.releases().is_empty(), "the guard holds the park");

        // A hart that leaves its safe point makes the predicate false: it is
        // derived from the hart set, not remembered from the request.
        harts.unack(2);
        assert!(!guard.all_parked());
        assert_eq!(guard.pending_count(), 1);
        guard.release();
        assert_eq!(harts.releases(), vec![1, 2]);
        assert!(harts.parked().is_empty());
        assert!(harts.outstanding().is_empty());
    }

    #[test]
    fn quiesce_partial_acknowledgement_times_out_and_restores_the_harts() {
        let harts = FakeHarts::new(&[0, 1, 2], 0).with_budget(8);
        harts.ack_on_request(&[1]);
        let state = QuiesceState::new();
        assert_eq!(
            acquire_err(&state, &harts),
            QuiesceError::Timeout { pending: 1 }
        );
        assert_eq!(
            harts.releases(),
            vec![1, 2],
            "the silent hart's request is cancelled too"
        );
        assert!(harts.outstanding().is_empty(), "no request is left behind");
        assert!(harts.parked().is_empty());
        assert!(
            harts.clock_ticks() <= 10,
            "the wait must be bounded by the budget: {}",
            harts.clock_ticks()
        );
        assert!(
            harts.clock_ticks() >= 2,
            "the wait must actually wait for the acknowledgement"
        );

        // The claim was freed with the release: a later capture can succeed.
        harts.ack_on_request(&[2]);
        let guard = state
            .acquire(&harts)
            .expect("the failed attempt left no claim behind");
        drop(guard);
    }

    #[test]
    fn quiesce_release_is_idempotent() {
        let harts = FakeHarts::new(&[0, 1, 2], 0);
        harts.ack_on_request(&[1, 2]);
        let state = QuiesceState::new();
        let mut guard = state.acquire(&harts).expect("all acknowledge");
        guard.release();
        guard.release();
        assert_eq!(
            harts.releases(),
            vec![1, 2],
            "the second release must not touch the harts again"
        );
        drop(guard);
        assert_eq!(harts.releases(), vec![1, 2]);
        // And the claim stays free, so a later request is not refused.
        assert!(state.acquire(&harts).is_ok());
    }

    #[test]
    fn quiesce_never_requests_the_requester_hart() {
        // The requester is hart 1 and it is *not* in `online`; the request must
        // still go to hart 0 only, and must succeed without hart 1 ever
        // acknowledging anything.
        let harts = FakeHarts::new(&[0], 1);
        harts.ack_on_request(&[0]);
        let state = QuiesceState::new();
        let mut guard = state.acquire(&harts).expect("target acknowledges");
        assert!(guard.all_parked());
        let requests = harts.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0, 0, "only the non-requester is asked");
        assert!(harts.parked().iter().all(|hart| *hart != 1));
        guard.release();
        assert_eq!(harts.releases(), vec![0]);
    }

    #[test]
    fn quiesce_refuses_a_second_concurrent_request() {
        let harts = FakeHarts::new(&[0, 1], 0);
        harts.ack_on_request(&[1]);
        let state = QuiesceState::new();
        let guard = state.acquire(&harts).expect("target acknowledges");
        assert_eq!(harts.requests().len(), 1);
        assert_eq!(acquire_err(&state, &harts), QuiesceError::AlreadyRequested);
        assert_eq!(
            harts.requests().len(),
            1,
            "the refused request must not reach the hart set"
        );
        drop(guard);
        let guard = state.acquire(&harts).expect("the claim is free again");
        assert_eq!(harts.requests().len(), 2);
        drop(guard);
    }

    #[test]
    fn quiesce_refuses_multi_hart_without_a_park_hook() {
        let harts = FakeHarts::new(&[0, 1, 2], 0).with_hook(false);
        harts.ack_on_request(&[1, 2]);
        let state = QuiesceState::new();
        assert_eq!(acquire_err(&state, &harts), QuiesceError::Unsupported);
        assert!(
            harts.requests().is_empty(),
            "a request no hart can answer must not be issued at all"
        );
        assert_eq!(harts.clock_ticks(), 0, "nothing may be waited out");
        assert!(harts.releases().is_empty());
        // The claim was released, so the refusal is not sticky.
        assert_eq!(acquire_err(&state, &harts), QuiesceError::Unsupported);
    }

    #[test]
    fn quiesce_refuses_more_harts_than_it_can_track() {
        let online: Vec<usize> = (0..=MAX_QUIESCE_HARTS).collect();
        let harts = FakeHarts::new(&online, 0);
        harts.ack_on_request(&online);
        let state = QuiesceState::new();
        assert_eq!(acquire_err(&state, &harts), QuiesceError::TooManyHarts);
        assert!(harts.requests().is_empty());
        assert!(harts.releases().is_empty());
        // The claim was released with the refusal.
        assert_eq!(acquire_err(&state, &harts), QuiesceError::TooManyHarts);
    }
}
