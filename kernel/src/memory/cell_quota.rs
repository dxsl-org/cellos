//! Per-cell quota accounts.
//!
//! - `QUOTA_LIMITS`: one atomic limit word per CellId slot, `UNREGISTERED` when
//!   the slot holds no incarnation; `GUEST_LIMITS` beside it.
//! - `IN_USE` (memory), `DMA_IN_USE` and `GUEST_IN_USE`: one atomic counter per
//!   slot, packing the registered incarnation's generation above its live byte
//!   count.
//!
//! A charge names its owner explicitly ([`Charge`]): one Cell incarnation, or
//! the kernel. Nothing here locks or allocates, so a charge or refund is safe
//! inside `GlobalAlloc::alloc` and in interrupt context, and every refund is
//! checked against the generation it was charged to.

use core::sync::atomic::{AtomicUsize, Ordering};
use types::CellId;

/// Maximum CellId tracked (index into IN_USE array).
///
/// 64 is the production ceiling: it sizes the per-cell counter array and the
/// CellId space. The D5 per-request-server gate asks how many cells actually fit
/// now that the boot memory map is read from the DTB, and at 64 this constant
/// binds long before memory does — so `cell-scale-experiment` raises it for the
/// measurement lane (scripts/qemu-cell-scale.sh) only.
#[cfg(not(feature = "cell-scale-experiment"))]
pub const MAX_CELLS: usize = 64;
#[cfg(feature = "cell-scale-experiment")]
pub const MAX_CELLS: usize = 4096;

/// Default heap quota per Cell: 16 MiB.
///
/// Raised from 4 MiB to support heavier cells (DOOM needs ~10 MiB for WAD +
/// zone heap; compositor and VFS also benefit from the headroom).  The quota
/// is a runtime charge limit — no physical pages are pre-allocated.
pub const DEFAULT_QUOTA_BYTES: usize = 16 * 1024 * 1024;

// The constant above remains the hard default/ceiling used by limit tests.
// Cold boot may lower the default for subsequently spawned cells only.
static RUNTIME_DEFAULT_QUOTA_BYTES: AtomicUsize = AtomicUsize::new(DEFAULT_QUOTA_BYTES);

pub fn default_quota_bytes() -> usize {
    RUNTIME_DEFAULT_QUOTA_BYTES.load(Ordering::Acquire)
}

pub(crate) fn set_default_quota_bytes(bytes: usize) {
    assert!((1024 * 1024..=DEFAULT_QUOTA_BYTES).contains(&bytes));
    RUNTIME_DEFAULT_QUOTA_BYTES.store(bytes, Ordering::Release);
}

/// A quota owner: one Cell incarnation, or the kernel (cell 0).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Charge {
    cell: CellId,
    generation: u64,
}

impl Charge {
    /// The kernel: never limited and never counted.
    pub(crate) const KERNEL: Charge = Charge::new(CellId(0), 0);

    /// Incarnation `generation` of `cell`.
    pub(crate) const fn new(cell: CellId, generation: u64) -> Self {
        Self { cell, generation }
    }

    /// The incarnation `task` runs as.
    pub(crate) fn of(task: &crate::task::Task) -> Self {
        Self::new(task.cell_id, task.cell_generation)
    }

    /// The incarnation dispatched on this hart. Only syscall entry turns it
    /// into an allocation charge (`memory::heap::ChargeScope`).
    pub(crate) fn dispatched() -> Self {
        Self::new(
            CellId(crate::sched::hart_local::current_cell_id() as u64),
            crate::sched::hart_local::current_cell_generation(),
        )
    }

    pub(crate) fn cell(self) -> CellId {
        self.cell
    }

    pub(crate) fn generation(self) -> u64 {
        self.generation
    }

    /// The counter slot this charge names; `None` for the kernel and for a
    /// CellId beyond the table, neither of which is counted.
    fn slot(self) -> Option<usize> {
        if self.cell == Self::KERNEL.cell {
            return None;
        }
        usize::try_from(self.cell.0)
            .ok()
            .filter(|&id| id < MAX_CELLS)
    }

    /// The generation bits a counter word carries for this incarnation.
    fn tag(self) -> usize {
        (self.generation as usize & GENERATION_MASK) << GENERATION_SHIFT
    }
}

/// Which counter a charge draws from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Account {
    /// Kernel heap, and the frames billed beside it (thread stacks, SHM,
    /// domain page tables).
    Memory,
    /// Bytes published for device DMA; bounded by the memory limit.
    Dma,
    /// A VM's guest RAM, nested or Stage-2 tables and control structures;
    /// bounded by the guest limit.
    Guest,
}

