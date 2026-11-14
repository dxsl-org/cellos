//! One owner type for a run of physical frames, and the one release contract
//! every owner of such a run follows (architecture remediation Phase 16).
//!
//! A [`FrameRun`] is a page-aligned run of frames with an owner incarnation and
//! a state:
//!
//! ```text
//! Mapped ──unpublish──► Unpublished ──flush confirmed──► (normalise) ─► Free
//!   │                        └──────flush unconfirmed──► Quarantined(epochs) ─► reaper ─► Free
//!   └──long holder (pin, lease, DMA, VM)──► Quarantined(event) ──event──► Mapped
//! ```
//!
//! The release order (census §7.2): long holders are checked while the run is
//! still `Mapped`, every publication is removed, the invalidation is flushed
//! **once per operation** ([`Batch`]), short leases drain, the kernel alias is
//! normalised, and only then do the frames return to the allocator.
//!
//! A run whose invalidation is not confirmed is never freed and never panics:
//! it moves to `Quarantined` with the epochs each remote hart was asked for, and
//! the [`Pending`] list frees it once every one of those epochs is published
//! (`sched::smp::tlb_flush_completed`). Every remote hart serves an epoch with a
//! *full* local flush, so one acknowledgement covers shared-root and tag
//! invalidations alike. Nothing is ever abandoned: a hart that stays silent
//! keeps its frames quarantined, counted, and logged once on escalation.
//!
//! The quarantine is bounded by refusing new allocations past 1/64 of managed
//! memory ([`cap_exceeded`]): a degraded node, never a reused frame.

use crate::memory::frame::FlushCap;
use crate::memory::paging::PAGE_SIZE;
use crate::sched::smp::MAX_HARTS;
use crate::sync::Spinlock;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use types::{PhysAddr, VAddr};

/// One requested invalidation epoch per hart; 0 = that hart was not asked.
pub type Epochs = [usize; MAX_HARTS];

/// Who a run belongs to. A cell is named by its incarnation, not by a tid alone,
/// so a reused tid can never inherit a dead incarnation's frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunOwner {
    Cell {
        cell: u64,
        generation: u64,
        tid: usize,
    },
    Kernel,
    Vm {
        owner: usize,
        vm_id: u64,
    },
}

/// What the frames hold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunKind {
    /// Data: stack, segment, grant, SHM or guest memory.
    Leaf,
    /// A page-table frame detached from a root.
    PageTable,
    /// A hypervisor control structure (VMCB, VMCS, MSR/IO bitmaps, APIC page).
    VmStructure,
}

/// The kernel-root alias of the run, which decides normalisation before free.
///
/// RV64 and AArch64 keep every free frame identity-mapped kernel RWX (the loader
/// zeroes a fresh frame through `phys_to_virt`); x86_64 reaches RAM through the
/// HHDM only. The creator of a run picks the variant for its architecture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelAlias {
    /// Whatever alias the frame has is the one a free frame must have: nothing
    /// to restore.
    Intact,
    /// The identity leaf was removed: restore it as kernel RWX before free.
    Removed,
    /// The identity leaf is published USER: unpublish, then restore it.
    User,
}

/// Why a run is held back from the allocator.
///
/// The three invalidation reasons wait on per-hart epochs, which the pending
/// entry holding the run records once for all of its runs (an epoch set is
/// `MAX_HARTS` words: too large to copy into every run).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    /// A private root's tag invalidation was not acknowledged.
    TagUnconfirmed { asid: u16 },
    /// A shared-root invalidation was not acknowledged within the probe bound.
    SharedUnconfirmed,
    /// The SBI RFENCE call failed and the IPI epoch fallback was not
    /// acknowledged within the probe bound either.
    RfenceFailed,
    /// An in-flight asynchronous operation pins the frames (`memory::pin`).
    Pinned(crate::memory::pin::ReleaseKey),
    /// A DMA export window's IOTLB drain was not acknowledged.
    IotlbUnacked { export_id: u64 },
    /// A domain-grant record is still revoking.
    DomainRevoking,
    /// The VM that maps the frames may still run.
    VmLive,
}

/// Number of [`Reason`] variants, for the per-reason counters.
pub const REASONS: usize = 7;

impl Reason {
    /// Stable index into the per-reason counters.
    pub const fn index(&self) -> usize {
        match self {
            Reason::TagUnconfirmed { .. } => 0,
            Reason::SharedUnconfirmed => 1,
            Reason::RfenceFailed => 2,
            Reason::Pinned(_) => 3,
            Reason::IotlbUnacked { .. } => 4,
            Reason::DomainRevoking => 5,
            Reason::VmLive => 6,
        }
    }

