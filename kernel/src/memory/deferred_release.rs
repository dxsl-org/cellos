//! Bounded deferred release of page-table frames whose tag invalidation was not
//! confirmed.
//!
//! A release path must never wait synchronously for a remote acknowledgement. A
//! peer hart can be held non-preemptible for seconds, and waiting for it either
//! burns the boot's own time budget (the 25 x 200 ms budget this replaces
//! truncated two-hart boots inside the grant-revoke fixture) or — if the wait
//! were shortened — would free a frame a remote hart can still resolve. So a
//! path that cannot confirm its tag invalidation keeps the frames here together
//! with the tag whose invalidation must land first, and
//! [`reap_deferred_releases`] completes the release from the timer path on hart 0
//! once the acknowledgement arrives.
//!
//! Fail-closed, in this order:
//!
//! 1. a frame is released **only** after the invalidation that covers it is
//!    acknowledged — never on a timeout, never silently;
//! 2. an entry that spends its bounded retry budget is quarantined (counted and
//!    logged) and its tag is recorded as unconfirmed for the rest of the boot, so
//!    [`tag_invalidation_unconfirmed`] keeps every dependent path refusing to
//!    reuse the address space;
//! 3. a deferral that finds no room at all is quarantined immediately, also
//!    loudly. A permanently unresponsive peer therefore leaks loudly and never
//!    silently.

use super::frame::OwnedFrame;
use crate::sync::Spinlock;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Deferred releases this queue can hold at once. Entries are merged per tag, so
/// the depth is bounded by the number of *unconfirmed tags* rather than by the
/// number of unmap calls, and a full queue means 32 distinct roots are waiting on
/// acknowledgements that are not arriving.
const MAX_DEFERRED_ENTRIES: usize = 32;

/// Tags known to be unconfirmed for the rest of the boot. Every live tag holds a
/// pool slot, and the pool is 256 wide, so the table can always record one entry
/// per tag.
const MAX_ABANDONED_TAGS: usize = 256;

/// Entries one reaper call touches. Small on purpose: the reaper runs in hart 0's
/// timer ISR and must not monopolize the slice.
const REAPER_ENTRIES_PER_CALL: usize = 2;

/// Reissue step, in reaper attempts. A remote hart that took the IPI but never
/// published its completion (or whose request was lost) only finishes when it is
/// asked again; checking alone would wait forever for an answer that is no longer
/// coming. Every other attempt is a pure check, so a responsive peer completes on
/// the first one.
const REAPER_REISSUE_EVERY: usize = 32;

/// Attempts one entry gets before the fail-closed quarantine. The reaper takes
/// one attempt per entry per call, and hart 0's timer calls it every 10 ms, so
/// this is about five seconds of grace — the window the old 25 x 200 ms
/// synchronous budget covered, without blocking any caller.
const REAPER_MAX_ATTEMPTS: usize = 512;

/// One retained release: frames whose invalidation is still unconfirmed, plus the
/// tag whose acknowledgement frees them.
struct DeferredRelease {
    asid: usize,
    domain: u64,
    /// The invalidation epoch this tag was last requested under, per hart
    /// (0 = this hart was never asked). Completion is "every asked hart has
    /// published that epoch", which is a statement about *this* tag — the
    /// earlier "does any hart owe any invalidation" test stayed true for as long
    /// as unrelated teardowns kept asking, and delayed every confirmation behind
    /// them.
    epochs: [usize; crate::task::smp::MAX_HARTS],
    /// Why the frames were retained. Kept verbatim so the quarantine it may end
    /// in names the same cause the release path would have.
    reason: &'static str,
    frames: Vec<OwnedFrame>,
    attempts: usize,
    /// Confirming the tag also returns its architectural-tag lease slot (a root
    /// teardown whose `AsidLease::release` could not confirm the invalidation).
    release_tag_slot: bool,
}