impl Account {
    fn counters(self) -> &'static [AtomicUsize; MAX_CELLS] {
        match self {
            Self::Memory => &IN_USE,
            Self::Dma => &DMA_IN_USE,
            Self::Guest => &GUEST_IN_USE,
        }
    }
}

/// The limits an incarnation is registered with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Limits {
    /// Bound on the memory account, and on the DMA account.
    pub(crate) memory: usize,
    /// Bound on the guest account; 0 refuses every VM.
    pub(crate) guest: usize,
}

/// A charge was refused; nothing was taken.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct QuotaExceeded;

/// `QUOTA_LIMITS` value of a slot with no registered incarnation.
const UNREGISTERED: usize = usize::MAX;

/// Limit per CellId slot. A registered limit is clamped to `BYTES_MASK`, so it
/// never equals `UNREGISTERED`. Read lock-free by every charge.
static QUOTA_LIMITS: [AtomicUsize; MAX_CELLS] =
    [const { AtomicUsize::new(UNREGISTERED) }; MAX_CELLS];

/// Live memory bytes per CellId slot.
///
/// Each word packs the registered incarnation's generation (bits
/// `GENERATION_SHIFT..`) above its live byte count (`BYTES_MASK`). Every charge
/// and refund compares the generation in the same CAS that adds or subtracts,
/// so bytes charged to an earlier incarnation of a reused CellId can never move
/// the current incarnation's counter.
///
/// Uses an inline `const { }` repeat-array seed (not a named `const`) — each
/// slot is independently zero-initialized at compile time, so there is no
/// shared-instance footgun (clippy::declare_interior_mutable_const) to worry
/// about; a named `static` cannot be used here because `AtomicUsize` is not
/// `Copy`, which the `[expr; N]` repeat form otherwise requires.
static IN_USE: [AtomicUsize; MAX_CELLS] = [const { AtomicUsize::new(0) }; MAX_CELLS];

/// Live DMA-published bytes per CellId slot, packed as `IN_USE`.
/// DMA quota = 1× memory quota (validated 2026-06-22).
static DMA_IN_USE: [AtomicUsize; MAX_CELLS] = [const { AtomicUsize::new(0) }; MAX_CELLS];

/// Guest limit per CellId slot, read only while `QUOTA_LIMITS` holds the slot;
/// 0 for a slot that holds no incarnation.
static GUEST_LIMITS: [AtomicUsize; MAX_CELLS] = [const { AtomicUsize::new(0) }; MAX_CELLS];

/// Live guest bytes per CellId slot, packed as `IN_USE`.
static GUEST_IN_USE: [AtomicUsize; MAX_CELLS] = [const { AtomicUsize::new(0) }; MAX_CELLS];

/// Low bits of a counter word: live bytes, at most 4 GiB - 1 per account.
/// Every account's bound fits: the memory and DMA limits default to
/// `DEFAULT_QUOTA_BYTES` (16 MiB), the largest guest budget is 1544 MiB
/// (`catalog`), and a larger registered limit is clamped to `BYTES_MASK`.
const GENERATION_SHIFT: u32 = 32;
const BYTES_MASK: usize = (1 << GENERATION_SHIFT) - 1;
/// The task generation bits a counter word keeps (the low 32). A generation is
/// minted per incarnation (`tcb::next_cell_generation`; a thread takes its
/// Cell's), so a stale charge or refund can alias a later incarnation of its
/// CellId only after 2^32 incarnations.
const GENERATION_MASK: usize = (1 << (usize::BITS - GENERATION_SHIFT)) - 1;
const _: () = assert!(
    usize::BITS == 64,
    "counter words pack generation and bytes into 64 bits"
);
const _: () = assert!(
    DEFAULT_QUOTA_BYTES <= BYTES_MASK && MAX_CELLS <= BYTES_MASK,
    "a counter's byte field holds every default limit, and an owner token's \
     low bits every CellId"
);

/// Owner token for allocations that are not charged to any Cell row.
pub(crate) const UNCHARGED_OWNER: usize = 0;

/// `word` with `size` live bytes removed and its generation kept.
fn without_bytes(word: usize, size: usize) -> usize {
    (word & !BYTES_MASK) | (word & BYTES_MASK).saturating_sub(size)
}

#[cfg(feature = "p01-trace")]
struct BoundQuota {
    ready: core::sync::atomic::AtomicBool,
    cell: core::sync::atomic::AtomicU64,
    generation: core::sync::atomic::AtomicU64,
    retired: core::sync::atomic::AtomicBool,
}
#[cfg(feature = "p01-trace")]
static P01_SOURCE: crate::p01_resources::Source = crate::p01_resources::Source::new();
#[cfg(feature = "p01-trace")]
static P01_SUBJECTS: [BoundQuota; 9] = [const {
    BoundQuota {
        ready: core::sync::atomic::AtomicBool::new(false),
        cell: core::sync::atomic::AtomicU64::new(0),
        generation: core::sync::atomic::AtomicU64::new(0),
        retired: core::sync::atomic::AtomicBool::new(false),
    }
}; 9];