    /// Log name.
    pub const fn name(&self) -> &'static str {
        match self {
            Reason::TagUnconfirmed { .. } => "tag-unconfirmed",
            Reason::SharedUnconfirmed => "shared-unconfirmed",
            Reason::RfenceFailed => "rfence-failed",
            Reason::Pinned(_) => "pinned",
            Reason::IotlbUnacked { .. } => "iotlb-unacked",
            Reason::DomainRevoking => "domain-revoking",
            Reason::VmLive => "vm-live",
        }
    }

    /// An invalidation reason, ended only by published epochs; the others are
    /// event keys, ended only by [`Pending::release_event`].
    pub const fn waits_for_epochs(&self) -> bool {
        matches!(
            self,
            Reason::TagUnconfirmed { .. } | Reason::SharedUnconfirmed | Reason::RfenceFailed
        )
    }

    /// The private tag a reissue must flush locally, if any.
    const fn asid(&self) -> Option<usize> {
        match self {
            Reason::TagUnconfirmed { asid } => Some(*asid as usize),
            _ => None,
        }
    }
}

/// Where a run is in its release protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunState {
    /// Some root (kernel, private, IOMMU, stage-2) may still translate to it.
    Mapped,
    /// Every publication is removed; the invalidation is not yet confirmed.
    Unpublished,
    /// Held back from the allocator; see [`Reason`].
    Quarantined(Reason),
    /// Returned to the allocator.
    Free,
}

/// Why a shared invalidation is not confirmed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnconfirmedCause {
    /// `hart` did not publish its epoch before the probe bound.
    Timeout { hart: usize },
    /// The SBI RFENCE call failed and the IPI epoch fallback did not confirm
    /// before the probe bound.
    RfenceFailed { hart: usize },
}

/// The result of one bounded invalidation.
// reason: returned by value once per flush; boxing the epochs would allocate
// on the teardown path, which must not depend on the heap.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlushOutcome {
    /// Every hart that could hold the translation has dropped it.
    Confirmed,
    /// At least one hart did not confirm; `epochs` are the requests whose
    /// completion proves it later.
    Unconfirmed {
        epochs: Epochs,
        cause: UnconfirmedCause,
    },
}

impl FlushOutcome {
    pub const fn is_confirmed(&self) -> bool {
        matches!(self, FlushOutcome::Confirmed)
    }

    /// Combine two flushes of one operation: confirmed only if both are, and
    /// otherwise waiting for every epoch either asked for.
    pub fn and(self, other: FlushOutcome) -> FlushOutcome {
        match (self, other) {
            (FlushOutcome::Confirmed, x) | (x, FlushOutcome::Confirmed) => x,
            (
                FlushOutcome::Unconfirmed { epochs: a, cause },
                FlushOutcome::Unconfirmed { epochs: b, .. },
            ) => FlushOutcome::Unconfirmed {
                epochs: max_epochs(&a, &b),
                cause,
            },
        }
    }

    /// The quarantine reason an unconfirmed flush gives its runs.
    pub const fn reason(&self) -> Option<Reason> {
        match self {
            FlushOutcome::Confirmed => None,
            FlushOutcome::Unconfirmed {
                cause: UnconfirmedCause::Timeout { .. },
                ..
            } => Some(Reason::SharedUnconfirmed),
            FlushOutcome::Unconfirmed {
                cause: UnconfirmedCause::RfenceFailed { .. },
                ..
            } => Some(Reason::RfenceFailed),
        }
    }
}

fn max_epochs(a: &Epochs, b: &Epochs) -> Epochs {
    let mut out = *a;
    for (slot, epoch) in out.iter_mut().zip(b.iter()) {
        *slot = (*slot).max(*epoch);
    }
    out
}

/// A virtual range that must stay reserved until the run's invalidation is
/// confirmed (census risk 2): a VA handed out again while a remote hart may
/// still translate it would resolve the new owner's VA to the old frame.
/// `release(start, end)` runs once, right after the frames are freed and under
/// the same frame-allocator hold, so it may take only leaf locks.
#[derive(Clone, Copy, Debug)]
pub struct VaHold {
    pub release: fn(usize, usize),
    pub start: usize,
    pub end: usize,
}

/// A transition the run's current state does not allow. Returned, never a
/// panic: the run keeps its state and its frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateError {
    pub state: RunState,
}

