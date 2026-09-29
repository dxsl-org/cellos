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
//! # The per-hart park hook
//!
//! A hart can only acknowledge a park request if something on that hart
//! observes the request, reaches a safe point and publishes the
//! acknowledgement. On RV64 that observation point is the trap path
//! ([`park_here_if_requested`], called from `task::vi_timer_tick`): a hart that
//! is idle in `wfi` still takes the requester's IPI, and a hart that is running
//! can be brought to a trap by it, so no hart has to be *already* cooperating to
//! be parked. [`KernelHarts::park_hook_available`] reports `true` there and only
//! there — a target whose trap path does not call the hook would silently never
//! acknowledge, and the protocol must refuse such a hart set outright rather than
//! wait out a request that cannot be answered. A single-hart system needs no
//! hook: the protocol is then a no-op, because the requester is the only hart
//! that could mutate the image.

use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
            Self::Unsupported => {
                "no per-hart park hook: a multi-hart request cannot be acknowledged"
            }
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

// ── The live park state and the trap-path hook ───────────────────────────────

/// Park state is kept for every hart id the protocol can name ([`HartSet`]'s
/// bound, which the kernel's own `smp::MAX_HARTS` sits under), so no id a hart
/// set can carry is left without state.
const PARK_HARTS: usize = MAX_QUIESCE_HARTS;

/// The epoch of the latest park request for each hart. `0` means "no request".
static PARK_REQUEST: [AtomicUsize; PARK_HARTS] = [const { AtomicUsize::new(0) }; PARK_HARTS];

/// The latest epoch each hart has stopped for, published by the target hart at
/// the safe point itself. Read by the requester's predicate, so it is monotone
/// and never cleared: the epoch space is what makes a stale entry harmless — a
/// later request has a strictly greater epoch and is not covered by it.
static PARK_ACK: [AtomicUsize; PARK_HARTS] = [const { AtomicUsize::new(0) }; PARK_HARTS];

/// The latest epoch released by a requester. A target parks for request `E` only
/// while `PARK_RELEASE < E`, so this single store is both "resume" and "cancel a
/// request that was never satisfied". It is monotone (`fetch_max`), which is what
/// makes [`release_park`] idempotent and safe for a hart that never acknowledged.
static PARK_RELEASE: [AtomicUsize; PARK_HARTS] = [const { AtomicUsize::new(0) }; PARK_HARTS];

/// How long a parked hart stays parked with no release: 2 s of `mtime`. A
/// requester that dies between parking a hart and releasing it never runs the
/// guard's `Drop` (the kernel is built `panic = "abort"`), so the parked loop
/// has to be able to give up on its own; the capture path commits nothing before
/// it finishes, so resuming is consistent — the alternative is a permanent hang.
#[cfg(target_arch = "riscv64")]
const PARK_ABANDON_TICKS: u64 = 200 * hal::common::timer::TICKS_PER_10MS;

/// Clock-independent backstop on the parked loop, for a build whose clock cannot
/// move. The RV64 value is deliberately far above `PARK_ABANDON_TICKS` — the
/// deadline is the real bound — and the host value is small so a unit test that
/// parks without a release terminates in constant time instead of hanging.
#[cfg(target_arch = "riscv64")]
const PARK_SPIN_LIMIT: usize = 1 << 34;
#[cfg(not(target_arch = "riscv64"))]
const PARK_SPIN_LIMIT: usize = 1 << 10;

/// What the trap path did with the park request it observed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParkOutcome {
    /// No park request was outstanding for this hart.
    Idle,
    /// The hart stopped, published its acknowledgement, and the requester
    /// released it.
    Parked { epoch: usize },
    /// A request existed but had already been released when this hart reached a
    /// safe point, so nothing was parked and no acknowledgement was published.
    AlreadyReleased { epoch: usize },
    /// The park loop gave up waiting for a release (`PARK_ABANDON_TICKS` or
    /// `PARK_SPIN_LIMIT`): the requester is gone. The acknowledgement, if one
    /// was published, is not withdrawn — the epoch has moved on by then.
    Abandoned { epoch: usize },
    /// Test-hooks only: this hart was told to withhold its acknowledgement, to
    /// reach the requester's fail-closed path without a real stall.
    Withheld { epoch: usize },
}