#[cfg(feature = "p01-trace")]
pub(crate) fn p01_static_bytes() -> usize {
    core::mem::size_of_val(&P01_SOURCE) + core::mem::size_of_val(&P01_SUBJECTS)
}
/// Publication binds the quota row while the exact incarnation still owns it.
/// This is a copy-only hook; it never locks or allocates.
#[cfg(feature = "p01-trace")]
pub(crate) fn p01_bind(subject: u8, cell: u64, generation: u64) {
    let _write = P01_SOURCE.write();
    let Some(row) = P01_SUBJECTS.get(subject as usize) else {
        P01_SOURCE.fault();
        return;
    };
    if cell == 0
        || cell as usize >= MAX_CELLS
        || generation == 0
        || row.ready.load(Ordering::SeqCst)
    {
        P01_SOURCE.fault();
        return;
    }
    row.cell.store(cell, Ordering::SeqCst);
    row.generation.store(generation, Ordering::SeqCst);
    row.ready.store(true, Ordering::SeqCst);
}
#[cfg(feature = "p01-trace")]
pub(crate) fn p01_snapshot() -> crate::p01_resources::Metric {
    let before = P01_SOURCE.before();
    let mut charge = 0u64;
    for (index, row) in P01_SUBJECTS.iter().enumerate() {
        if !row.ready.load(Ordering::SeqCst) || row.retired.load(Ordering::SeqCst) {
            continue;
        }
        let cell = row.cell.load(Ordering::SeqCst);
        let generation = row.generation.load(Ordering::SeqCst);
        // Worker rows, if ever present, describe the same Cell charge. Never
        // multiply the execution-context counter by the number of task rows.
        if P01_SUBJECTS[..index].iter().any(|prior| {
            prior.ready.load(Ordering::SeqCst)
                && !prior.retired.load(Ordering::SeqCst)
                && prior.cell.load(Ordering::SeqCst) == cell
                && prior.generation.load(Ordering::SeqCst) == generation
        }) {
            continue;
        }
        let value = (IN_USE[cell as usize].load(Ordering::SeqCst) & BYTES_MASK) as u64;
        match charge.checked_add(value) {
            Some(next) => charge = next,
            None => P01_SOURCE.fault(),
        }
    }
    // Retired means this execution-context charge row was reset/deregistered,
    // never that deferred physical resources or historical objects were freed.
    P01_SOURCE.metric(charge, before, 5)
}

/// Start slot `id`'s counters from zero under `tag`.
fn reset_counters(cell_id: CellId, tag: usize) {
    let id = cell_id.0 as usize;
    #[cfg(feature = "p01-trace")]
    let _write = P01_SOURCE.write();
    #[cfg(feature = "p01-trace")]
    if P01_SUBJECTS.iter().any(|row| {
        row.ready.load(Ordering::SeqCst)
            && !row.retired.load(Ordering::SeqCst)
            && row.cell.load(Ordering::SeqCst) == cell_id.0
    }) {
        P01_SOURCE.fault();
    }
    IN_USE[id].store(tag, Ordering::Release);
    DMA_IN_USE[id].store(tag, Ordering::Release);
    GUEST_IN_USE[id].store(tag, Ordering::Release);
}

/// Register incarnation `generation` of `cell_id` with `limits`, replacing
/// whatever the slot held: its counters restart from zero under the new
/// generation. Allocates nothing and takes no lock.
pub(crate) fn register(cell_id: CellId, generation: u64, limits: Limits) {
    let charge = Charge::new(cell_id, generation);
    let Some(id) = charge.slot() else {
        return;
    };
    reset_counters(cell_id, charge.tag());
    GUEST_LIMITS[id].store(limits.guest.min(BYTES_MASK), Ordering::Release);
    QUOTA_LIMITS[id].store(limits.memory.min(BYTES_MASK), Ordering::Release);
}