/// A run of physical frames with one owner and one release protocol.
///
/// Dropping a run that is not [`RunState::Free`] never frees it: the frames are
/// leaked (counted in [`Counters::leaked`]) because nothing proves them safe.
#[derive(Debug)]
pub struct FrameRun {
    base: PhysAddr,
    pages: u32,
    owner: RunOwner,
    kind: RunKind,
    alias: KernelAlias,
    state: RunState,
    va_hold: Option<VaHold>,
}

impl FrameRun {
    /// A run some root still maps.
    pub fn mapped(
        base: PhysAddr,
        pages: usize,
        owner: RunOwner,
        kind: RunKind,
        alias: KernelAlias,
    ) -> Self {
        Self::with_state(base, pages, owner, kind, alias, RunState::Mapped)
    }

    /// A run whose publications are already removed (or never existed); its
    /// invalidation is still to be confirmed.
    pub fn unpublished(
        base: PhysAddr,
        pages: usize,
        owner: RunOwner,
        kind: RunKind,
        alias: KernelAlias,
    ) -> Self {
        Self::with_state(base, pages, owner, kind, alias, RunState::Unpublished)
    }

    fn with_state(
        base: PhysAddr,
        pages: usize,
        owner: RunOwner,
        kind: RunKind,
        alias: KernelAlias,
        state: RunState,
    ) -> Self {
        debug_assert!(base.is_multiple_of(PAGE_SIZE));
        Self {
            base,
            // A run longer than 2^32 pages (16 TiB) cannot be allocated.
            pages: u32::try_from(pages).unwrap_or(u32::MAX),
            owner,
            kind,
            alias,
            state,
            va_hold: None,
        }
    }

    /// Keep `hold` reserved until the frames are freed.
    pub fn with_va_hold(mut self, hold: VaHold) -> Self {
        self.va_hold = Some(hold);
        self
    }

    #[inline]
    pub const fn base(&self) -> PhysAddr {
        self.base
    }

    #[inline]
    pub const fn pages(&self) -> usize {
        self.pages as usize
    }

    pub const fn owner(&self) -> RunOwner {
        self.owner
    }

    pub const fn kind(&self) -> RunKind {
        self.kind
    }

    pub const fn alias(&self) -> KernelAlias {
        self.alias
    }

    pub const fn state(&self) -> RunState {
        self.state
    }

    /// `Mapped → Unpublished`: the caller removed every publication. A `User`
    /// alias is now `Removed`.
    pub fn unpublish(&mut self) -> Result<(), StateError> {
        if self.state != RunState::Mapped {
            return Err(StateError { state: self.state });
        }
        if self.alias == KernelAlias::User {
            self.alias = KernelAlias::Removed;
        }
        self.state = RunState::Unpublished;
        Ok(())
    }

    /// `Mapped → Quarantined(reason)` for a long holder (pin, lease, DMA export,
    /// revoking domain record, live VM): its PTEs stay, because the holder still
    /// dereferences them. Invalidation reasons are refused: they arise only from
    /// an unconfirmed flush of an unpublished run.
    pub fn hold(&mut self, reason: Reason) -> Result<(), StateError> {
        if self.state != RunState::Mapped || reason.waits_for_epochs() {
            return Err(StateError { state: self.state });
        }
        self.state = RunState::Quarantined(reason);
        Ok(())
    }

    /// `Quarantined(event) → Mapped` once the event the hold waited on happened;
    /// the run then takes the normal release path.
    // reason: the pin, DMA-export and domain-grant owners adopt event holds in
    // their own Phase 16 migrations; host tests cover the transition today.
    #[cfg_attr(not(test), allow(dead_code))]
    fn end_hold(&mut self) -> Result<(), StateError> {
        match self.state {
            RunState::Quarantined(reason) if !reason.waits_for_epochs() => {
                self.state = RunState::Mapped;
                Ok(())
            }
            state => Err(StateError { state }),
        }
    }

    /// `Unpublished → Quarantined(reason)` after an unconfirmed flush.
    fn quarantine(&mut self, reason: Reason) -> Result<(), StateError> {
        if self.state != RunState::Unpublished || !reason.waits_for_epochs() {
            return Err(StateError { state: self.state });
        }
        self.state = RunState::Quarantined(reason);
        Ok(())
    }