/// Ask `hart` to park at its next safe point, and return the epoch it must
/// acknowledge. Also delivers the IPI that brings the request to a hart that is
/// already idle in `wfi`.
fn request_park(hart: usize) -> usize {
    let Some(request) = PARK_REQUEST.get(hart) else {
        return 0;
    };
    let epoch = request.fetch_add(1, Ordering::AcqRel) + 1;
    #[cfg(target_arch = "riscv64")]
    {
        // A hart cannot interrupt itself, and the protocol never makes the
        // requester a target. Sending to a self-mapped id would be harmless but
        // pointless; skip it rather than depend on the SBI call tolerating it.
        if hart != crate::task::hart_local::current_hart_id() {
            if let Some((mask, base)) = crate::task::smp::logical_sbi_target(hart) {
                let _ = hal::common::sbi::sbi_send_ipi(mask, base);
            }
        }
    }
    epoch
}

/// Has `hart` stopped for `epoch`?
fn park_acknowledged(hart: usize, epoch: usize) -> bool {
    epoch != 0
        && PARK_ACK
            .get(hart)
            .is_some_and(|ack| ack.load(Ordering::Acquire) >= epoch)
}

/// Cancel `hart`'s outstanding request and resume it if it parked for it.
///
/// Idempotent by construction: the release epoch only ever moves forward, and a
/// hart that never acknowledged simply finds its request already released when it
/// reaches a safe point (see [`ParkOutcome::AlreadyReleased`]).
fn release_park(hart: usize) {
    let Some(request) = PARK_REQUEST.get(hart) else {
        return;
    };
    let Some(release) = PARK_RELEASE.get(hart) else {
        return;
    };
    release.fetch_max(request.load(Ordering::Acquire), Ordering::AcqRel);
}