static DEFERRED: Spinlock<Vec<DeferredRelease>> = Spinlock::new(Vec::new());
/// Tags whose invalidation was given up on. They never become confirmed again,
/// because the only proof — an acknowledgement published after the PTE teardown
/// — is not coming. Kept separately from the queue so an abandoned tag does not
/// hold a queue slot the reaper can still use.
static ABANDONED_TAGS: Spinlock<Vec<usize>> = Spinlock::new(Vec::new());
static ABANDONED_SATURATED: AtomicBool = AtomicBool::new(false);
static ATTEMPTS_TOTAL: AtomicUsize = AtomicUsize::new(0);
static ABANDONED_ENTRIES: AtomicUsize = AtomicUsize::new(0);
static ABANDONED_FRAMES: AtomicUsize = AtomicUsize::new(0);
static LEAKED_STACK_FRAMES: AtomicUsize = AtomicUsize::new(0);

/// Retain `frames` until `asid`'s invalidation is confirmed.
///
/// The frames must already be detached from every structure that could hand them
/// back to the allocator: this is the last owner. The tag stays live (the caller
/// keeps its lease), so nothing can reuse it while the entry is pending.
pub fn defer_frames(asid: usize, domain: u64, frames: Vec<OwnedFrame>, reason: &'static str) {
    defer(asid, domain, frames, reason, false)
}

/// Retain `frames` **and** return the tag's lease slot once the invalidation is
/// confirmed. Used by a root teardown, whose `AsidLease::release` already gave up
/// its slot: without the handover the tag would be reserved forever while its
/// last frames were still unconfirmed.
pub fn defer_frames_and_tag(
    asid: usize,
    domain: u64,
    frames: Vec<OwnedFrame>,
    reason: &'static str,
) {
    defer(asid, domain, frames, reason, true)
}

fn defer(
    asid: usize,
    domain: u64,
    frames: Vec<OwnedFrame>,
    reason: &'static str,
    release_tag_slot: bool,
) {
    // An entry is recorded even with no frames: the record is what makes the tag
    // read unconfirmed, and a path that probed and did not get an acknowledgement
    // must not leave a tag that other paths would take for confirmed. (A rollback
    // can have detached no table at all and still have retired a leaf.)
    if tag_abandoned(asid) {
        // The reaper already gave up on this tag, so no acknowledgement will ever
        // confirm it. A fresh queue slot would only hide the leak; hand the frames
        // where its earlier frames went.
        super::address_space::quarantine_frames(frames, reason);
        return;
    }
    let count = frames.len();
    // The decision and the log are separated so the console lock never nests
    // inside the queue lock: the queue is taken from release paths and from the
    // timer ISR, and neither should wait on a UART.
    // The entry is what makes the tag read unconfirmed, so it must carry the
    // epochs it is waiting for from the moment it exists: an entry with no
    // request behind it would read as confirmed vacuously.
    let placement = place(asid, domain, reason, frames, release_tag_slot);
    request_tag_epochs(asid);
    match placement {
        Placement::Merged(total) => log::warn!(
            "[aspace] deferred release extended: tag={} total_frames={} reason={}",
            asid,
            total,
            reason
        ),
        Placement::Queued => log::warn!(
            "[aspace] deferred release queued: tag={} domain={} frames={} reason={} — the tag stays unconfirmed and every frame behind it is retained until an acknowledgement lands",
            asid,
            domain,
            count,
            reason
        ),
        Placement::Full(frames) => {
            mark_abandoned(asid);
            super::address_space::quarantine_frames(frames, reason);
        }
    }
}

/// Where a retention landed, so the caller can log and quarantine it once the
/// queue lock is released. `Full` hands the frames back: nothing retained them.
enum Placement {
    /// Appended to the tag's existing entry; that entry's new total.
    Merged(usize),
    /// A new entry for the tag.
    Queued,
    /// No room for a new tag.
    Full(Vec<OwnedFrame>),
}