    /// `Unpublished | Quarantined(epochs) → Free`: normalise, free, release the
    /// VA hold. Only callers that proved the invalidation reach this.
    fn free(&mut self, backend: &mut impl Backend) -> Result<(), StateError> {
        match self.state {
            RunState::Unpublished => {}
            RunState::Quarantined(reason) if reason.waits_for_epochs() => {}
            state => return Err(StateError { state }),
        }
        if self.alias != KernelAlias::Intact {
            backend.normalise(self);
        }
        backend.free(self.base, self.pages(), self.va_hold.take());
        self.state = RunState::Free;
        Ok(())
    }
}

impl Drop for FrameRun {
    fn drop(&mut self) {
        if self.state != RunState::Free {
            PENDING
                .counters
                .leaked
                .fetch_add(self.pages(), Ordering::AcqRel);
            log::error!(
                "[frame-run] run {:#x}+{} dropped in state {:?}: frames leaked, never reused",
                self.base,
                self.pages,
                self.state
            );
        }
    }
}

/// The machinery a release needs, separated so host tests drive the same state
/// machine with a fake acknowledgement source and allocator.
pub(crate) trait Backend {
    /// Has `hart` published `epoch` (0 = never asked)?
    fn completed(&self, hart: usize, epoch: usize) -> bool;
    /// Ask every online remote hart again (flushing `asid` locally first, if
    /// any) and return the epochs requested.
    fn reissue(&mut self, asid: Option<usize>) -> Epochs;
    /// Restore the frames' kernel alias before free (`KernelAlias` ≠ `Intact`).
    fn normalise(&mut self, run: &FrameRun);
    /// Return `pages` frames at `base` to the allocator, then release `hold`
    /// in the same allocator hold: no allocation can take the frames back
    /// between the two, so a new owner's reservation of the same range is
    /// never the one released.
    fn free(&mut self, base: PhysAddr, pages: usize, hold: Option<VaHold>);
}

fn epochs_confirmed(backend: &impl Backend, epochs: &Epochs) -> bool {
    epochs
        .iter()
        .enumerate()
        .all(|(hart, &epoch)| backend.completed(hart, epoch))
}

/// Finish an unpublished run after its operation's flush: free it when
/// `outcome` is confirmed, otherwise quarantine it on `pending`.
///
/// Returns the pages freed. A run that is not `Unpublished` is refused and kept
/// as it is (handed back in `Err`), never freed.
pub(crate) fn settle(
    mut run: FrameRun,
    outcome: FlushOutcome,
    backend: &mut impl Backend,
    pending: &Pending,
) -> Result<usize, FrameRun> {
    if run.state != RunState::Unpublished {
        return Err(run);
    }
    match outcome {
        FlushOutcome::Confirmed => {
            let pages = run.pages();
            match run.free(backend) {
                Ok(()) => Ok(pages),
                Err(_) => Err(run),
            }
        }
        FlushOutcome::Unconfirmed { epochs, .. } => {
            let reason = outcome.reason().unwrap_or(Reason::SharedUnconfirmed);
            match run.quarantine(reason) {
                Ok(()) => {
                    pending.push(run, &epochs);
                    Ok(0)
                }
                Err(_) => Err(run),
            }
        }
    }
}

/// Counters for logs and fixtures; every page count is in 4 KiB frames.
pub struct Counters {
    /// Pages currently quarantined, every reason.
    quarantined: AtomicUsize,
    /// Pages currently quarantined, per [`Reason::index`].
    by_reason: [AtomicUsize; REASONS],
    /// Pages the reaper freed after a later epoch confirmed them.
    released_after_retry: AtomicUsize,
    /// Allocations refused by the 1/64 cap.
    refused: AtomicUsize,
    /// Pages that could be neither freed nor recorded (dropped in a non-free
    /// state, or no heap for a pending entry): never reused.
    leaked: AtomicUsize,
}

impl Counters {
    const fn new() -> Self {
        Self {
            quarantined: AtomicUsize::new(0),
            by_reason: [const { AtomicUsize::new(0) }; REASONS],
            released_after_retry: AtomicUsize::new(0),
            refused: AtomicUsize::new(0),
            leaked: AtomicUsize::new(0),
        }
    }

    pub fn quarantined(&self) -> usize {
        self.quarantined.load(Ordering::Acquire)
    }

    pub fn quarantined_for(&self, reason_index: usize) -> usize {
        self.by_reason
            .get(reason_index)
            .map_or(0, |count| count.load(Ordering::Acquire))
    }

    pub fn released_after_retry(&self) -> usize {
        self.released_after_retry.load(Ordering::Acquire)
    }

