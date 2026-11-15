//! One owner type for a run of physical frames, and the one release contract
//! every owner of such a run follows (architecture remediation Phase 16).
//!
//! A [`FrameRun`] is a page-aligned run of frames with a charge (its owner
//! incarnation and the quota account its pages are billed to) and a state:
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
//! The quarantine of runs waiting on an invalidation is bounded by refusing new
//! allocations once it passes 1/64 of managed memory ([`cap_exceeded`]): a
//! degraded node, never a reused frame. Event holds (a pin, a lease, a DMA
//! export) are not counted, as before the cap existed.

use crate::memory::cell_quota::{self, Account, Charge, Reservation};
use crate::memory::frame::FlushCap;
use crate::memory::paging::PAGE_SIZE;
use crate::memory::tag_pool::{Invalidated, TagLease};
use crate::sched::smp::MAX_HARTS;
use crate::sync::Spinlock;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use types::{PhysAddr, VAddr};

/// One requested invalidation epoch per hart; 0 = that hart was not asked.
pub type Epochs = [usize; MAX_HARTS];

/// Who a run belongs to and what it is billed to. The owner is an incarnation,
/// never a tid, so a reused tid or CellId can never inherit a dead
/// incarnation's frames or its refund.
///
/// `account: Some(_)` — the run holds `pages × PAGE_SIZE` bytes of that
/// account, taken before its frames were allocated and refunded exactly once,
/// when they return to the allocator. `None` — the owner is recorded and
/// nothing is billed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RunCharge {
    pub(crate) owner: Charge,
    pub(crate) account: Option<Account>,
}

impl RunCharge {
    /// Kernel frames: never billed.
    pub(crate) const KERNEL: Self = Self::unbilled(Charge::KERNEL);

    /// `owner` recorded, nothing billed.
    pub(crate) const fn unbilled(owner: Charge) -> Self {
        Self {
            owner,
            account: None,
        }
    }

    /// The charge of a run of `pages` frames billed with `bill`. A reservation
    /// must cover exactly those frames; its bytes now belong to the run. The
    /// kernel's reservations count nothing, so its runs carry no account.
    pub(super) fn of(bill: RunBill, pages: usize) -> Self {
        match bill {
            RunBill::Unbilled(owner) => Self::unbilled(owner),
            RunBill::Reserved(reservation) => {
                let (owner, account, bytes) = reservation.into_parts();
                debug_assert_eq!(
                    bytes,
                    pages * PAGE_SIZE,
                    "a run is billed exactly its frames"
                );
                if owner == Charge::KERNEL {
                    return Self::KERNEL;
                }
                Self {
                    owner,
                    account: Some(account),
                }
            }
        }
    }

    /// Return a freed run's bytes to its account. Lock-free and allocation-free
    /// (it runs in the hart-0 reaper inside the timer interrupt); a no-op once
    /// the owner's slot holds another incarnation.
    pub(super) fn refund(self, pages: usize) {
        if let Some(account) = self.account {
            cell_quota::refund(self.owner, account, pages * PAGE_SIZE);
        }
    }
}

/// What a new run is charged with: a [`Reservation`] covering exactly its
/// frames, taken before they were allocated, or an owner billed nothing
/// (`Charge::KERNEL` for kernel frames).
#[derive(Debug)]
pub(crate) enum RunBill {
    Unbilled(Charge),
    Reserved(Reservation),
}

impl From<Charge> for RunBill {
    fn from(owner: Charge) -> Self {
        Self::Unbilled(owner)
    }
}

impl From<Reservation> for RunBill {
    fn from(reservation: Reservation) -> Self {
        Self::Reserved(reservation)
    }
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
/// RV64 and AArch64 keep every free frame identity-mapped kernel RW+NX
/// (`paging::FREE_FRAME_ALIAS`: the loader zeroes a fresh frame through
/// `phys_to_virt`); x86_64 reaches RAM through the HHDM only. The creator of a
/// run picks the variant for its architecture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelAlias {
    /// Whatever alias the frame has is the one a free frame must have: nothing
    /// to restore.
    Intact,
    /// The identity leaf was removed: restore it as kernel RW+NX before free.
    Removed,
    /// The identity leaf is published USER: unpublish, then restore it.
    User,
}

impl KernelAlias {
    /// The alias of frames whose identity VA (VA == PA) another mapping took
    /// over — a stack, a grant, an SHM page. Where the kernel reaches RAM
    /// through that identity VA (RV64, AArch64), the run must restore it
    /// before free: `Removed`. Where it reaches RAM through the HHDM (x86_64),
    /// the identity VA was never the kernel's alias: `Intact`, and no
    /// lower-half kernel alias is ever added (census F3).
    pub fn of_identity_remap(base: PhysAddr) -> Self {
        if crate::memory::frame::phys_to_virt(base) == base {
            KernelAlias::Removed
        } else {
            KernelAlias::Intact
        }
    }
}