fn place(
    asid: usize,
    domain: u64,
    reason: &'static str,
    frames: Vec<OwnedFrame>,
    release_tag_slot: bool,
) -> Placement {
    let mut queue = DEFERRED.lock();
    if let Some(index) = queue.iter().position(|entry| entry.asid == asid) {
        // One entry per tag: the invalidation is per tag, so a second retention
        // under the same tag is waiting on the same acknowledgement.
        let entry = &mut queue[index];
        entry.frames.extend(frames);
        entry.release_tag_slot |= release_tag_slot;
        Placement::Merged(entry.frames.len())
    } else if queue.len() >= MAX_DEFERRED_ENTRIES {
        Placement::Full(frames)
    } else {
        queue.push(DeferredRelease {
            asid,
            domain,
            epochs: [0; crate::task::smp::MAX_HARTS],
            reason,
            frames,
            attempts: 0,
            release_tag_slot,
        });
        Placement::Queued
    }
}

/// Is `asid`'s invalidation still unconfirmed?
///
/// True while a queue entry waits on it, and **permanently** true once the reaper
/// gives up on the tag. Every path that gates a reuse of the address space on a
/// confirmed invalidation (a grant drain, a re-publish over a retired VA) must ask
/// this instead of waiting: a `false` answer is the only proof that no hart can
/// still resolve a stale translation, and a missing acknowledgement must never
/// look like one.
pub fn tag_invalidation_unconfirmed(asid: usize) -> bool {
    if ABANDONED_SATURATED.load(Ordering::Acquire) {
        return true;
    }
    if ABANDONED_TAGS.lock().contains(&asid) {
        return true;
    }
    DEFERRED
        .lock()
        .iter()
        .any(|entry| entry.asid == asid && !tag_epochs_confirmed(entry))
}

/// Has every hart this tag was requested from published that epoch?
fn tag_epochs_confirmed(entry: &DeferredRelease) -> bool {
    // Test-only: the withheld window reports every tag unconfirmed, which is how
    // the release-path fixture reaches the retained-frames branch on a one-hart
    // boot where nothing remote can be outstanding.
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    if crate::memory::tlb_shootdown::test_tag_ack_withheld() {
        return false;
    }
    (0..crate::task::smp::MAX_HARTS)
        .all(|hart| crate::task::smp::tlb_flush_completed(hart, entry.epochs[hart]))
}

/// Ask every online remote hart to invalidate `asid` and remember the epochs.
///
/// The IPI is sent before the queue lock is taken: the answering hart does its
/// flush in its own trap path, and nothing in that path may be waiting on this
/// queue.
fn request_tag_epochs(asid: usize) {
    let me = crate::task::hart_local::current_hart_id();
    let mut requested = [0usize; crate::task::smp::MAX_HARTS];
    for hart in crate::task::smp::online_harts().filter(|hart| *hart != me) {
        requested[hart] = crate::task::smp::request_tlb_flush(hart);
    }
    if requested.iter().all(|epoch| *epoch == 0) {
        return;
    }
    let mut queue = DEFERRED.lock();
    if let Some(entry) = queue.iter_mut().find(|entry| entry.asid == asid) {
        for hart in 0..crate::task::smp::MAX_HARTS {
            if requested[hart] != 0 {
                // Monotonic per hart, so the newest request subsumes older ones.
                entry.epochs[hart] = requested[hart];
            }
        }
    }
}

/// Bounded drain for the timer path on hart 0.
///
/// Touches at most [`REAPER_ENTRIES_PER_CALL`] entries, one non-blocking attempt
/// each, and holds **no lock** across the acknowledgement machinery: the queue is
/// read and updated under its own short lock, and releasing the frames happens
/// outside it. It never waits on a remote hart, so it cannot widen the timer
/// ISR's interrupt window, and it is safe before the acknowledgement machinery is
/// ready — with no remote hart online every check is vacuous and an empty queue
/// makes the whole call a no-op (set at most two uncontended loads).
pub fn reap_deferred_releases() {
    let mut stepped: Option<usize> = None;
    for _ in 0..REAPER_ENTRIES_PER_CALL {
        let Some((asid, reissue)) = next_step() else {
            return;
        };
        // One attempt per entry per call, or a single waiting entry would spend
        // two attempts every tick and halve its own grace window.
        if stepped == Some(asid) {
            return;
        }
        stepped = Some(asid);
        if reissue {
            crate::memory::tlb_shootdown::flush_tag_local(asid);
            request_tag_epochs(asid);
            note_attempt(asid);
            continue;
        }
        let confirmed = DEFERRED
            .lock()
            .iter()
            .find(|entry| entry.asid == asid)
            .is_some_and(tag_epochs_confirmed);
        if confirmed {
            complete(asid);
        } else {
            note_attempt(asid);
        }
    }
}