/// Deregister a Cell on exit: the slot holds no incarnation, and its counters
/// keep the retired generation at zero bytes, so a late refund is a no-op.
pub(crate) fn deregister(cell_id: CellId) {
    let Some(id) = Charge::new(cell_id, 0).slot() else {
        return;
    };
    #[cfg(feature = "p01-trace")]
    let _write = P01_SOURCE.write();
    #[cfg(feature = "p01-trace")]
    for row in &P01_SUBJECTS {
        if row.ready.load(Ordering::SeqCst) && row.cell.load(Ordering::SeqCst) == cell_id.0 {
            row.retired.store(true, Ordering::SeqCst);
        }
    }
    QUOTA_LIMITS[id].store(UNREGISTERED, Ordering::Release);
    GUEST_LIMITS[id].store(0, Ordering::Release);
    for counters in [&IN_USE, &DMA_IN_USE, &GUEST_IN_USE] {
        let _ = counters[id].try_update(Ordering::AcqRel, Ordering::Acquire, |word| {
            Some(word & !BYTES_MASK)
        });
    }
}

/// Rollback owner for a quota row belonging to an unpublished cell.
pub(crate) struct QuotaReservation {
    cell_id: CellId,
    committed: bool,
}

impl QuotaReservation {
    /// Reserve the exact requested CellId for incarnation `generation`.
    pub(crate) fn reserve(
        cell_id: CellId,
        generation: u64,
        limits: Limits,
    ) -> Result<Self, types::ViError> {
        let charge = Charge::new(cell_id, generation);
        let Some(id) = charge.slot() else {
            return Err(types::ViError::PermissionDenied);
        };
        // The claim is the limit store itself, so two reservations of one
        // CellId cannot both succeed.
        if QUOTA_LIMITS[id]
            .compare_exchange(
                UNREGISTERED,
                limits.memory.min(BYTES_MASK),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return Err(types::ViError::AlreadyExists);
        }
        reset_counters(cell_id, charge.tag());
        GUEST_LIMITS[id].store(limits.guest.min(BYTES_MASK), Ordering::Release);
        Ok(Self {
            cell_id,
            committed: false,
        })
    }

    /// Reserve the lowest vacant bounded identity. Cell identity is independent
    /// from the monotonic task ID, so exited cells release their quota slot for
    /// later publication rather than permanently exhausting `MAX_CELLS`.
    pub(crate) fn reserve_next(generation: u64, limits: Limits) -> Result<Self, types::ViError> {
        for raw in 1..MAX_CELLS {
            match Self::reserve(CellId(raw as u64), generation, limits) {
                Ok(reservation) => return Ok(reservation),
                Err(types::ViError::AlreadyExists) => {}
                Err(error) => return Err(error),
            }
        }
        log::error!(
            "Stack alloc failed: cell quota table exhausted (max={})",
            MAX_CELLS
        );
        Err(types::ViError::OutOfMemory)
    }

    pub(crate) fn cell_id(&self) -> CellId {
        self.cell_id
    }

    /// Transfer cleanup responsibility to the published task exit path.
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for QuotaReservation {
    fn drop(&mut self) {
        if !self.committed {
            deregister(self.cell_id);
        }
    }
}

/// Add `bytes` to `charge`'s `account`. Refused past the slot's limit, or when
/// the slot holds a different incarnation. The kernel, a CellId beyond the
/// table and an unregistered slot have no limit and are not counted
/// (`Ok(false)`).
fn add(charge: Charge, account: Account, bytes: usize) -> Result<bool, QuotaExceeded> {
    let Some(id) = charge.slot() else {
        return Ok(false);
    };
    let registered = QUOTA_LIMITS[id].load(Ordering::Acquire);
    if registered == UNREGISTERED {
        return Ok(false);
    }
    let limit = match account {
        Account::Memory | Account::Dma => registered,
        Account::Guest => GUEST_LIMITS[id].load(Ordering::Acquire),
    };
    let tag = charge.tag();
    #[cfg(feature = "p01-trace")]
    let _write = P01_SOURCE.write();
    // checked_add refuses a hostile size near MAX instead of wrapping past the
    // limit, and the byte count can never carry into the generation bits.
    account.counters()[id]
        .try_update(Ordering::AcqRel, Ordering::Acquire, |word| {
            if word & !BYTES_MASK != tag {
                return None;
            }
            let bytes = (word & BYTES_MASK)
                .checked_add(bytes)
                .filter(|&total| total <= limit)?;
            Some(tag | bytes)
        })
        .map(|_| true)
        .map_err(|_| QuotaExceeded)
}

/// Return `bytes` to `charge`'s `account`. A no-op unless the slot still holds
/// that incarnation, so a refund never lands on a later incarnation of the
/// CellId. Saturating: never underflows into the generation bits.
pub(crate) fn refund(charge: Charge, account: Account, bytes: usize) {
    let Some(id) = charge.slot() else {
        return;
    };
    let tag = charge.tag();
    #[cfg(feature = "p01-trace")]
    let _write = P01_SOURCE.write();
    let _ = account.counters()[id].try_update(Ordering::AcqRel, Ordering::Acquire, |word| {
        (word & !BYTES_MASK == tag).then(|| without_bytes(word, bytes))
    });
}

/// Bytes taken from one account of one incarnation, refunded once on drop, to
/// that incarnation only (a no-op once the slot was re-registered).
#[must_use = "dropping a reservation refunds it"]
#[derive(Debug)]
pub(crate) struct Reservation {
    charge: Charge,
    account: Account,
    bytes: usize,
}

impl Reservation {
    /// Take `bytes` from `charge`'s `account`, or refuse with nothing taken.
    pub(crate) fn take(
        charge: Charge,
        account: Account,
        bytes: usize,
    ) -> Result<Self, QuotaExceeded> {
        add(charge, account, bytes)?;
        Ok(Self {
            charge,
            account,
            bytes,
        })
    }