/// Why a run is held back from the allocator.
///
/// The invalidation reasons (and [`Reason::AliasUnrestored`]) wait on per-hart
/// epochs, which the pending entry holding the run records once for all of its
/// runs (an epoch set is `MAX_HARTS` words: too large to copy into every run).
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
    /// The invalidation is confirmed, but the kernel alias the free frame
    /// needs could not be restored (no page-table frame); the reaper retries.
    AliasUnrestored,
}

/// Number of [`Reason`] variants, for the per-reason counters.
pub const REASONS: usize = 8;

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
            Reason::AliasUnrestored => 7,
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
            Reason::AliasUnrestored => "alias-unrestored",
        }
    }

    /// A reason the reaper ends: invalidations by their published epochs, an
    /// unrestored alias by its next successful restore. The others are event
    /// keys, ended only by [`Pending::release_event`].
    pub const fn waits_for_epochs(&self) -> bool {
        matches!(
            self,
            Reason::TagUnconfirmed { .. }
                | Reason::SharedUnconfirmed
                | Reason::RfenceFailed
                | Reason::AliasUnrestored
        )
    }

    /// A reason only an acknowledged invalidation ends (a private tag, the
    /// shared root, an RFENCE fallback): the silent-CPU case the 1/64 cap
    /// bounds. Event holds and an unrestored alias are not counted.
    pub const fn awaits_invalidation(&self) -> bool {
        matches!(
            self,
            Reason::TagUnconfirmed { .. } | Reason::SharedUnconfirmed | Reason::RfenceFailed
        )
    }

    /// The private tag a reissue must flush locally, if any.
    pub(crate) const fn asid(&self) -> Option<usize> {
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
    /// Nobody waited: the flushing hart holds the frame allocator, so the
    /// requests went out (to `hart`, if any remote was asked) and the reaper
    /// confirms them later.
    NotAwaited { hart: Option<usize> },
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
                cause: UnconfirmedCause::Timeout { .. } | UnconfirmedCause::NotAwaited { .. },
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

/// A run of physical frames with one charge and one release protocol.
///
/// Dropping a run that is not [`RunState::Free`] never frees it: the frames are
/// leaked (counted in [`Counters::leaked`]) because nothing proves them safe.
#[derive(Debug)]
pub struct FrameRun {
    base: PhysAddr,
    pages: u32,
    charge: RunCharge,
    kind: RunKind,
    alias: KernelAlias,
    state: RunState,
    va_hold: Option<VaHold>,
    /// p01 provenance of a frame retained before it became a run; retired
    /// when the run is freed.
    #[cfg(feature = "p01-trace")]
    p01_retained: Option<crate::p01_retained::Token>,
}

impl FrameRun {
    /// A run some root still maps, billed with `bill`.
    pub(crate) fn mapped(
        base: PhysAddr,
        pages: usize,
        bill: impl Into<RunBill>,
        kind: RunKind,
        alias: KernelAlias,
    ) -> Self {
        let charge = RunCharge::of(bill.into(), pages);
        Self::with_state(base, pages, charge, kind, alias, RunState::Mapped)
    }

    /// A run whose publications are already removed (or never existed); its
    /// invalidation is still to be confirmed. Billed with `bill`.
    pub(crate) fn unpublished(
        base: PhysAddr,
        pages: usize,
        bill: impl Into<RunBill>,
        kind: RunKind,
        alias: KernelAlias,
    ) -> Self {
        let charge = RunCharge::of(bill.into(), pages);
        Self::unpublished_carrying(base, pages, charge, kind, alias)
    }

    /// [`Self::unpublished`] for frames that already carry `charge` (an
    /// [`OwnedFrame`](super::frame::OwnedFrame) becoming a run): the bytes it
    /// holds move into the run, nothing is taken again.
    pub(super) fn unpublished_carrying(
        base: PhysAddr,
        pages: usize,
        charge: RunCharge,
        kind: RunKind,
        alias: KernelAlias,
    ) -> Self {
        Self::with_state(base, pages, charge, kind, alias, RunState::Unpublished)
    }

    fn with_state(
        base: PhysAddr,
        pages: usize,
        charge: RunCharge,
        kind: RunKind,
        alias: KernelAlias,
        state: RunState,
    ) -> Self {
        debug_assert!(base.is_multiple_of(PAGE_SIZE));
        Self {
            base,
            // A run longer than 2^32 pages (16 TiB) cannot be allocated.
            pages: u32::try_from(pages).unwrap_or(u32::MAX),
            charge,
            kind,
            alias,
            state,
            va_hold: None,
            #[cfg(feature = "p01-trace")]
            p01_retained: None,
        }
    }

    /// Carry `token`'s p01 provenance until the run is freed.
    #[cfg(feature = "p01-trace")]
    pub(crate) fn with_p01_retained(mut self, token: Option<crate::p01_retained::Token>) -> Self {
        self.p01_retained = token;
        self
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

    pub(crate) const fn charge(&self) -> RunCharge {
        self.charge
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
    /// VA hold, refund the charge. Only callers that proved the invalidation
    /// reach this, whichever context they run in (the owner's syscall,
    /// retirement, the hart-0 reaper in the timer interrupt). A run whose alias
    /// cannot be restored is not freed: it keeps its state (still freeable) and
    /// the `Err` carries it, for the caller to quarantine as
    /// [`Reason::AliasUnrestored`].
    fn free(&mut self, backend: &mut impl Backend) -> Result<(), StateError> {
        if !self.freeable() {
            return Err(StateError { state: self.state });
        }
        if self.alias != KernelAlias::Intact && !backend.normalise(self) {
            return Err(StateError { state: self.state });
        }
        backend.free(self.base, self.pages(), self.va_hold.take());
        self.state = RunState::Free;
        // Once per run: `Free` is not freeable, so this is never reached again.
        self.charge.refund(self.pages());
        #[cfg(feature = "p01-trace")]
        crate::p01_retained::retire(self.p01_retained.take());
        Ok(())
    }

    /// May [`Self::free`] run (unpublished, or quarantined on a reason the
    /// reaper ends)?
    fn freeable(&self) -> bool {
        match self.state {
            RunState::Unpublished => true,
            RunState::Quarantined(reason) => reason.waits_for_epochs(),
            RunState::Mapped | RunState::Free => false,
        }
    }
}

/// A run dropped outside `Free` keeps its frames and its charge: neither is
/// returned while nothing proves the frames unreachable. The owner's
/// deregistration at retirement resets the account.
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

/// A run no shared root maps: the kernel reaches it only through its own
/// alias of RAM (`Intact`), and hardware reaches it only through structures
/// its owner tears down itself. Hypervisor control structures (VMCB, VMCS,
/// MSR/IO bitmaps, the xAPIC page), nested and Stage-2 tables, and a VM's guest
/// RAM are such runs (`RunKind::VmStructure`, `PageTable`, `Leaf`).
///
/// Its release has nothing to unpublish and nothing to flush in the shared
/// root, so dropping it frees the frames (through a [`Batch`], which
/// quarantines them for the reaper on a hart that may not take the
/// allocator). The owner drops it only once nothing else can reach the frames:
/// the VM left its registry, so no vCPU can enter it; its VMCS was cleared;
/// its nested or Stage-2 translations were invalidated. An error path that
/// drops a structure no VM ever used frees it the same way, so a partial
/// construction never leaks (census §5).
#[derive(Debug)]
pub struct PrivateRun(Option<FrameRun>);

impl PrivateRun {
    /// `pages` fresh contiguous frames (not zeroed) billed with `bill`, or
    /// `None` when the allocator has no such run or the quarantine cap refuses
    /// it (a reservation in `bill` is then refunded).
    // reason: only the x86 VM control structures allocate a private run whole.
    #[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
    pub(crate) fn allocate(pages: usize, bill: impl Into<RunBill>, kind: RunKind) -> Option<Self> {
        let base = crate::memory::frame::allocate_contiguous(pages)?;
        Some(Self::adopt(base, pages, bill, kind))
    }

    /// Own `pages` frames at `base` the caller has just allocated, billed with
    /// `bill`.
    pub(crate) fn adopt(
        base: PhysAddr,
        pages: usize,
        bill: impl Into<RunBill>,
        kind: RunKind,
    ) -> Self {
        Self(Some(FrameRun::mapped(
            base,
            pages,
            bill,
            kind,
            KernelAlias::Intact,
        )))
    }

    #[inline]
    pub fn base(&self) -> PhysAddr {
        self.0.as_ref().map_or(0, FrameRun::base)
    }

    /// Never free the run: hardware may still reach it (its owner's release
    /// of it failed). Counted and logged as leaked; the owner drops what is
    /// left as an empty run.
    pub fn leak(&mut self, why: &str) {
        if let Some(run) = self.0.take() {
            PENDING.leak(run, why);
        }
    }

    /// Hand the run to `batch`, which frees it with the operation's other
    /// runs once the batch finishes.
    pub fn retire_into(mut self, batch: &mut Batch) {
        if let Some(run) = self.0.take() {
            retire_private(run, batch);
        }
    }
}

impl Drop for PrivateRun {
    fn drop(&mut self) {
        if let Some(run) = self.0.take() {
            let mut batch = Batch::new();
            retire_private(run, &mut batch);
            let _ = batch.complete();
        }
    }
}

/// `Mapped → Unpublished` with nothing to remove, then into `batch`.
fn retire_private(mut run: FrameRun, batch: &mut Batch) {
    // A `PrivateRun` holds only `Mapped` runs; anything else is leaked by
    // the batch (counted), never freed.
    let _ = run.unpublish();
    batch.retire(run);
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
    /// `false`: not restored (no page-table frame); the run must not be freed.
    fn normalise(&mut self, run: &FrameRun) -> bool;
    /// Return `pages` frames at `base` to the allocator, then release `hold`
    /// in the same allocator hold: no allocation can take the frames back
    /// between the two, so a new owner's reservation of the same range is
    /// never the one released.
    fn free(&mut self, base: PhysAddr, pages: usize, hold: Option<VaHold>);
    /// Test-hooks seam: is every private tag's acknowledgement withheld? A
    /// tag entry then stays unconfirmed whatever its epochs say.
    fn tag_ack_withheld(&self) -> bool {
        false
    }
    /// Private tag `asid`'s wait has ended (its slot, if the entry held one,
    /// is already back in its pool): work that skipped the tag while it was
    /// unconfirmed may run again.
    fn tag_wait_ended(&mut self, _asid: usize) {}
}

fn epochs_confirmed(backend: &impl Backend, epochs: &Epochs) -> bool {
    epochs
        .iter()
        .enumerate()
        .all(|(hart, &epoch)| backend.completed(hart, epoch))
}

/// Has `entry`'s wait ended? A private tag's entry also waits out the
/// test-hooks withheld-acknowledgement window.
fn entry_confirmed(backend: &impl Backend, entry: &Entry) -> bool {
    !(entry.reason.asid().is_some() && backend.tag_ack_withheld())
        && epochs_confirmed(backend, &entry.epochs)
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
                // The alias could not be restored: the reaper retries it.
                Err(_) if run.freeable() => match run.quarantine(Reason::AliasUnrestored) {
                    Ok(()) => {
                        pending.push(run, &[0; MAX_HARTS]);
                        Ok(0)
                    }
                    Err(_) => Err(run),
                },
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

/// The epochs a run behind private tag `asid` waits for: the tag flushed again
/// locally and every online remote asked again — each serves its epoch with a
/// full flush, which drops that tag's entries too — merged with what the
/// operation's own shared flush is still waiting on.
pub(crate) fn tag_epochs(asid: usize, outcome: FlushOutcome, backend: &mut impl Backend) -> Epochs {
    let asked = backend.reissue(Some(asid));
    match outcome {
        FlushOutcome::Confirmed => asked,
        FlushOutcome::Unconfirmed { epochs, .. } => max_epochs(&asked, &epochs),
    }
}

/// Quarantine an unpublished run behind private tag `asid`, whose bounded
/// probe did not confirm, until `epochs` are published (see [`tag_epochs`]).
/// The run is never freed here, whatever the shared flush said: a remote hart
/// may still resolve the private root's stale entry. A tag no reason can carry
/// (wider than 16 bits) waits as a shared invalidation, on the same epochs.
/// A run that is not `Unpublished` is refused and handed back, never freed.
pub(crate) fn quarantine_behind_tag(
    mut run: FrameRun,
    asid: usize,
    epochs: &Epochs,
    pending: &Pending,
) -> Result<(), FrameRun> {
    let reason = u16::try_from(asid).map_or(Reason::SharedUnconfirmed, |asid| {
        Reason::TagUnconfirmed { asid }
    });
    match run.quarantine(reason) {
        Ok(()) => {
            pending.push(run, epochs);
            Ok(())
        }
        Err(_) => Err(run),
    }
}

/// Counters for logs and fixtures; every page count is in 4 KiB frames.
pub struct Counters {
    /// Pages currently quarantined, every reason.
    quarantined: AtomicUsize,
    /// Pages currently quarantined, per [`Reason::index`].
    by_reason: [AtomicUsize; REASONS],
    /// Pages currently quarantined behind an invalidation
    /// ([`Reason::awaits_invalidation`]): the 1/64 cap's input.
    awaiting_invalidation: AtomicUsize,
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
            awaiting_invalidation: AtomicUsize::new(0),
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

    /// Pages quarantined behind an invalidation: what the 1/64 cap counts.
    pub fn awaiting_invalidation(&self) -> usize {
        self.awaiting_invalidation.load(Ordering::Acquire)
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
        if reason.awaits_invalidation() {
            self.awaiting_invalidation
                .fetch_add(pages, Ordering::AcqRel);
        }
    }

    fn sub(&self, reason: &Reason, pages: usize) {
        self.quarantined.fetch_sub(pages, Ordering::AcqRel);
        self.by_reason[reason.index()].fetch_sub(pages, Ordering::AcqRel);
        if reason.awaits_invalidation() {
            self.awaiting_invalidation
                .fetch_sub(pages, Ordering::AcqRel);
        }
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
    /// The tag lease of a private root whose teardown could not confirm this
    /// entry's tag: released, with the entry's proof, only once it completes.
    tag_slot: Option<TagLease>,
}

/// Where [`Pending::defer_tag`] recorded a private tag's wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TagWait {
    /// A new entry for the tag.
    Queued,
    /// Merged into the tag's entry, which now holds `pages`.
    Extended { pages: usize },
    /// Not recorded (no heap): the runs are leaked and every tag reads
    /// unconfirmed from now on.
    Lost,
}

/// Quarantined runs, one entry per wait (one per private tag, one for shared
/// invalidations, one for RFENCE fallbacks, one per event key).
pub struct Pending {
    entries: Spinlock<Vec<Entry>>,
    counters: Counters,
    /// Set once a private tag's wait could not be recorded (no heap): from
    /// then on every tag reads unconfirmed, because a missing entry would read
    /// as a confirmed one.
    tags_saturated: core::sync::atomic::AtomicBool,
}

/// The kernel's quarantine.
pub static PENDING: Pending = Pending::new(crate::sync::rank::FRAME_RUN_PENDING);

impl Pending {
    pub(crate) const fn new(rank: crate::sync::Rank) -> Self {
        Self {
            entries: Spinlock::ranked(Vec::new(), rank),
            counters: Counters::new(),
            tags_saturated: core::sync::atomic::AtomicBool::new(false),
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
        if let Err((run, why)) = self.try_push(run, epochs) {
            if matches!(run.state, RunState::Quarantined(reason) if reason.asid().is_some()) {
                self.saturate_tags(why);
            }
            self.leak(run, why);
        }
    }

    /// [`Self::push`] for a caller that holds a lock the console ranks below
    /// (the pin registry): a run that cannot be recorded is handed back, for
    /// the caller to [`Self::forget_unlogged`] and log once its locks are
    /// released. Never frees it.
    pub(crate) fn try_push(
        &self,
        run: FrameRun,
        epochs: &Epochs,
    ) -> Result<(), (FrameRun, &'static str)> {
        let RunState::Quarantined(reason) = run.state else {
            // Only quarantined runs belong here; anything else keeps its frames.
            return Err((run, "not quarantined"));
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
                    tag_slot: None,
                });
                self.counters.add(&reason, pages);
                None
            } else {
                Some(run)
            }
        };
        drop(entries);
        match rejected {
            Some(run) => Err((run, "no heap for the pending entry")),
            None => Ok(()),
        }
    }

    /// Wait for private tag `asid` until `epochs` are published: quarantine
    /// `runs` (unpublished: the private root no longer maps them) behind it
    /// and, with `tag_slot` (the lease of tag `asid`), keep that root's tag
    /// reserved until the same entry completes. An entry is recorded even with
    /// no run: it is what makes the tag read unconfirmed
    /// ([`Self::tag_unconfirmed`]).
    ///
    /// Nothing waits here and nothing is freed. A wait that cannot be recorded
    /// (no heap) leaks its runs (counted), drops the lease (so its tag stays
    /// reserved for the boot), and makes every tag read unconfirmed from then
    /// on: fail closed.
    pub(crate) fn defer_tag(
        &self,
        asid: usize,
        runs: Vec<FrameRun>,
        tag_slot: Option<TagLease>,
        epochs: &Epochs,
    ) -> TagWait {
        debug_assert!(tag_slot
            .as_ref()
            .is_none_or(|lease| lease.value() as usize == asid));
        let Ok(tag) = u16::try_from(asid) else {
            self.lose_tag_wait(runs, "tag wider than a reason carries");
            return TagWait::Lost;
        };
        let reason = Reason::TagUnconfirmed { asid: tag };
        let mut quarantined = Vec::new();
        if quarantined.try_reserve_exact(runs.len()).is_err() {
            self.lose_tag_wait(runs, "no heap for the tag wait");
            return TagWait::Lost;
        }
        for mut run in runs {
            match run.quarantine(reason) {
                Ok(()) => quarantined.push(run),
                Err(_) => self.leak(run, "tag run not unpublished"),
            }
        }
        let pages: usize = quarantined.iter().map(FrameRun::pages).sum();
        let mut entries = self.entries.lock();
        let placed = match entries.iter().position(|entry| entry.reason == reason) {
            Some(index) => {
                let entry = &mut entries[index];
                if entry.runs.try_reserve(quarantined.len()).is_ok() {
                    entry.epochs = max_epochs(&entry.epochs, epochs);
                    entry.runs.append(&mut quarantined);
                    entry.pages += pages;
                    // One lease per tag value, so an entry never holds two;
                    // a second would be dropped, which keeps it reserved.
                    entry.tag_slot = entry.tag_slot.take().or(tag_slot);
                    Ok(TagWait::Extended { pages: entry.pages })
                } else {
                    Err(quarantined)
                }
            }
            None if entries.try_reserve(1).is_ok() => {
                entries.push(Entry {
                    reason,
                    epochs: *epochs,
                    runs: quarantined,
                    pages,
                    attempts: 0,
                    tag_slot,
                });
                Ok(TagWait::Queued)
            }
            None => Err(quarantined),
        };
        if placed.is_ok() {
            self.counters.add(&reason, pages);
        }
        drop(entries);
        placed.unwrap_or_else(|runs| {
            self.lose_tag_wait(runs, "no heap for the tag wait");
            TagWait::Lost
        })
    }

    /// Fail closed for a tag wait that could not be recorded: its runs are
    /// leaked (counted, never freed) and every tag reads unconfirmed from now
    /// on.
    fn lose_tag_wait(&self, runs: Vec<FrameRun>, why: &str) {
        self.saturate_tags(why);
        for run in runs {
            self.leak(run, why);
        }
    }

    /// [`Self::lose_tag_wait`] for `pages` frames that could not even become
    /// runs (no heap): the caller already forgot them.
    pub(crate) fn lose_frames(&self, pages: usize, why: &str) {
        self.counters.leaked.fetch_add(pages, Ordering::AcqRel);
        log::error!(
            "[frame-run] {} frames not recorded ({}): frames leaked, never reused",
            pages,
            why
        );
        self.saturate_tags(why);
    }

    fn saturate_tags(&self, why: &str) {
        if !self.tags_saturated.swap(true, Ordering::AcqRel) {
            log::error!(
                "[frame-run] tag wait not recorded ({}): every private tag now reads unconfirmed",
                why
            );
        }
    }

    /// Is private tag `asid`'s invalidation still unconfirmed? True while a
    /// tag entry waits on epochs not all published (or inside the test-hooks
    /// withheld window), and for every tag once a tag wait was lost.
    pub(crate) fn tag_unconfirmed(&self, asid: usize, backend: &impl Backend) -> bool {
        self.tags_saturated.load(Ordering::Acquire)
            || self
                .entries
                .lock()
                .iter()
                .any(|entry| entry.reason.asid() == Some(asid) && !entry_confirmed(backend, entry))
    }

    /// Pages quarantined behind private tag `asid`.
    pub fn tag_pages(&self, asid: usize) -> usize {
        self.entries
            .lock()
            .iter()
            .filter(|entry| entry.reason.asid() == Some(asid))
            .map(|entry| entry.pages)
            .sum()
    }

    /// `(tag, pages, attempts)` of every private-tag entry, for fixtures that
    /// must name what holds a frame.
    pub fn tag_snapshot(&self) -> Vec<(usize, usize, usize)> {
        let entries = self.entries.lock();
        let mut out = Vec::new();
        if out.try_reserve(entries.len()).is_ok() {
            out.extend(
                entries
                    .iter()
                    .filter_map(|entry| Some((entry.reason.asid()?, entry.pages, entry.attempts))),
            );
        }
        out
    }

    /// Leak `run` (counted, never reused) without logging: the caller holds a
    /// lock the console ranks below and logs the loss after releasing it.
    /// Returns the pages leaked.
    pub(crate) fn forget_unlogged(&self, run: FrameRun) -> usize {
        let pages = run.pages();
        self.counters.leaked.fetch_add(pages, Ordering::AcqRel);
        core::mem::forget(run);
        pages
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
    /// A run whose alias cannot be restored stays quarantined and is retried on
    /// the next pass (never within this one). Event-keyed entries are left to
    /// [`Self::release_event`]. Never waits on a hart. Returns the pages freed.
    pub(crate) fn reap(&self, backend: &mut impl Backend) -> usize {
        let mut freed = 0;
        let mut index = 0;
        let mut retry = Vec::new();
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
                } else if entry_confirmed(backend, entry) {
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
                let (pages, kept) = self.complete(entry, backend);
                freed += pages;
                if let Some(kept) = kept {
                    if retry.try_reserve(1).is_ok() {
                        retry.push(kept);
                    } else {
                        for run in kept.runs {
                            self.leak(run, "no heap to keep an unrestored run");
                        }
                    }
                }
            }
        }
        // Back on the list only now, so this pass cannot retry them forever.
        for entry in retry {
            self.reinsert(entry);
        }
        freed
    }

    /// Free an entry's runs. Returns the pages freed and, when some run's alias
    /// could not be restored, the entry holding the runs still quarantined
    /// (its counters already removed; [`Self::reinsert`] restores them).
    fn complete(&self, mut entry: Entry, backend: &mut impl Backend) -> (usize, Option<Entry>) {
        self.counters.sub(&entry.reason, entry.pages);
        if let Some(asid) = entry.reason.asid() {
            // The lanes pair this line with the `queued`/`extended` line of
            // `memory::deferred_release`.
            log::info!(
                "[aspace] deferred release confirmed: tag={} frames={} attempts={}",
                asid,
                entry.pages,
                entry.attempts
            );
            // Only a confirmed entry reaches here (`reap` checked
            // `entry_confirmed`): every hart published an epoch asked for
            // after the tag's last unmap, which is the proof that the tag is
            // clean. Its slot may be reissued even if a run below still waits
            // for its alias.
            if let Some(lease) = entry.tag_slot.take() {
                lease.release(Invalidated::of(asid as u32));
            }
            backend.tag_wait_ended(asid);
        }
        let mut freed = 0;
        let mut kept = Vec::new();
        let mut kept_pages = 0;
        for mut run in core::mem::take(&mut entry.runs) {
            let pages = run.pages();
            match run.free(backend) {
                Ok(()) => freed += pages,
                Err(_) if run.freeable() && kept.try_reserve(1).is_ok() => {
                    kept_pages += pages;
                    kept.push(run);
                }
                Err(_) if run.freeable() => self.leak(run, "no heap to keep an unrestored run"),
                // A run of another state cannot be in an epoch entry; keep it.
                Err(_) => self.leak(run, "pending run in an unexpected state"),
            }
        }
        self.counters
            .released_after_retry
            .fetch_add(freed, Ordering::AcqRel);
        if freed > 0 {
            log::info!(
                "[frame-run] released {} quarantined pages ({}) after {} attempts",
                freed,
                entry.reason.name(),
                entry.attempts
            );
        }
        if kept.is_empty() {
            return (freed, None);
        }
        // Rate-limited: a sustained shortage logs on powers of two.
        if entry.attempts == 0 || entry.attempts.is_power_of_two() {
            log::error!(
                "[frame-run] {} pages ({}) not freed: their kernel alias could not be restored; they stay quarantined",
                kept_pages,
                entry.reason.name()
            );
        }
        entry.runs = kept;
        entry.pages = kept_pages;
        entry.attempts += 1;
        (freed, Some(entry))
    }

    /// Put back an entry [`Self::complete`] kept, counters included.
    fn reinsert(&self, entry: Entry) {
        let mut entries = self.entries.lock();
        if entries.try_reserve(1).is_ok() {
            self.counters.add(&entry.reason, entry.pages);
            entries.push(entry);
            return;
        }
        drop(entries);
        for run in entry.runs {
            self.leak(run, "no heap to keep an unrestored run");
        }
    }

    /// End every event hold `matches` selects: those runs return to `Mapped`
    /// and to the caller, who resumes their release. Invalidation entries are
    /// never released here — only a published epoch ends them.
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

    /// Pages held under the event keys `matches` selects.
    pub(crate) fn event_pages(&self, matches: impl Fn(&Reason) -> bool) -> usize {
        self.entries
            .lock()
            .iter()
            .filter(|entry| !entry.reason.waits_for_epochs() && matches(&entry.reason))
            .map(|entry| entry.pages)
            .sum()
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
                "[frame-run] allocation refused: {} pages awaiting an invalidation exceed 1/64 of {} managed frames (refusals {})",
                self.counters.awaiting_invalidation(),
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
    /// A private tag every run also waits for ([`Self::await_tag`]).
    tag: Option<usize>,
}

impl Batch {
    pub const fn new() -> Self {
        Self {
            ranges: Vec::new(),
            all: false,
            runs: Vec::new(),
            drain_user_copies: false,
            tag: None,
        }
    }

    /// Every run retired into this batch also waits for private tag `asid`,
    /// whose bounded probe did not confirm after the caller unmapped the frames
    /// from that domain root. The runs are then quarantined as
    /// [`Reason::TagUnconfirmed`] whatever the shared flush says, and the
    /// reaper frees them once every remote hart has published an epoch asked
    /// for after this flush.
    pub fn await_tag(&mut self, asid: usize) {
        self.tag = Some(asid);
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
    /// Adjacent ranges merge; with no heap for a new range, or a range that
    /// wraps the address space, the batch widens to a full invalidation rather
    /// than drop one.
    pub fn invalidate_range(&mut self, start: VAddr, size: usize) {
        if size == 0 || self.all {
            return;
        }
        let Some(end) = start.checked_add(size) else {
            self.invalidate_all();
            return;
        };
        if let Some(last) = self.ranges.last_mut() {
            if last.0.checked_add(last.1) == Some(start) {
                last.1 += size;
                return;
            }
            if last.0 <= start
                && last
                    .0
                    .checked_add(last.1)
                    .is_some_and(|last_end| end <= last_end)
            {
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

    /// Keep `hold` reserved until the last run retired so far is freed (an
    /// owner whose VA must outlive its frames' invalidation, census risk 2).
    /// `false` when there is no such run, or it already holds a range: the
    /// caller keeps the VA and releases it itself.
    pub fn hold_va(&mut self, hold: VaHold) -> bool {
        match self.runs.last_mut() {
            Some(run) if run.va_hold.is_none() => {
                run.va_hold = Some(hold);
                true
            }
            _ => false,
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
    /// allocator and so may not wait — [`Self::complete_without_wait`].
    pub fn complete(self) -> FlushOutcome {
        match FlushCap::take() {
            Some(mut cap) => self.finish(&mut cap),
            None => self.complete_without_wait(),
        }
    }

    /// The local flush plus the remote requests, without waiting for any
    /// answer, for a hart that may not wait (it holds the frame allocator, or
    /// runs the timer reaper). Nothing is freed here — a free takes the frame
    /// allocator — so every run is quarantined behind the epochs asked for
    /// (none on a single hart: the next reap frees it) and the reaper frees it.
    pub(crate) fn complete_without_wait(mut self) -> FlushOutcome {
        let flushed = if self.all || !self.ranges.is_empty() {
            crate::memory::tlb_shootdown::flush_ranges_without_wait(&self.ranges, self.all)
        } else {
            FlushOutcome::Confirmed
        };
        let outcome = match flushed {
            FlushOutcome::Confirmed if !self.runs.is_empty() => FlushOutcome::Unconfirmed {
                epochs: [0; MAX_HARTS],
                cause: UnconfirmedCause::NotAwaited { hart: None },
            },
            outcome => outcome,
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
        let behind_tag = self
            .tag
            .take()
            .map(|asid| (asid, tag_epochs(asid, outcome, &mut backend)));
        let mut quarantined = 0;
        for run in self.runs.drain(..) {
            let pages = run.pages();
            if let Some((asid, epochs)) = &behind_tag {
                if let Err(run) = quarantine_behind_tag(run, *asid, epochs, &PENDING) {
                    PENDING.leak(run, "batch run not unpublished");
                }
                continue;
            }
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
/// these lines; a fixture that holds a CPU on purpose brackets its own. A
/// flush nobody waited for is not a silent hart: it logs its own line (and
/// only when it quarantined something).
fn log_unconfirmed(cause: UnconfirmedCause, quarantined: usize) {
    let (hart, what) = match cause {
        UnconfirmedCause::Timeout { hart } => (hart, "silent through the probe"),
        UnconfirmedCause::RfenceFailed { hart } => (
            hart,
            "silent through the epoch fallback after an RFENCE error",
        ),
        UnconfirmedCause::NotAwaited { .. } => {
            if quarantined > 0 {
                log::warn!(
                    "[frame-run] shared flush not awaited (this hart holds the frame allocator): {} pages wait for the reaper",
                    quarantined
                );
            }
            return;
        }
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
// reason: only the x86 silent-CPU fixture (`smp_bringup.rs`) arms it.
#[allow(dead_code)]
pub(crate) fn set_test_silent_cpu_fixture(armed: bool) {
    TEST_SILENT_CPU_FIXTURE.store(armed, Ordering::Release);
}

/// The kernel's [`Backend`]: per-hart flush epochs, the kernel root, the frame
/// allocator.
pub(crate) struct KernelBackend;

impl KernelBackend {
    /// The leaf [`Backend::normalise`] restores: the boot alias of a free
    /// frame, read/write and never executable.
    pub(crate) const ALIAS: crate::memory::paging::Flags = crate::memory::paging::FREE_FRAME_ALIAS;
}

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

    /// Restore the identity leaf as kernel RW+NX ([`KernelBackend::ALIAS`]):
    /// every free frame is identity-mapped there on RV64/AArch64, because the
    /// loader zeroes a fresh frame through `phys_to_virt`. The run is
    /// unpublished and its invalidation confirmed, so each leaf is new and
    /// needs no shared flush; the local flush drops any invalid entry this hart
    /// may still cache. A page restored by an earlier, partly failed attempt is
    /// skipped. Never waits: the hart-0 timer reaper runs this.
    fn normalise(&mut self, run: &FrameRun) -> bool {
        // `frame::with` below would self-deadlock on a hart that holds it.
        debug_assert!(!crate::memory::frame::held_here());
        let (base, pages) = (run.base(), run.pages());
        let restored = crate::memory::frame::with(|allocator| {
            let mut batch = Batch::new();
            let mut restored = true;
            for page in 0..pages {
                let frame = base + page * PAGE_SIZE;
                if crate::memory::paging::virt_to_phys(frame) == Some(frame) {
                    continue;
                }
                if crate::memory::paging::map_page(allocator, &mut batch, frame, frame, Self::ALIAS)
                    .is_err()
                {
                    restored = false;
                    break;
                }
            }
            (batch, restored)
        });
        // No allocator, nothing restored (and no paging to flush yet).
        let Some((batch, restored)) = restored else {
            return true;
        };
        let _ = batch.complete_without_wait();
        crate::memory::paging::tlb_flush_all();
        restored
    }

    #[cfg(feature = "test-hooks")]
    fn tag_ack_withheld(&self) -> bool {
        crate::memory::tlb_shootdown::test_tag_ack_withheld()
    }

    /// Re-arm the scheduler's housekeeping pass: the domain-grant sweep skips
    /// a row while its tag is unconfirmed.
    fn tag_wait_ended(&mut self, _asid: usize) {
        crate::task::note_housekeeping();
    }

    fn free(&mut self, base: PhysAddr, pages: usize, hold: Option<VaHold>) {
        // `frame::with` below would self-deadlock on a hart that holds it.
        debug_assert!(!crate::memory::frame::held_here());
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

/// The quarantine cap: refuse new allocations once the pages quarantined
/// behind an invalidation ([`Counters::awaiting_invalidation`]) exceed 1/64 of
/// the frames the allocator manages.
pub const fn cap_exceeded(quarantined: usize, total_frames: usize) -> bool {
    quarantined.saturating_mul(64) > total_frames
}

#[cfg(test)]
#[path = "frame_run_tests.rs"]
mod tests;