/// The next entry to step, as `(asid, reissue)`. FIFO order keeps a steady stream
/// of failing entries from starving the older ones.
fn next_step() -> Option<(usize, bool)> {
    let queue = DEFERRED.lock();
    let entry = queue.first()?;
    Some((entry.asid, entry.attempts % REAPER_REISSUE_EVERY == 0))
}

/// The invalidation landed: return the frames to the allocator and, for a root
/// teardown, hand its tag slot back.
///
/// A concurrent deferral for the same tag merges only *after* its own probe spent
/// the probe deadline, so it cannot land inside this check-then-remove window: the
/// request the check depended on was issued after that deferral's PTE store, which
/// is what makes the acknowledgement cover it. (The test-only withhold seam skips
/// that deadline, and its fixture is single-threaded on one hart.)
fn complete(asid: usize) {
    let entry = {
        let mut queue = DEFERRED.lock();
        queue
            .iter()
            .position(|entry| entry.asid == asid)
            .map(|index| queue.remove(index))
    };
    let Some(entry) = entry else {
        return;
    };
    // Outside the queue lock: releasing a frame takes the frame allocator, and the
    // tag slot takes the lease pool.
    log::info!(
        "[aspace] deferred release confirmed: tag={} domain={} frames={} attempts={} reason={}",
        entry.asid,
        entry.domain,
        entry.frames.len(),
        entry.attempts,
        entry.reason
    );
    if entry.release_tag_slot {
        super::address_space::release_tag_slot_after_invalidation(entry.asid, entry.domain);
    }
    drop(entry.frames);
}

/// Count one attempt and, at the budget, give up on the tag: quarantine the frames
/// and keep refusing to reuse it.
fn note_attempt(asid: usize) {
    ATTEMPTS_TOTAL.fetch_add(1, Ordering::AcqRel);
    let exhausted = {
        let mut queue = DEFERRED.lock();
        match queue.iter().position(|entry| entry.asid == asid) {
            Some(index) => {
                queue[index].attempts += 1;
                if queue[index].attempts >= REAPER_MAX_ATTEMPTS {
                    Some(queue.remove(index))
                } else {
                    None
                }
            }
            None => None,
        }
    };
    let Some(entry) = exhausted else {
        return;
    };
    mark_abandoned(entry.asid);
    ABANDONED_FRAMES.fetch_add(entry.frames.len(), Ordering::AcqRel);
    log::error!(
        "[aspace] deferred release abandoned after {} attempts: tag={} domain={} frames={} — a remote hart never acknowledged the invalidation; the frames are quarantined and the tag stays unconfirmed",
        entry.attempts,
        entry.asid,
        entry.domain,
        entry.frames.len()
    );
    super::address_space::quarantine_frames(entry.frames, entry.reason);
}

fn tag_abandoned(asid: usize) -> bool {
    if ABANDONED_SATURATED.load(Ordering::Acquire) {
        return true;
    }
    ABANDONED_TAGS.lock().contains(&asid)
}