    pub fn refused(&self) -> usize {
        self.refused.load(Ordering::Acquire)
    }

    pub fn leaked(&self) -> usize {
        self.leaked.load(Ordering::Acquire)
    }

    fn add(&self, reason: &Reason, pages: usize) {
        self.quarantined.fetch_add(pages, Ordering::AcqRel);
        self.by_reason[reason.index()].fetch_add(pages, Ordering::AcqRel);
    }

    fn sub(&self, reason: &Reason, pages: usize) {
        self.quarantined.fetch_sub(pages, Ordering::AcqRel);
        self.by_reason[reason.index()].fetch_sub(pages, Ordering::AcqRel);
    }
}

/// Reaper attempts between two reissues of an entry's invalidation request. A
/// hart that took the IPI but lost the request answers only when asked again;
/// every other attempt is a pure check.
pub(crate) const REISSUE_EVERY: usize = 32;

/// Reaper attempts (10 ms each on hart 0's timer) after which a still-pending
/// entry is logged once as an error. The entry keeps waiting: nothing is ever
/// abandoned.
pub(crate) const ESCALATE_AFTER: usize = 512;

struct Entry {
    reason: Reason,
    /// The epochs an invalidation reason waits for (unused for event keys).
    epochs: Epochs,
    runs: Vec<FrameRun>,
    pages: usize,
    attempts: usize,
}

/// Quarantined runs, one entry per wait (one per private tag, one for shared
/// invalidations, one for RFENCE fallbacks, one per event key).
pub struct Pending {
    entries: Spinlock<Vec<Entry>>,
    counters: Counters,
}

/// The kernel's quarantine.
pub static PENDING: Pending = Pending::new(crate::sync::rank::FRAME_RUN_PENDING);

impl Pending {
    pub(crate) const fn new(rank: crate::sync::Rank) -> Self {
        Self {
            entries: Spinlock::ranked(Vec::new(), rank),
            counters: Counters::new(),
        }
    }

    pub fn counters(&self) -> &Counters {
        &self.counters
    }

    /// Pages currently quarantined.
    pub fn quarantined_pages(&self) -> usize {
        self.counters.quarantined()
    }

    /// Record a quarantined run; `epochs` are the requests an invalidation
    /// reason waits for (ignored for an event key).
    ///
    /// Runs that wait on the same thing share an entry (same tag, any shared
    /// flush, any RFENCE fallback, the same event key); merged epochs take the
    /// per-hart maximum, which only ever waits longer. Built with
    /// `try_reserve`: teardown must not panic on heap, so a run that cannot be
    /// recorded is leaked (counted), never freed.
    pub(crate) fn push(&self, run: FrameRun, epochs: &Epochs) {
        let RunState::Quarantined(reason) = run.state else {
            // Only quarantined runs belong here; anything else keeps its frames.
            self.leak(run, "not quarantined");
            return;
        };
        let pages = run.pages();
        let mut entries = self.entries.lock();
        let rejected = if let Some(entry) = entries.iter_mut().find(|e| e.reason == reason) {
            if entry.runs.try_reserve(1).is_ok() {
                entry.epochs = max_epochs(&entry.epochs, epochs);
                entry.runs.push(run);
                entry.pages += pages;
                self.counters.add(&reason, pages);
                None
            } else {
                Some(run)
            }
        } else {
            let mut runs = Vec::new();
            if entries.try_reserve(1).is_ok() && runs.try_reserve(1).is_ok() {
                runs.push(run);
                entries.push(Entry {
                    reason,
                    epochs: *epochs,
                    runs,
                    pages,
                    attempts: 0,
                });
                self.counters.add(&reason, pages);
                None
            } else {
                Some(run)
            }
        };
        drop(entries);
        if let Some(run) = rejected {
            self.leak(run, "no heap for the pending entry");
        }
    }

    pub(crate) fn leak(&self, run: FrameRun, why: &str) {
        self.counters
            .leaked
            .fetch_add(run.pages(), Ordering::AcqRel);
        log::error!(
            "[frame-run] run {:#x}+{} not recorded ({}): frames leaked, never reused",
            run.base,
            run.pages,
            why
        );
        core::mem::forget(run);
    }