/// Observe an outstanding park request on `hart` and, if there is one, stop this
/// hart at the trap path until the requester releases it.
///
/// This is the target half of [`QuiesceHarts`] on RV64. It is called from
/// `task::vi_timer_tick`, i.e. from the S-mode trap handler, for every trap —
/// the timer tick that every online hart already takes every 10 ms, and the
/// requester's IPI, which is what reaches a hart that is otherwise idle in `wfi`.
///
/// # Why the trap path is the safe point
///
/// 1. **Every hart passes it.** The timer is armed on every hart and an IPI is
///    taken by a hart in `wfi` (interrupts are enabled there), so a request
///    cannot be answered only by an already-cooperating hart.
/// 2. **No kernel lock is held.** A trap is only taken with `sstatus.SIE` set,
///    and `crate::sync::Spinlock` clears `SIE` for the whole life of its guard —
///    so the interrupted context cannot be inside `SCHEDULER`,
///    `FRAME_ALLOCATOR` or any other kernel spin lock, and those locks (which
///    the requester's own path needs) are guaranteed free. The trap handler takes
///    no lock before this call: it is placed ahead of `tick()`, the console poll
///    and `yield_cpu()`.
/// 3. **No frame allocation is in flight**, for the same reason —
///    `FRAME_ALLOCATOR` is a `crate::sync::Spinlock`, so an interrupted context
///    cannot be inside it.
/// 4. **Able to resume.** The vector's trap frame already holds the interrupted
///    context in full; leaving the loop returns through the handler and `sret`
///    restores the same registers, stack and privilege level. Nothing in the
///    scheduler moved, so no switch bookkeeping has to be repaired.
/// 5. **It stays stopped.** `SIE` is clear for the whole trap, so a parked hart
///    takes no further trap and cannot run any code but this loop until it is
///    released.
///
/// The residual is stated rather than papered over: `SIE`-set kernel code that
/// *allocates* holds the heap's `spinning_top` lock, which does **not** mask
/// interrupts, so a park can land on a hart holding that lock (the reachable RV64
/// cases are the boot path and a cooperative `yield_cpu` caller). The requester
/// must therefore not allocate while it holds the guard, and the requester's own
/// bounded wait is what keeps a mistake here from becoming a hang.
///
/// # The acknowledgement
///
/// The hart publishes [`PARK_ACK`] *before* it stops and with a release store,
/// and it cannot take a trap between that store and the loop, so a requester that
/// observes the acknowledgement knows the hart is not running anything else.
/// Publishing after the loop instead would claim a park that had already been
/// missed — the epoch could be released before the store landed.
///
/// The loop exits when [`PARK_RELEASE`] reaches the parked epoch. It is bounded
/// twice over (`PARK_ABANDON_TICKS`, `PARK_SPIN_LIMIT`) because the requester
/// may be gone; an abandoned park is logged rather than left as a hang.
#[inline]
pub fn park_here_if_requested(hart: usize) -> ParkOutcome {
    let Some(request) = PARK_REQUEST.get(hart) else {
        return ParkOutcome::Idle;
    };
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    lane::note_trap(hart);

    let epoch = request.load(Ordering::Acquire);
    if epoch == 0 {
        return ParkOutcome::Idle;
    }
    let released = || {
        PARK_RELEASE
            .get(hart)
            .is_some_and(|release| release.load(Ordering::Acquire) >= epoch)
    };
    if released() {
        #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
        lane::note_release_seen(hart, epoch);
        return ParkOutcome::AlreadyReleased { epoch };
    }
    #[cfg(any(feature = "test-hooks", test))]
    if park_ack_withheld(hart) {
        #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
        lane::note_withheld(hart, epoch);
        return ParkOutcome::Withheld { epoch };
    }

    // I am stopping: publish that, with everything that must not change while I
    // am parked already committed.
    PARK_ACK[hart].store(epoch, Ordering::Release);
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    lane::note_stopped(hart, epoch);

    #[cfg(target_arch = "riscv64")]
    let deadline = hal::common::timer::read_mtime().saturating_add(PARK_ABANDON_TICKS);
    let mut spins = 0usize;
    loop {
        if released() {
            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            lane::note_resumed(hart, epoch);
            return ParkOutcome::Parked { epoch };
        }
        #[cfg(target_arch = "riscv64")]
        if hal::common::timer::read_mtime() > deadline {
            break;
        }
        spins += 1;
        if spins >= PARK_SPIN_LIMIT {
            break;
        }
        core::hint::spin_loop();
    }
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    lane::note_abandoned(hart, epoch);
    log::warn!(
        "[quiesce] hart {hart} left the parked loop without a release at epoch {epoch}: \
         the requester is gone; resuming"
    );
    ParkOutcome::Abandoned { epoch }
}

#[cfg(any(feature = "test-hooks", test))]
static PARK_ACK_WITHHELD: [AtomicBool; PARK_HARTS] = [const { AtomicBool::new(false) }; PARK_HARTS];

/// Test-hooks: make `hart` observe park requests without ever acknowledging one,
/// so the requester's fail-closed path (bounded wait, release, no park) is
/// reachable without waiting for a real hart to stall.
#[cfg(any(feature = "test-hooks", test))]
pub fn set_park_ack_withheld(hart: usize, withheld: bool) {
    if let Some(flag) = PARK_ACK_WITHHELD.get(hart) {
        flag.store(withheld, Ordering::Release);
    }
}

#[cfg(any(feature = "test-hooks", test))]
fn park_ack_withheld(hart: usize) -> bool {
    PARK_ACK_WITHHELD
        .get(hart)
        .is_some_and(|flag| flag.load(Ordering::Acquire))
}

// ── The live kernel hart set ─────────────────────────────────────────────────

/// The kernel's own hart set.
pub struct KernelHarts;

impl QuiesceHarts for KernelHarts {
    fn requester(&self) -> usize {
        crate::task::hart_local::current_hart_id()
    }

    fn online_harts(&self, out: &mut HartSet) -> Result<(), QuiesceError> {
        online_hart_ids(self.requester(), out)
    }