fn mark_abandoned(asid: usize) {
    let mut tags = ABANDONED_TAGS.lock();
    if tags.contains(&asid) {
        return;
    }
    if tags.len() >= MAX_ABANDONED_TAGS {
        // Cannot record the tag, so a later `tag_invalidation_unconfirmed` could
        // wrongly answer `false`. Fail closed: report every tag as unconfirmed
        // from here on, loudly.
        ABANDONED_SATURATED.store(true, Ordering::Release);
        log::error!(
            "[aspace] abandoned-tag table full at tag {}; every tag now reports as unconfirmed",
            asid
        );
        return;
    }
    tags.push(asid);
    ABANDONED_ENTRIES.fetch_add(1, Ordering::AcqRel);
}

/// Record stack backing that a fail-closed task teardown could not retain. Those
/// frames are never returned to the allocator either, so they are counted with
/// the quarantine even though no `OwnedFrame` can be handed over.
pub fn note_leaked_stack_frames(pages: usize) {
    LEAKED_STACK_FRAMES.fetch_add(pages, Ordering::AcqRel);
}

/// Test-hooks view of the queue: entries still waiting on an acknowledgement.
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(target_arch = "riscv64"),
    allow(dead_code) // reason: the fixture that asserts on it is RV64-only today
)]
pub(crate) fn deferred_release_depth() -> usize {
    DEFERRED.lock().len()
}

/// Test-hooks view of the queue: frames retained across all pending entries.
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(target_arch = "riscv64"),
    allow(dead_code) // reason: the fixture that asserts on it is RV64-only today
)]
pub(crate) fn deferred_release_frames() -> usize {
    DEFERRED.lock().iter().map(|entry| entry.frames.len()).sum()
}

/// Test-hooks view of the queue for one tag: frames it currently retains there.
///
/// A deferred release withholds its page-table frames from the allocator, and no
/// allocator-side count can see them. A fixture that accounts a deferred revoke
/// by its grant backing alone (the `grant-gate` frames property) therefore needs
/// this to tell a correct deferral from a leak.
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(target_arch = "riscv64"),
    allow(dead_code) // reason: the fixture that asserts on it is RV64-only today
)]
pub(crate) fn deferred_release_frames_for(asid: usize) -> usize {
    DEFERRED
        .lock()
        .iter()
        .filter(|entry| entry.asid == asid)
        .map(|entry| entry.frames.len())
        .sum()
}

/// Test-hooks view of the whole queue: `(tag, frames, attempts, reason)` per
/// pending entry, so a fixture that does not add up can name what is holding a
/// frame instead of inferring a retention owner.
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(target_arch = "riscv64"),
    allow(dead_code) // reason: the fixture that asserts on it is RV64-only today
)]
pub(crate) fn deferred_release_snapshot() -> Vec<(usize, usize, usize, &'static str)> {
    DEFERRED
        .lock()
        .iter()
        .map(|entry| (entry.asid, entry.frames.len(), entry.attempts, entry.reason))
        .collect()
}

/// Test-hooks view: reaper attempts spent since boot.
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(target_arch = "riscv64"),
    allow(dead_code) // reason: the fixture that asserts on it is RV64-only today
)]
pub(crate) fn deferred_release_attempts() -> usize {
    ATTEMPTS_TOTAL.load(Ordering::Acquire)
}

/// Test-hooks view: tags given up on, i.e. the loud leaks.
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(target_arch = "riscv64"),
    allow(dead_code) // reason: the fixture that asserts on it is RV64-only today
)]
pub(crate) fn deferred_release_abandoned() -> usize {
    ABANDONED_ENTRIES.load(Ordering::Acquire)
}

/// Test-hooks view: frames quarantined by a given-up deferred release.
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(target_arch = "riscv64"),
    allow(dead_code) // reason: the fixture that asserts on it is RV64-only today
)]
pub(crate) fn deferred_release_abandoned_frames() -> usize {
    ABANDONED_FRAMES.load(Ordering::Acquire)
}

/// Test-hooks view: stack frames a fail-closed teardown had to leak.
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(target_arch = "riscv64"),
    allow(dead_code) // reason: the fixture that asserts on it is RV64-only today
)]
pub(crate) fn leaked_stack_frames() -> usize {
    LEAKED_STACK_FRAMES.load(Ordering::Acquire)
}