    /// One reaper pass: step **every** entry once. An entry whose epochs are all
    /// published is removed and its runs normalised and freed outside the lock;
    /// the others count an attempt and are reissued every [`REISSUE_EVERY`].
    /// Event-keyed entries are left to [`Self::release_event`]. Never waits on a
    /// hart. Returns the pages freed.
    pub(crate) fn reap(&self, backend: &mut impl Backend) -> usize {
        let mut freed = 0;
        let mut index = 0;
        loop {
            let mut escalate = None;
            let ready = {
                let mut entries = self.entries.lock();
                let Some(entry) = entries.get_mut(index) else {
                    break;
                };
                if !entry.reason.waits_for_epochs() {
                    index += 1;
                    None
                } else if epochs_confirmed(backend, &entry.epochs) {
                    // Do not advance: the last entry moved into `index`.
                    Some(entries.swap_remove(index))
                } else {
                    entry.attempts += 1;
                    if entry.attempts.is_multiple_of(REISSUE_EVERY) {
                        let asked = backend.reissue(entry.reason.asid());
                        entry.epochs = max_epochs(&entry.epochs, &asked);
                    }
                    if entry.attempts == ESCALATE_AFTER {
                        escalate = Some((entry.reason, entry.pages, entry.attempts));
                    }
                    index += 1;
                    None
                }
            };
            if let Some((reason, pages, attempts)) = escalate {
                log::error!(
                    "[frame-run] {} pages still {} after {} attempts: a hart has not acknowledged; they stay quarantined",
                    pages,
                    reason.name(),
                    attempts
                );
            }
            if let Some(entry) = ready {
                freed += self.complete(entry, backend);
            }
        }
        freed
    }

    fn complete(&self, entry: Entry, backend: &mut impl Backend) -> usize {
        self.counters.sub(&entry.reason, entry.pages);
        let mut freed = 0;
        for mut run in entry.runs {
            let pages = run.pages();
            match run.free(backend) {
                Ok(()) => freed += pages,
                // A run of another state cannot be in an epoch entry; keep it.
                Err(_) => self.leak(run, "pending run in an unexpected state"),
            }
        }
        self.counters
            .released_after_retry
            .fetch_add(freed, Ordering::AcqRel);
        log::info!(
            "[frame-run] released {} quarantined pages ({}) after {} attempts",
            freed,
            entry.reason.name(),
            entry.attempts
        );
        freed
    }

    /// End every event hold `matches` selects: those runs return to `Mapped`
    /// and to the caller, who resumes their release. Invalidation entries are
    /// never released here — only a published epoch ends them.
    // reason: see `FrameRun::end_hold`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn release_event(&self, matches: impl Fn(&Reason) -> bool) -> Vec<FrameRun> {
        let mut released = Vec::new();
        let mut entries = self.entries.lock();
        let mut index = 0;
        while index < entries.len() {
            let entry = &entries[index];
            if !entry.reason.waits_for_epochs() && matches(&entry.reason) {
                let entry = entries.swap_remove(index);
                self.counters.sub(&entry.reason, entry.pages);
                for mut run in entry.runs {
                    // `end_hold` cannot fail for an event entry's run.
                    let _ = run.end_hold();
                    released.push(run);
                }
            } else {
                index += 1;
            }
        }
        released
    }

    /// Is a run quarantined under `asid`'s tag invalidation?
    pub fn tag_pending(&self, asid: usize) -> bool {
        self.entries
            .lock()
            .iter()
            .any(|entry| entry.reason.asid() == Some(asid))
    }

    /// Pending entries, for fixtures and logs.
    pub fn entries(&self) -> usize {
        self.entries.lock().len()
    }

    /// Note one allocation refused by the cap; logs on powers of two so a
    /// sustained refusal is visible without flooding the console.
    pub(crate) fn note_refused(&self, total_frames: usize) {
        let refused = self.counters.refused.fetch_add(1, Ordering::AcqRel) + 1;
        if refused.is_power_of_two() {
            log::error!(
                "[frame-run] allocation refused: {} pages quarantined exceed 1/64 of {} managed frames (refusals {})",
                self.counters.quarantined(),
                total_frames,
                refused
            );
        }
    }
}

/// One operation's shared-root invalidation and the runs it frees.
///
/// Owners record every changed translation ([`Self::invalidate`]) and hand over
/// every unpublished run ([`Self::retire`]); [`Self::finish`] then flushes
/// **once** and frees or quarantines the runs. A cell teardown is O(operations)
/// broadcasts, not O(pages). Dropping an unfinished batch finishes it (without
/// waiting when the cap is unavailable), so a forgotten finish can never free a
/// frame under a live translation.
#[must_use = "a batch flushes and frees on finish (or drop)"]
pub struct Batch {
    ranges: Vec<(VAddr, usize)>,
    all: bool,
    runs: Vec<FrameRun>,
    drain_user_copies: bool,
}