    /// The kernel's budget for an object that draws frames over its life
    /// (a kernel-owned VM's tables): every split succeeds and nothing is
    /// counted.
    pub(crate) const fn kernel(account: Account) -> Self {
        Self {
            charge: Charge::KERNEL,
            account,
            bytes: usize::MAX,
        }
    }

    /// Draw `bytes` down into a reservation of their own, same owner and
    /// account; `None`, changing nothing, when fewer remain.
    pub(crate) fn split(&mut self, bytes: usize) -> Option<Self> {
        self.bytes = self.bytes.checked_sub(bytes)?;
        Some(Self {
            charge: self.charge,
            account: self.account,
            bytes,
        })
    }

    /// Leave the bytes charged with no owning object. They return only through
    /// [`refund`] with the same charge and account, or at `deregister`.
    pub(crate) fn keep(self) {
        core::mem::forget(self);
    }

    /// Hand the bytes to an object that refunds them itself (a `FrameRun`
    /// when its frames are freed): they stay charged, and the parts name the
    /// refund.
    pub(crate) fn into_parts(self) -> (Charge, Account, usize) {
        let parts = (self.charge, self.account, self.bytes);
        core::mem::forget(self);
        parts
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        refund(self.charge, self.account, self.bytes);
    }
}

/// Charge `size` heap bytes to `charge` and return the owner token the charge
/// was recorded against, or `None` if it is refused — the caller
/// (`QuotaAlloc::alloc`) must return `null_mut()` in that case.
///
/// The token names the exact incarnation (CellId + generation) that was
/// charged; `UNCHARGED_OWNER` when the charge is not counted. Pass it to
/// `refund_owner` so the refund reaches that incarnation and nothing else.
pub(crate) fn charge_owner(charge: Charge, size: usize) -> Option<usize> {
    match add(charge, Account::Memory, size) {
        Ok(true) => Some(charge.tag() | charge.cell.0 as usize),
        Ok(false) => Some(UNCHARGED_OWNER),
        Err(QuotaExceeded) => None,
    }
}

/// Refund `size` bytes to the row incarnation named by `owner` (from
/// `charge_owner`).  Lock-free.  A no-op when the row has been re-registered
/// since the charge: that incarnation's counter was reset and is not ours.
pub(crate) fn refund_owner(owner: usize, size: usize) {
    let cell_id_raw = owner & BYTES_MASK;
    if cell_id_raw == 0 || cell_id_raw >= MAX_CELLS {
        return;
    }
    #[cfg(feature = "p01-trace")]
    let _write = P01_SOURCE.write();
    let _ = IN_USE[cell_id_raw].try_update(Ordering::AcqRel, Ordering::Acquire, |word| {
        (word & !BYTES_MASK == owner & !BYTES_MASK).then(|| without_bytes(word, size))
    });
}

/// The registered memory limit of a Cell slot, or `None` when it has none.
/// Lock-free; safe under any lock.
pub fn limit(cell_id_raw: usize) -> Option<usize> {
    QUOTA_LIMITS
        .get(cell_id_raw)
        .map(|limit| limit.load(Ordering::Acquire))
        .filter(|&limit| limit != UNREGISTERED)
}

/// Current byte usage for a Cell (for diagnostics).
pub fn in_use(cell_id: CellId) -> usize {
    let id = cell_id.0 as usize;
    if id < MAX_CELLS {
        IN_USE[id].load(Ordering::Relaxed) & BYTES_MASK
    } else {
        0
    }
}

/// Current guest-account usage for a Cell (for diagnostics and fixtures).
pub fn guest_in_use(cell_id: CellId) -> usize {
    GUEST_IN_USE
        .get(cell_id.0 as usize)
        .map_or(0, |word| word.load(Ordering::Acquire) & BYTES_MASK)
}

#[cfg(feature = "test-hooks")]
#[derive(Debug, PartialEq)]
pub(crate) struct QuotaSnapshot {
    limits: alloc::vec::Vec<Option<usize>>,
    heap: alloc::vec::Vec<usize>,
    dma: alloc::vec::Vec<usize>,
    guest: alloc::vec::Vec<usize>,
}

#[cfg(feature = "test-hooks")]
/// Copies quota state into heap-backed rows.
///
/// Heap-backed, never stack-resident: under `cell-scale-experiment` `MAX_CELLS`
/// is 4096, so these four arrays are 64 KiB + 3 × 32 KiB = 160 KiB — more than
/// the 64 KiB kernel stack (`__stack_bottom..__stack_top`, `kernel/linker.ld`).
/// A stack copy wrote straight through every static below the stack — `PLATFORM`
/// sits 5.5 KiB under `__stack_bottom` — so the boot self-tests ran on smashed
/// state and the kernel panicked in `platform::with` ("platform::init not called
/// before platform::with"); measured 2026-10-03. Returning by value moved the
/// same 128 KiB through `StateSnapshot`, so the whole snapshot chain must stay
/// heap-backed.
pub(crate) fn snapshot() -> QuotaSnapshot {
    let mut limits = alloc::vec![None; MAX_CELLS];
    let mut heap = alloc::vec![0; MAX_CELLS];
    let mut dma = alloc::vec![0; MAX_CELLS];
    let mut guest = alloc::vec![0; MAX_CELLS];
    for cell_id in 0..MAX_CELLS {
        limits[cell_id] = limit(cell_id);
        heap[cell_id] = IN_USE[cell_id].load(Ordering::Acquire) & BYTES_MASK;
        dma[cell_id] = DMA_IN_USE[cell_id].load(Ordering::Acquire) & BYTES_MASK;
        guest[cell_id] = GUEST_IN_USE[cell_id].load(Ordering::Acquire) & BYTES_MASK;
    }
    QuotaSnapshot {
        limits,
        heap,
        dma,
        guest,
    }
}

#[cfg(feature = "test-hooks")]
pub(crate) fn registered_limit_for_test(cell_id: CellId) -> Option<usize> {
    limit(cell_id.0 as usize)
}

#[cfg(feature = "test-hooks")]
pub(crate) fn reusable_cell_id_contract() -> bool {
    const GENERATION: u64 = 1;
    let limits = Limits {
        memory: DEFAULT_QUOTA_BYTES,
        guest: 0,
    };
    let before = snapshot();
    let mut held = alloc::vec::Vec::new();
    loop {
        match QuotaReservation::reserve_next(GENERATION, limits) {
            Ok(reservation) => held.push(reservation),
            Err(types::ViError::OutOfMemory) | Err(types::ViError::PermissionDenied) => break,
            Err(_) => return false,
        }
    }
    let Some(released) = held.pop() else {
        return false;
    };
    let released_id = released.cell_id();
    drop(released);
    let reused = QuotaReservation::reserve_next(GENERATION, limits)
        .map(|reservation| reservation.cell_id() == released_id)
        .unwrap_or(false);
    drop(held);
    reused && snapshot() == before
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;
    use alloc::sync::Arc;
    use core::sync::atomic::AtomicUsize;
    use std::sync::Barrier;
    use std::thread;

    const CELL_RAW: usize = MAX_CELLS - 1;
    const CELL: CellId = CellId(CELL_RAW as u64);

    fn limits(memory: usize) -> Limits {
        Limits { memory, guest: 0 }
    }

    fn dma_in_use(cell: CellId) -> usize {
        DMA_IN_USE[cell.0 as usize].load(Ordering::Relaxed) & BYTES_MASK
    }

    #[test]
    fn concurrent_workers_share_one_atomic_dma_quota() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        const LIMIT: usize = 4096;
        register(CELL, 1, limits(LIMIT));
        let charge = Charge::new(CELL, 1);

        let barrier = Arc::new(Barrier::new(2));
        let successes = Arc::new(AtomicUsize::new(0));
        let mut workers = alloc::vec::Vec::new();
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            let successes = Arc::clone(&successes);
            workers.push(thread::spawn(move || {
                let reservation = Reservation::take(charge, Account::Dma, LIMIT);
                if reservation.is_ok() {
                    successes.fetch_add(1, Ordering::Relaxed);
                }
                barrier.wait();
                drop(reservation);
            }));
        }
        for worker in workers {
            worker.join().expect("DMA quota worker");
        }