    /// Present on RV64, where the trap path calls [`park_here_if_requested`] on
    /// every trap: a target then answers a request by itself, and the requester's
    /// bounded wait is a real wait for a real acknowledgement. Anywhere else no
    /// hart observes a request, so a multi-hart request must be refused outright
    /// instead of burning a budget on an acknowledgement that cannot arrive.
    fn park_hook_available(&self) -> bool {
        cfg!(target_arch = "riscv64")
    }

    fn request_park(&self, hart: usize) -> usize {
        request_park(hart)
    }

    fn park_acked(&self, hart: usize, epoch: usize) -> bool {
        park_acknowledged(hart, epoch)
    }

    fn release_park(&self, hart: usize) {
        release_park(hart);
    }

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

/// The logical id of the hart that runs `kmain`. It never publishes
/// `smp::HART_ONLINE` (`task::init` installs it as hart 0 before there is an SMP
/// layer to publish into), so the online set has to name it explicitly.
const BOOT_HART: usize = 0;

/// Record every hart that can run kernel code for a request issued by
/// `requester`, including the boot hart.
///
/// The boot hart never publishes `smp::HART_ONLINE` — it is running before there
/// is anything to publish into — so the published secondary list alone is an
/// incomplete online set. The requester used to be unioned in to cover that,
/// which is only enough while the requester *is* the boot hart: a capture
/// requested from hart 1 would leave hart 0 out of the target set, and a hart
/// executing kernel code that can mutate the image would never be asked to stop.
fn online_hart_ids(requester: usize, out: &mut HartSet) -> Result<(), QuiesceError> {
    out.insert(BOOT_HART)?;
    out.insert(requester)?;
    for hart in crate::task::smp::online_harts() {
        out.insert(hart)?;
    }
    Ok(())
}

/// The kernel's single-flight protocol instance.
pub static KERNEL_STATE: QuiesceState = QuiesceState::new();

/// The kernel's hart set.
pub static KERNEL_HARTS: KernelHarts = KernelHarts;

// ── The RV64 lane fixture for the park hook ──────────────────────────────────

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub mod lane {
    //! Boot fixture for the RV64 lane: the park hook, driven end to end on real
    //! harts, plus its fail-closed path.
    //!
    //! The requester half runs here on the boot hart — the only hart that can
    //! drive a fixture synchronously before the shell exists — and the target half
    //! is whatever real hart the request reaches through the trap path. Nothing in
    //! this module is a model of the mechanism: it calls the same
    //! [`KernelHarts`](super::KernelHarts)/[`QuiesceState`](super::QuiesceState)
    //! protocol the capture preflight calls, over the same IPI and the same
    //! trap-path hook.
    //!
    //! Witness shape. Each hart says what *it* did, so no line has to be trusted
    //! as a summary of another hart:
    //!
    //! * `hart=1 state=withheld epoch=E` — the target saw the request and was made
    //!   not to acknowledge it, which is the negative case's precondition;
    //! * `hart=0 state=refused pending=1` — the requester's bounded wait expired
    //!   and it refused: fail-closed, no success, no disk commit;
    //! * `hart=1 state=release-observed epoch=E` — the release reached a hart that
    //!   never acknowledged, i.e. the cancelled request left nothing behind;
    //! * `hart=1 state=parked epoch=E` / `hart=0 state=all-parked pending=0` —
    //!   target and requester agree the hart stopped;
    //! * `hart=0 state=proceed frozen_ticks=A->B` — the requester kept running
    //!   while the target's own trap counter stayed frozen, so "parked" is
    //!   observed, not asserted;
    //! * `hart=1 state=resumed epoch=E` and the counter moving again — the hart
    //!   went back to running its own code.
    //!
    //! The fixture takes `&mut` nothing and allocates nothing: it runs in `kmain`
    //! before the shell exists.

    use super::*;

    /// One 10 ms tick of `mtime`, in `mtime` units.
    const TICK: u64 = hal::common::timer::TICKS_PER_10MS;

    /// Traps observed on each hart: the frozen-window witness. A parked hart
    /// takes no trap, so this counter cannot move while it is parked, and it moves
    /// again the moment the hart is released.
    static TRAP_TICKS: [AtomicUsize; PARK_HARTS] = [const { AtomicUsize::new(0) }; PARK_HARTS];
    static STOPPED: [AtomicUsize; PARK_HARTS] = [const { AtomicUsize::new(0) }; PARK_HARTS];
    static RESUMED: [AtomicUsize; PARK_HARTS] = [const { AtomicUsize::new(0) }; PARK_HARTS];
    static ABANDONED: [AtomicUsize; PARK_HARTS] = [const { AtomicUsize::new(0) }; PARK_HARTS];
    static WITHHELD: [AtomicUsize; PARK_HARTS] = [const { AtomicUsize::new(0) }; PARK_HARTS];
    static RELEASE_SEEN: [AtomicUsize; PARK_HARTS] = [const { AtomicUsize::new(0) }; PARK_HARTS];
    /// Last epoch each one-shot marker was logged for, so a request that stays
    /// outstanding across several 10 ms traps logs its observation once. One entry
    /// per hart *and* per event: the two events can name the same epoch (a request
    /// is withheld and then cancelled), and a shared key would drop the second.
    static LOGGED_WITHHELD: [AtomicUsize; PARK_HARTS] = [const { AtomicUsize::new(0) }; PARK_HARTS];
    static LOGGED_RELEASE: [AtomicUsize; PARK_HARTS] = [const { AtomicUsize::new(0) }; PARK_HARTS];

    fn bump(counters: &[AtomicUsize; PARK_HARTS], hart: usize) {
        if let Some(counter) = counters.get(hart) {
            counter.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn count(counters: &[AtomicUsize; PARK_HARTS], hart: usize) -> usize {
        counters.get(hart).map_or(0, |c| c.load(Ordering::Acquire))
    }

    pub(super) fn note_trap(hart: usize) {
        bump(&TRAP_TICKS, hart);
    }

    pub(super) fn note_stopped(hart: usize, epoch: usize) {
        bump(&STOPPED, hart);
        log::warn!("[selftest] S22-RV64-PARK: hart={hart} state=parked epoch={epoch}");
    }

    pub(super) fn note_resumed(hart: usize, epoch: usize) {
        bump(&RESUMED, hart);
        log::warn!("[selftest] S22-RV64-PARK: hart={hart} state=resumed epoch={epoch}");
    }

    pub(super) fn note_abandoned(hart: usize, epoch: usize) {
        bump(&ABANDONED, hart);
        log::warn!("[selftest] S22-RV64-PARK: hart={hart} state=abandoned epoch={epoch}");
    }

    pub(super) fn note_withheld(hart: usize, epoch: usize) {
        bump(&WITHHELD, hart);
        log_once(&LOGGED_WITHHELD, hart, epoch, "withheld");
    }

    pub(super) fn note_release_seen(hart: usize, epoch: usize) {
        if let Some(seen) = RELEASE_SEEN.get(hart) {
            // The release is observed on every later trap too; the counter records
            // the last release this hart noticed, the marker fires once per epoch.
            seen.fetch_max(epoch, Ordering::AcqRel);
        }
        log_once(&LOGGED_RELEASE, hart, epoch, "release-observed");
    }

    fn log_once(logged_for: &[AtomicUsize; PARK_HARTS], hart: usize, epoch: usize, state: &str) {
        let Some(logged) = logged_for.get(hart) else {
            return;
        };
        if logged.swap(epoch, Ordering::AcqRel) != epoch {
            log::warn!("[selftest] S22-RV64-PARK: hart={hart} state={state} epoch={epoch}");
        }
    }

    /// Spin until `predicate` holds or `ticks` of `mtime` have passed.
    fn wait_for(ticks: u64, mut predicate: impl FnMut() -> bool) -> bool {
        let deadline = hal::common::timer::read_mtime() + ticks;
        loop {
            if predicate() {
                return true;
            }
            if hal::common::timer::read_mtime() > deadline {
                return false;
            }
            core::hint::spin_loop();
        }
    }

    /// Let `ticks` of `mtime` pass without taking any lock or allocating — the
    /// requester's side of the frozen window.
    fn spin_ticks(ticks: u64) {
        let deadline = hal::common::timer::read_mtime() + ticks;
        while hal::common::timer::read_mtime() <= deadline {
            core::hint::spin_loop();
        }
    }

    fn terminal(ok: bool, harts: usize, parked: usize, resumed: usize, refused: usize) -> bool {
        log::warn!(
            "[selftest] S22-RV64-PARK: hart=0 state=summary harts={harts} parked={parked} \
             resumed={resumed} refused={refused}"
        );
        if ok {
            // The terminal stays exactly `... PASS harts=N` so the lane's
            // exact-match terminal assertion cannot be satisfied by a partial run;
            // the counts above are detail, not the verdict.
            log::warn!("S22-RV64-PARK: PASS harts={harts}");
        } else {
            log::warn!("S22-RV64-PARK: FAIL harts={harts}");
        }
        ok
    }

    /// Drive the park hook on the harts this boot actually has, and report whether
    /// every step held.
    ///
    /// Returns `false` after emitting its own verdict marker: `UNAVAILABLE` when
    /// the readiness gate found the target not taking traps before any request was
    /// made (no park was witnessed in this boot), or `FAIL` when a step of the
    /// witness itself did not hold. The caller only has to keep that marker
    /// visible; the lane decides on the markers, not on this value.
    pub fn run_primary() -> bool {
        if crate::task::hart_local::current_hart_id() != BOOT_HART {
            return true;
        }
        let target = match crate::task::smp::online_harts().find(|hart| *hart != BOOT_HART) {
            Some(hart) => hart,
            None => {
                // One hart: no target, no request, no wait — the protocol's no-op
                // arm, taken through the live state and the live hart set. It must
                // succeed with the single-flight claim and must never ask the
                // requester to park itself, or a single-hart capture would deadlock
                // on its own acknowledgement.
                let ok = match super::KERNEL_STATE.acquire(&super::KERNEL_HARTS) {
                    Ok(guard) => guard.all_parked() && guard.pending_count() == 0,
                    Err(_) => false,
                };
                let asked = PARK_REQUEST[BOOT_HART].load(Ordering::Acquire) != 0
                    || count(&STOPPED, BOOT_HART) != 0;
                if !ok || asked {
                    log::warn!(
                        "[selftest] S22-RV64-PARK: hart=0 state=single-hart-arm-failed \
                         noop_ok={ok} requester_asked={asked}"
                    );
                    return terminal(false, 1, 0, 0, 0);
                }
                log::warn!("[selftest] S22-RV64-PARK: hart=0 state=no-targets (single-hart no-op)");
                return terminal(true, 1, 0, 0, 0);
            }
        };

        // ── 0. Readiness gate, *before* this fixture asks the target for
        // anything. A boot in which the second hart has already stopped taking
        // traps is the known mid-boot condition the rest of this lane tolerates
        // (phase-02's "a remote hart can stop acknowledging mid-boot": a hart can
        // be held non-preemptible for seconds, and in one observed boot hart 0 and
        // then hart 1 were both reported silent before this fixture ran). It is
        // not a statement about the park hook, and it cannot be one: no park
        // request has been issued yet — `PARK_REQUEST[target]` is still 0 — so the
        // hook has had nothing to answer and nothing it could have broken. What a
        // *post*-request silence means is the opposite, and stays a hard failure
        // below, which is what keeps this gate from hiding a hook that parks a hart
        // and never releases it.
        let baseline = count(&TRAP_TICKS, target);
        if !wait_for(30 * TICK, || count(&TRAP_TICKS, target) > baseline + 1) {
            log::warn!(
                "[selftest] S22-RV64-PARK: hart={target} state=not-taking-traps \
                 ticks={baseline} (before any request)"
            );
            log::warn!("S22-RV64-PARK: UNAVAILABLE harts=2 reason=target-not-taking-traps");
            return false;
        }

        // ── 1. The negative case: the target observes the request and withholds
        // its acknowledgement, so the requester must fail closed.
        super::set_park_ack_withheld(target, true);
        let refused = match super::KERNEL_STATE.acquire(&super::KERNEL_HARTS) {
            Ok(guard) => {
                let pending = guard.pending_count();
                drop(guard);
                log::warn!(
                    "[selftest] S22-RV64-PARK: hart=0 state=unexpected-park pending={pending}"
                );
                None
            }
            Err(err) => Some(err),
        };
        super::set_park_ack_withheld(target, false);
        let cancelled = PARK_REQUEST[target].load(Ordering::Acquire);
        let refused_ok = matches!(refused, Some(QuiesceError::Timeout { pending: 1 }));
        if !refused_ok {
            log::warn!(
                "[selftest] S22-RV64-PARK: hart=0 state=refused-shape-unknown \
                 error={:?}",
                refused
            );
            return terminal(false, 2, 0, 0, 0);
        }
        log::warn!(
            "[selftest] S22-RV64-PARK: hart=0 state=refused pending=1 epoch={cancelled} \
             fail-closed"
        );
        if count(&WITHHELD, target) == 0 {
            log::warn!("[selftest] S22-RV64-PARK: hart={target} state=never-observed-request");
            return terminal(false, 2, 0, 0, 1);
        }

        // ── 2. The release reaches a hart that never acknowledged: the cancelled
        // request leaves no park behind, and the target is running its own code.
        let observed = wait_for(50 * TICK, || count(&RELEASE_SEEN, target) >= cancelled);
        if !observed {
            log::warn!(
                "[selftest] S22-RV64-PARK: hart={target} state=release-not-observed \
                 epoch={cancelled}"
            );
            return terminal(false, 2, 0, 0, 1);
        }

        // ── 3. The positive case: a request the target answers.
        let want_resumed = count(&RESUMED, target) + 1;
        let guard = match super::KERNEL_STATE.acquire(&super::KERNEL_HARTS) {
            Ok(guard) => guard,
            Err(err) => {
                log::warn!("[selftest] S22-RV64-PARK: hart=0 state=park-refused error={err}");
                return terminal(false, 2, 0, 0, 1);
            }
        };
        let epoch = PARK_REQUEST[target].load(Ordering::Acquire);
        if !guard.all_parked() || count(&STOPPED, target) == 0 {
            let pending = guard.pending_count();
            drop(guard);
            log::warn!(
                "[selftest] S22-RV64-PARK: hart=0 state=not-all-parked pending={pending} \
                 epoch={epoch}"
            );
            return terminal(false, 2, count(&STOPPED, target), 0, 1);
        }
        log::warn!("[selftest] S22-RV64-PARK: hart=0 state=all-parked pending=0 epoch={epoch}");

        // The requester proceeds while the target is parked, and proves the park
        // was real: the target's own trap counter cannot move while it is stopped.
        let before = count(&TRAP_TICKS, target);
        spin_ticks(4 * TICK);
        let after = count(&TRAP_TICKS, target);
        let frozen = before == after;
        log::warn!(
            "[selftest] S22-RV64-PARK: hart=0 state=proceed frozen_ticks={before}->{after} \
             window_ticks=4"
        );

        drop(guard);
        log::warn!("[selftest] S22-RV64-PARK: hart=0 state=released epoch={epoch}");

        // ── 4. The target resumes: the release is observed and the same hart takes
        // traps again, which is what "resumed" means.
        let resumed = wait_for(50 * TICK, || {
            count(&RESUMED, target) >= want_resumed && count(&TRAP_TICKS, target) > after
        });
        if !resumed {
            log::warn!(
                "[selftest] S22-RV64-PARK: hart={target} state=not-resumed restored={} ticks={}",
                count(&RESUMED, target),
                count(&TRAP_TICKS, target)
            );
        }
        let ok = frozen && resumed && count(&ABANDONED, target) == 0;
        terminal(ok, 2, count(&STOPPED, target), count(&RESUMED, target), 1)
    }
}

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
        assert!(
            guard.all_parked(),
            "vacuous predicate holds with no targets"
        );
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

    // ── The live park state machine ──────────────────────────────────────────
    //
    // `FakeHarts` covers the protocol over a programmable hart set; these cases
    // cover the state machine the kernel actually runs — `request_park`,
    // `park_here_if_requested`, `release_park`, `park_acknowledged` over the live
    // per-hart epochs — on the host, where the only arch-specific parts (the IPI
    // and the clock) are compiled out. Each case owns hart ids above the kernel's
    // own `smp::MAX_HARTS` so the parallel test threads cannot see each other's
    // state; the host build has no clock, so a park that is never released ends at
    // the clock-independent backstop instead of hanging.

    #[test]
    fn live_request_is_not_an_acknowledgement_and_an_abandoned_park_has_stopped() {
        let hart = 3;
        let epoch = request_park(hart);
        assert_ne!(epoch, 0);
        assert!(
            !park_acknowledged(hart, epoch),
            "asking a hart to park is not evidence that it parked"
        );

        let outcome = park_here_if_requested(hart);
        assert_eq!(
            outcome,
            ParkOutcome::Abandoned { epoch },
            "with no release and no host clock the parked loop must end at its backstop"
        );
        assert!(
            park_acknowledged(hart, epoch),
            "the acknowledgement is published before the hart stops, so it is already visible"
        );

        // The release is idempotent, and the cancelled epoch is what the hart now
        // observes: it is resumed, and it does not park for that epoch again.
        release_park(hart);
        release_park(hart);
        assert_eq!(
            park_here_if_requested(hart),
            ParkOutcome::AlreadyReleased { epoch }
        );
        assert!(
            park_acknowledged(hart, epoch),
            "the epoch was parked for; a release resumes the hart, it does not un-park history"
        );
    }

    #[test]
    fn live_release_cancels_a_request_the_hart_never_satisfied() {
        let hart = 4;
        let epoch = request_park(hart);
        release_park(hart);
        assert_eq!(
            park_here_if_requested(hart),
            ParkOutcome::AlreadyReleased { epoch },
            "a hart that reaches a safe point after the release must not park"
        );
        assert!(
            !park_acknowledged(hart, epoch),
            "no acknowledgement may be published for a cancelled request"
        );

        // The next request is a new epoch, so it is not covered by the old release.
        let next = request_park(hart);
        assert_eq!(next, epoch + 1);
        assert!(
            !park_acknowledged(hart, next),
            "a new epoch needs a new park"
        );
        assert_eq!(
            park_here_if_requested(hart),
            ParkOutcome::Abandoned { epoch: next }
        );
    }

    #[test]
    fn live_withheld_hart_observes_the_request_and_never_acknowledges() {
        let hart = 5;
        set_park_ack_withheld(hart, true);
        let epoch = request_park(hart);
        assert_eq!(
            park_here_if_requested(hart),
            ParkOutcome::Withheld { epoch },
            "the request is observed, and the withholding is what stops the park"
        );
        assert!(
            !park_acknowledged(hart, epoch),
            "the negative control must not acknowledge, or the requester's fail-closed \
             path would never be reached"
        );

        set_park_ack_withheld(hart, false);
        release_park(hart);
        assert_eq!(
            park_here_if_requested(hart),
            ParkOutcome::AlreadyReleased { epoch },
            "clearing the control leaves the hart restored, not parked"
        );
    }

    #[test]
    fn live_online_set_names_the_boot_hart_for_any_requester() {
        // The host cannot make `current_hart_id()` return a secondary hart, so the
        // requester is named explicitly here — which is the whole point of the
        // case: hart 0 runs kernel code without ever publishing `HART_ONLINE`, so a
        // request issued from hart 1 has to name it from outside the published
        // set. Before that, a capture requested from hart 1 would have left hart 0
        // running and reported success on a set it never froze.
        let mut set = HartSet::new();
        online_hart_ids(1, &mut set).expect("ids are bounded");
        let ids: Vec<usize> = set.iter().collect();
        assert!(
            ids.contains(&0),
            "the boot hart must be in the online set for a request from hart 1: {ids:?}"
        );
        assert!(ids.contains(&1), "the requester itself is online: {ids:?}");
    }
}