impl Batch {
    pub const fn new() -> Self {
        Self {
            ranges: Vec::new(),
            all: false,
            runs: Vec::new(),
            drain_user_copies: false,
        }
    }

    /// Nothing to flush and nothing to free.
    pub fn is_empty(&self) -> bool {
        !self.all && self.ranges.is_empty() && self.runs.is_empty()
    }

    /// The translation of the page holding `vaddr` changed in the shared root.
    pub fn invalidate(&mut self, vaddr: VAddr) {
        self.invalidate_range(vaddr & !(PAGE_SIZE - 1), PAGE_SIZE);
    }

    /// The translations of `[start, start + size)` changed in the shared root.
    /// Adjacent ranges merge; with no heap for a new range the batch widens to
    /// a full invalidation rather than drop one.
    pub fn invalidate_range(&mut self, start: VAddr, size: usize) {
        if size == 0 || self.all {
            return;
        }
        if let Some(last) = self.ranges.last_mut() {
            if last.0.checked_add(last.1) == Some(start) {
                last.1 += size;
                return;
            }
            if last.0 <= start && start.checked_add(size) <= last.0.checked_add(last.1) {
                return;
            }
        }
        if self.ranges.try_reserve(1).is_ok() {
            self.ranges.push((start, size));
        } else {
            self.all = true;
        }
    }

    /// Invalidate every translation (a page-table detach whose span is unknown).
    pub fn invalidate_all(&mut self) {
        self.all = true;
        self.ranges.clear();
    }

    /// Wait out open SAS user-copy windows after the flush and before any free
    /// (census §7.2 step 5): a copy that probed a page earlier must not land in
    /// its frame once it is reused.
    pub fn drain_user_copies(&mut self) {
        self.drain_user_copies = true;
    }

    /// Free `run` after the flush confirms; quarantine it otherwise. The run
    /// must be `Unpublished`. With no heap to hold it the run is leaked
    /// (counted), never freed.
    pub fn retire(&mut self, run: FrameRun) {
        if self.runs.try_reserve(1).is_ok() {
            self.runs.push(run);
        } else {
            PENDING.leak(run, "no heap for the teardown batch");
        }
    }

    /// Flush once, wait for the copy windows, then free every run or
    /// quarantine it. Returns the flush outcome; an unconfirmed one is logged.
    pub fn finish(mut self, cap: &mut FlushCap) -> FlushOutcome {
        let outcome = if self.all || !self.ranges.is_empty() {
            crate::memory::tlb_shootdown::flush_ranges(cap, &self.ranges, self.all)
        } else {
            FlushOutcome::Confirmed
        };
        self.settle(outcome);
        outcome
    }

    /// [`Self::finish`] with this hart's cap, or — when it holds the frame
    /// allocator and so may not wait — the local flush plus remote requests
    /// with no wait, quarantining the runs until the reaper confirms them.
    pub fn complete(self) -> FlushOutcome {
        match FlushCap::take() {
            Some(mut cap) => self.finish(&mut cap),
            None => self.complete_without_wait(),
        }
    }

    fn complete_without_wait(mut self) -> FlushOutcome {
        let outcome = if self.all || !self.ranges.is_empty() {
            crate::memory::tlb_shootdown::flush_ranges_without_wait(&self.ranges, self.all)
        } else {
            FlushOutcome::Confirmed
        };
        self.settle(outcome);
        outcome
    }

    fn settle(&mut self, outcome: FlushOutcome) {
        self.ranges.clear();
        self.all = false;
        if core::mem::take(&mut self.drain_user_copies) {
            #[cfg(all(feature = "native-domains", cellos_wide))]
            crate::task::user_copy::wait_sas_copies_quiescent();
        }
        let mut backend = KernelBackend;
        let mut quarantined = 0;
        for run in self.runs.drain(..) {
            let pages = run.pages();
            match settle(run, outcome, &mut backend, &PENDING) {
                Ok(0) if !outcome.is_confirmed() => quarantined += pages,
                Ok(_) => {}
                Err(run) => PENDING.leak(run, "batch run not unpublished"),
            }
        }
        if let FlushOutcome::Unconfirmed { cause, .. } = outcome {
            log_unconfirmed(cause, quarantined);
        }
    }
}

impl Default for Batch {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        if !self.is_empty() {
            let batch = core::mem::take(self);
            let _ = batch.complete();
        }
    }
}