        assert_eq!(successes.load(Ordering::Relaxed), 1);
        assert_eq!(dma_in_use(CELL), 0);
        deregister(CELL);
    }

    #[test]
    fn stale_generation_refund_leaves_reregistered_row_untouched() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        register(CELL, 1, limits(4096));
        let first = charge_owner(Charge::new(CELL, 1), 100).expect("gen1 charge fits");
        assert_eq!(in_use(CELL), 100);

        register(CELL, 2, limits(4096));
        let second = charge_owner(Charge::new(CELL, 2), 40).expect("gen2 charge fits");
        assert_ne!(first, second);
        refund_owner(first, 100);
        assert_eq!(in_use(CELL), 40, "stale gen1 refund must not touch gen2");

        refund_owner(second, 40);
        assert_eq!(in_use(CELL), 0);
        deregister(CELL);
    }

    #[test]
    fn charge_limit_boundary_is_unchanged() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        register(CELL, 1, limits(4096));
        let charge = Charge::new(CELL, 1);
        let full = Reservation::take(charge, Account::Memory, 4096).expect("limit fits");
        assert_eq!(
            Reservation::take(charge, Account::Memory, 1).unwrap_err(),
            QuotaExceeded
        );
        assert!(Reservation::take(charge, Account::Memory, usize::MAX).is_err());
        assert_eq!(in_use(CELL), 4096);
        drop(full);
        assert_eq!(in_use(CELL), 0);
        assert_eq!(charge_owner(Charge::KERNEL, 1), Some(UNCHARGED_OWNER));
        deregister(CELL);
    }

    #[test]
    fn refused_reservation_takes_nothing() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        register(CELL, 1, limits(4096));
        let charge = Charge::new(CELL, 1);
        let held = Reservation::take(charge, Account::Memory, 4000).expect("fits");
        assert!(Reservation::take(charge, Account::Memory, 97).is_err());
        assert!(Reservation::take(charge, Account::Dma, 4097).is_err());
        assert_eq!((in_use(CELL), dma_in_use(CELL)), (4000, 0));
        // An incarnation the slot does not hold is refused, not billed to it.
        assert!(Reservation::take(Charge::new(CELL, 2), Account::Memory, 1).is_err());
        assert_eq!(in_use(CELL), 4000);
        drop(held);
        deregister(CELL);
    }

    #[test]
    fn split_draws_down_and_each_part_refunds_once() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        register(CELL, 1, limits(4096));
        let mut whole =
            Reservation::take(Charge::new(CELL, 1), Account::Memory, 3000).expect("fits");
        let part = whole.split(1000).expect("1000 of 3000 remain");
        assert!(whole.split(2001).is_none(), "only 2000 remain");
        assert_eq!(in_use(CELL), 3000, "a split moves bytes, it charges none");
        drop(part);
        assert_eq!(in_use(CELL), 2000);
        let rest = whole.split(2000).expect("the remainder");
        drop(whole);
        assert_eq!(in_use(CELL), 2000, "an emptied reservation refunds nothing");
        drop(rest);
        assert_eq!(in_use(CELL), 0);
        deregister(CELL);
    }

    #[test]
    fn stale_reservation_refund_misses_the_next_incarnation() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        register(CELL, 7, limits(4096));
        let stale = Reservation::take(Charge::new(CELL, 7), Account::Memory, 100).expect("fits");
        let stale_dma = Reservation::take(Charge::new(CELL, 7), Account::Dma, 50).expect("fits");
        deregister(CELL);
        register(CELL, 8, limits(4096));
        let live = Reservation::take(Charge::new(CELL, 8), Account::Memory, 40).expect("fits");
        let live_dma = Reservation::take(Charge::new(CELL, 8), Account::Dma, 20).expect("fits");
        drop(stale);
        drop(stale_dma);
        refund(Charge::new(CELL, 7), Account::Memory, 40);
        assert_eq!((in_use(CELL), dma_in_use(CELL)), (40, 20));
        drop(live);
        drop(live_dma);
        assert_eq!((in_use(CELL), dma_in_use(CELL)), (0, 0));
        deregister(CELL);
    }

    #[test]
    fn kernel_and_unregistered_charges_are_not_counted() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        deregister(CELL);
        let kernel = Reservation::take(Charge::KERNEL, Account::Memory, usize::MAX);
        assert!(kernel.is_ok());
        let unregistered = Reservation::take(Charge::new(CELL, 3), Account::Memory, 1 << 20);
        assert!(unregistered.is_ok());
        assert_eq!(in_use(CELL), 0);
        assert_eq!(limit(CELL_RAW), None);
    }

    #[test]
    fn reserve_claims_a_slot_once() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let first = QuotaReservation::reserve(CELL, 5, limits(4096)).expect("vacant");
        assert_eq!(
            QuotaReservation::reserve(CELL, 6, limits(4096)).err(),
            Some(types::ViError::AlreadyExists)
        );
        assert_eq!(limit(CELL_RAW), Some(4096));
        drop(first);
        assert_eq!(limit(CELL_RAW), None);
    }

    #[test]
    fn guest_reservation_is_bounded_by_the_guest_limit_alone() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        register(CELL, 1, limits(4096));
        let charge = Charge::new(CELL, 1);
        assert!(
            Reservation::take(charge, Account::Guest, 1).is_err(),
            "no guest limit refuses every VM"
        );
        register(
            CELL,
            2,
            Limits {
                memory: 0,
                guest: 64 * 4096,
            },
        );
        let charge = Charge::new(CELL, 2);
        assert!(Reservation::take(charge, Account::Guest, 65 * 4096).is_err());
        assert_eq!(guest_in_use(CELL), 0, "a refusal takes nothing");
        let mut whole = Reservation::take(charge, Account::Guest, 64 * 4096).expect("fits");
        assert_eq!((guest_in_use(CELL), in_use(CELL)), (64 * 4096, 0));
        let page = whole.split(4096).expect("a page remains");
        drop(whole);
        assert_eq!(guest_in_use(CELL), 4096);
        drop(page);
        assert_eq!(guest_in_use(CELL), 0);
        deregister(CELL);
        assert!(Reservation::take(charge, Account::Guest, 1).is_ok());
        assert_eq!(guest_in_use(CELL), 0, "an unregistered slot is not counted");
    }

    #[test]
    fn stale_guest_refund_misses_the_next_incarnation() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        let guest = |generation| {
            register(
                CELL,
                generation,
                Limits {
                    memory: 0,
                    guest: 1 << 20,
                },
            )
        };
        guest(1);
        let stale = Reservation::take(Charge::new(CELL, 1), Account::Guest, 8192).expect("fits");
        deregister(CELL);
        guest(2);
        let live = Reservation::take(Charge::new(CELL, 2), Account::Guest, 4096).expect("fits");
        drop(stale);
        refund(Charge::new(CELL, 1), Account::Guest, 4096);
        assert_eq!(guest_in_use(CELL), 4096, "gen 1 refunds miss gen 2");
        drop(live);
        assert_eq!(guest_in_use(CELL), 0);
        deregister(CELL);
    }

    #[test]
    fn every_account_bound_fits_the_byte_field() {
        let largest_guest = crate::catalog::rows()
            .map(|cell| cell.guest_memory)
            .max()
            .unwrap_or(0);
        assert_eq!(largest_guest, 1544 * 1024 * 1024, "the hypervisor budget");
        assert!(largest_guest <= BYTES_MASK && DEFAULT_QUOTA_BYTES <= BYTES_MASK);

        let _guard = crate::TEST_STATE_LOCK.lock();
        register(
            CELL,
            1,
            Limits {
                memory: DEFAULT_QUOTA_BYTES,
                guest: largest_guest,
            },
        );
        let charge = Charge::new(CELL, 1);
        let guest = Reservation::take(charge, Account::Guest, largest_guest).expect("fits");
        let memory = Reservation::take(charge, Account::Memory, DEFAULT_QUOTA_BYTES).expect("fits");
        assert_eq!(
            (guest_in_use(CELL), in_use(CELL)),
            (largest_guest, DEFAULT_QUOTA_BYTES),
            "a full account never carries into the generation"
        );
        assert!(Reservation::take(charge, Account::Guest, 1).is_err());
        drop((guest, memory));
        assert_eq!((guest_in_use(CELL), in_use(CELL)), (0, 0));
        deregister(CELL);
    }

    #[test]
    fn generations_differing_in_any_low_32_bit_never_alias() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        for stale in [1u64, (1 << 24) + 1, (1 << 31) + 1] {
            for bit in 0..32 {
                let live = stale ^ (1 << bit);
                register(CELL, stale, limits(4096));
                let old = Reservation::take(Charge::new(CELL, stale), Account::Memory, 100)
                    .expect("fits");
                let old_owner = charge_owner(Charge::new(CELL, stale), 8).expect("fits");
                register(CELL, live, limits(4096));
                let current =
                    Reservation::take(Charge::new(CELL, live), Account::Memory, 40).expect("fits");
                assert!(
                    Reservation::take(Charge::new(CELL, stale), Account::Memory, 1).is_err(),
                    "generation {stale} must not charge {live}"
                );
                drop(old);
                refund_owner(old_owner, 8);
                assert_eq!(in_use(CELL), 40, "generation {stale} refunded {live}");
                drop(current);
            }
        }
        deregister(CELL);
    }
}