/// Log an unconfirmed shared flush once per operation. The lane gates count
/// these lines; a fixture that holds a CPU on purpose brackets its own.
fn log_unconfirmed(cause: UnconfirmedCause, quarantined: usize) {
    let (hart, what) = match cause {
        UnconfirmedCause::Timeout { hart } => (hart, "silent through the probe"),
        UnconfirmedCause::RfenceFailed { hart } => (
            hart,
            "silent through the epoch fallback after an RFENCE error",
        ),
    };
    #[cfg(feature = "test-hooks")]
    if TEST_SILENT_CPU_FIXTURE.load(Ordering::Acquire) {
        log::warn!(
            "[frame-run] shared invalidation unconfirmed: hart {} {}; {} pages quarantined: held (fixture)",
            hart,
            what,
            quarantined
        );
        return;
    }
    log::error!(
        "[frame-run] shared invalidation unconfirmed: hart {} {}; {} pages quarantined until it acknowledges",
        hart,
        what,
        quarantined
    );
}

/// Test-hooks: a fixture holds a CPU silent on purpose; its unconfirmed
/// flushes name themselves so the lane can excuse exactly those.
#[cfg(feature = "test-hooks")]
static TEST_SILENT_CPU_FIXTURE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Arm or disarm [`TEST_SILENT_CPU_FIXTURE`].
#[cfg(feature = "test-hooks")]
// reason: the x86 silent-CPU fixture arms it from the next commit.
#[allow(dead_code)]
pub(crate) fn set_test_silent_cpu_fixture(armed: bool) {
    TEST_SILENT_CPU_FIXTURE.store(armed, Ordering::Release);
}

/// The kernel's [`Backend`]: per-hart flush epochs, the kernel root, the frame
/// allocator.
pub(crate) struct KernelBackend;

impl Backend for KernelBackend {
    fn completed(&self, hart: usize, epoch: usize) -> bool {
        crate::sched::smp::tlb_flush_completed(hart, epoch)
    }

    fn reissue(&mut self, asid: Option<usize>) -> Epochs {
        if let Some(asid) = asid {
            crate::memory::tlb_shootdown::flush_tag_local(asid);
        }
        crate::memory::tlb_shootdown::request_remote_flushes()
    }

    /// Restore the identity leaf as kernel RWX: every free frame is
    /// identity-mapped there on RV64/AArch64, because the loader zeroes a fresh
    /// frame through `phys_to_virt`. The run is unpublished and its
    /// invalidation confirmed, so each leaf is new and needs no shared flush;
    /// the local flush drops any invalid entry this hart may still cache.
    fn normalise(&mut self, run: &FrameRun) {
        use crate::memory::paging::Flags;
        let kernel_rwx = Flags::from_bits(
            Flags::VALID
                | Flags::READ
                | Flags::WRITE
                | Flags::EXECUTE
                | Flags::ACCESSED
                | Flags::DIRTY,
        );
        let (base, pages) = (run.base(), run.pages());
        let restored = crate::memory::frame::with(|allocator| {
            let mut batch = Batch::new();
            for page in 0..pages {
                let frame = base + page * PAGE_SIZE;
                let _ = crate::memory::paging::map_page(
                    allocator, &mut batch, frame, frame, kernel_rwx,
                );
            }
            batch
        });
        // No allocator, nothing restored (and no paging to flush yet).
        if let Some(batch) = restored {
            drop(batch);
            crate::memory::paging::tlb_flush_all();
        }
    }

    fn free(&mut self, base: PhysAddr, pages: usize, hold: Option<VaHold>) {
        let _ = crate::memory::frame::with(|allocator| {
            for page in 0..pages {
                allocator.deallocate_frame(base + page * PAGE_SIZE);
            }
            if let Some(hold) = hold {
                (hold.release)(hold.start, hold.end);
            }
        });
    }
}

/// One step of the hart-0 timer reaper: free every quarantined run whose
/// epochs are all published, reissue the stale ones. Never waits.
pub fn reap() {
    crate::arch::tlb::report_rfence_failures();
    let _ = PENDING.reap(&mut KernelBackend);
}

/// The quarantine cap: refuse new allocations once quarantined pages exceed
/// 1/64 of the frames the allocator manages.
pub const fn cap_exceeded(quarantined: usize, total_frames: usize) -> bool {
    quarantined.saturating_mul(64) > total_frames
}

#[cfg(test)]
#[path = "frame_run_tests.rs"]
mod tests;
