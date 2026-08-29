//! Per-cell heap quota enforcement.
//!
//! Uses a split design to avoid the alloc-inside-alloc deadlock:
//! - `QUOTA_LIMITS`: `Spinlock<BTreeMap<usize, usize>>` stores the limit per Cell.
//!   Only locked in `register`/`deregister` — never inside `GlobalAlloc::alloc`.
//! - `IN_USE`: `[AtomicUsize; MAX_CELLS]` stores the live byte count per Cell.
//!   Updated atomically without any lock — safe to call from inside `GlobalAlloc::alloc`.

use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicUsize, Ordering};
use types::CellId;

/// Maximum CellId tracked (index into IN_USE array).
pub const MAX_CELLS: usize = 64;

/// Default heap quota per Cell: 16 MiB.
///
/// Raised from 4 MiB to support heavier cells (DOOM needs ~10 MiB for WAD +
/// zone heap; compositor and VFS also benefit from the headroom).  The quota
/// is a runtime charge limit — no physical pages are pre-allocated.
pub const DEFAULT_QUOTA_BYTES: usize = 16 * 1024 * 1024;

/// Limit store — BTreeMap keyed by CellId raw value, stores the byte limit.
/// Locked only in `register`/`deregister` — NOT inside the allocator hot path.
static QUOTA_LIMITS: Spinlock<BTreeMap<usize, usize>> = Spinlock::new(BTreeMap::new());

/// Live byte counters — one AtomicUsize per Cell slot, zero-initialized.
/// Updated lock-free inside `charge`/`refund` to avoid alloc-inside-alloc deadlock.
///
/// Uses an inline `const { }` repeat-array seed (not a named `const`) — each
/// slot is independently zero-initialized at compile time, so there is no
/// shared-instance footgun (clippy::declare_interior_mutable_const) to worry
/// about; a named `static` cannot be used here because `AtomicUsize` is not
/// `Copy`, which the `[expr; N]` repeat form otherwise requires.
static IN_USE: [AtomicUsize; MAX_CELLS] = [const { AtomicUsize::new(0) }; MAX_CELLS];

/// Force-release this module's locks during fault teardown.
///
/// # Safety
/// Single-hart; called only from the fault/panic path with interrupts disabled.
pub unsafe fn force_unlock_locks() {
    QUOTA_LIMITS.force_unlock();
}

/// Register a new Cell with the given heap quota.
///
/// Call this at spawn, OUTSIDE the allocator.
///
/// Deadlock contract: `BTreeMap::insert` allocates a new tree node via the
/// global `QuotaAlloc`, whose `alloc` calls `charge(current_cell_id())`.  When
/// `register` runs inside a cell's syscall (e.g. init calling `SpawnFromPath`),
/// `current_cell_id()` is that cell (non-zero), so `charge` would try to RE-LOCK
/// the `QUOTA_LIMITS` Spinlock we already hold here → self-deadlock.  We pin
/// `CURRENT_CELL_ID` to 0 (kernel = unlimited, charge short-circuits without
/// locking) across the insert so the node allocation never re-enters `charge`'s
/// lock.  The node is kernel bookkeeping, so charging it to the kernel is also
/// semantically correct.
pub fn register(cell_id: CellId, limit: usize) {
    let id = cell_id.0 as usize;
    if id < MAX_CELLS {
        IN_USE[id].store(0, Ordering::Relaxed);
    }
    // Pin to kernel context (0) so the BTreeMap::insert's node allocation does
    // not re-enter `charge` (which locks QUOTA_LIMITS) while we hold that lock.
    // Deadlock contract: see module doc.
    let prev_cell = crate::task::hart_local::current_cell_id();
    crate::task::hart_local::set_current_cell_id(0);
    QUOTA_LIMITS.lock().insert(id, limit);
    crate::task::hart_local::set_current_cell_id(prev_cell);
}

/// Deregister a Cell on exit.
pub fn deregister(cell_id: CellId) {
    let id = cell_id.0 as usize;
    if id < MAX_CELLS {
        IN_USE[id].store(0, Ordering::Relaxed);
        DMA_IN_USE[id].store(0, Ordering::Relaxed);
    }
    QUOTA_LIMITS.lock().remove(&id);
}

/// Rollback owner for a quota row belonging to an unpublished cell.
pub(crate) struct QuotaReservation {
    cell_id: CellId,
    committed: bool,
}

impl QuotaReservation {
    /// Reserve the exact requested CellId.
    pub(crate) fn reserve(cell_id: CellId, limit: usize) -> Result<Self, types::ViError> {
        let id = cell_id.0 as usize;
        if id == 0 || id >= MAX_CELLS {
            return Err(types::ViError::PermissionDenied);
        }
        if QUOTA_LIMITS.lock().contains_key(&id) {
            return Err(types::ViError::AlreadyExists);
        }
        register(cell_id, limit);
        Ok(Self {
            cell_id,
            committed: false,
        })
    }

    /// Reserve the lowest vacant bounded identity. Cell identity is independent
    /// from the monotonic task ID, so exited cells release their quota slot for
    /// later publication rather than permanently exhausting `MAX_CELLS`.
    pub(crate) fn reserve_next(limit: usize) -> Result<Self, types::ViError> {
        for raw in 1..MAX_CELLS {
            match Self::reserve(CellId(raw as u64), limit) {
                Ok(reservation) => return Ok(reservation),
                Err(types::ViError::AlreadyExists) => {}
                Err(error) => return Err(error),
            }
        }
        Err(types::ViError::PermissionDenied)
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

/// Charge `size` bytes to the Cell.
///
/// Returns `false` if the quota would be exceeded — the caller (`QuotaAlloc::alloc`)
/// must return `null_mut()` in that case.
///
/// Lock-ordering: acquires `QUOTA_LIMITS` briefly for a read (no allocation inside),
/// then updates `IN_USE` atomically without any lock.
pub fn charge(cell_id_raw: usize, size: usize) -> bool {
    if cell_id_raw == 0 {
        return true; // kernel itself: unlimited
    }
    // Read the limit — BTreeMap::get does NOT allocate.  Lock released immediately.
    let limit = QUOTA_LIMITS
        .lock()
        .get(&cell_id_raw)
        .copied()
        .unwrap_or(usize::MAX);
    if cell_id_raw >= MAX_CELLS {
        return true; // no slot in IN_USE — uncapped
    }
    // Optimistic add; roll back on breach.
    // Use saturating_add to prevent wrapping past usize::MAX — a hostile layout.size()
    // near MAX would otherwise make the comparison read false and bypass the limit.
    let prev = IN_USE[cell_id_raw].fetch_add(size, Ordering::Relaxed);
    if prev.saturating_add(size) > limit {
        IN_USE[cell_id_raw].fetch_sub(size, Ordering::Relaxed);
        false
    } else {
        true
    }
}

/// Refund `size` bytes when the Cell frees memory.  Lock-free.
///
/// Uses saturating subtraction to prevent underflow if a deallocation is
/// attributed to a cell that has already been deregistered (e.g., dealloc
/// of a Box shared with another cell arrives after the originating cell exited).
pub fn refund(cell_id_raw: usize, size: usize) {
    if cell_id_raw == 0 || cell_id_raw >= MAX_CELLS {
        return;
    }
    // fetch_update with saturating_sub prevents wrapping to usize::MAX on spurious frees.
    let _ = IN_USE[cell_id_raw].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
        Some(cur.saturating_sub(size))
    });
}

/// Current byte usage for a Cell (for diagnostics).
pub fn in_use(cell_id: CellId) -> usize {
    let id = cell_id.0 as usize;
    if id < MAX_CELLS {
        IN_USE[id].load(Ordering::Relaxed)
    } else {
        0
    }
}

// ── DMA quota tracking (for sys_grant_dma) ───────────────────────────────────

/// Live DMA-mapped byte counts — one AtomicUsize per Cell slot, zero-initialized.
/// DMA quota = 1× memory quota (validated 2026-06-22).
///
/// See `IN_USE` above for why this uses an inline `const { }` repeat seed
/// instead of a named `const`.
static DMA_IN_USE: [AtomicUsize; MAX_CELLS] = [const { AtomicUsize::new(0) }; MAX_CELLS];

/// Atomic DMA quota reservation. Dropping an uncommitted reservation refunds
/// its bytes, so every failure path can roll back without a separate check/add
/// window.
pub(crate) struct DmaQuotaReservation {
    cell_id_raw: usize,
    size: usize,
    committed: bool,
}

impl DmaQuotaReservation {
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for DmaQuotaReservation {
    fn drop(&mut self) {
        if !self.committed {
            record_dma_unmapped(self.cell_id_raw, self.size);
        }
    }
}

/// Atomically reserve `size` bytes against the Cell's DMA quota.
///
/// Concurrent worker tasks in one Cell contend on the same atomic counter; at
/// most the reservations fitting within the configured limit can succeed.
pub(crate) fn try_reserve_dma(cell_id_raw: usize, size: usize) -> Option<DmaQuotaReservation> {
    if cell_id_raw == 0 || cell_id_raw >= MAX_CELLS {
        return Some(DmaQuotaReservation {
            cell_id_raw,
            size,
            committed: true,
        });
    }
    let limit = QUOTA_LIMITS
        .lock()
        .get(&cell_id_raw)
        .copied()
        .unwrap_or(DEFAULT_QUOTA_BYTES);
    DMA_IN_USE[cell_id_raw]
        .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |current| {
            current.checked_add(size).filter(|next| *next <= limit)
        })
        .ok()?;
    Some(DmaQuotaReservation {
        cell_id_raw,
        size,
        committed: false,
    })
}

/// Record `size` bytes of DMA released for Cell `cell_id_raw`. Lock-free.
pub fn record_dma_unmapped(cell_id_raw: usize, size: usize) {
    if cell_id_raw == 0 || cell_id_raw >= MAX_CELLS {
        return;
    }
    let _ = DMA_IN_USE[cell_id_raw].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
        Some(cur.saturating_sub(size))
    });
}

#[cfg(feature = "test-hooks")]
#[derive(Debug, PartialEq)]
pub(crate) struct QuotaSnapshot {
    limits: [Option<usize>; MAX_CELLS],
    heap: [usize; MAX_CELLS],
    dma: [usize; MAX_CELLS],
}

#[cfg(feature = "test-hooks")]
/// Copies quota state into fixed-size cells so `QUOTA_LIMITS` is never held
/// across an allocation that would re-enter `charge`.
pub(crate) fn snapshot() -> QuotaSnapshot {
    let mut limits = [None; MAX_CELLS];
    for (&cell_id, &limit) in QUOTA_LIMITS.lock().iter() {
        if cell_id < MAX_CELLS {
            limits[cell_id] = Some(limit);
        }
    }

    let mut heap = [0; MAX_CELLS];
    let mut dma = [0; MAX_CELLS];
    for cell_id in 0..MAX_CELLS {
        heap[cell_id] = IN_USE[cell_id].load(Ordering::Acquire);
        dma[cell_id] = DMA_IN_USE[cell_id].load(Ordering::Acquire);
    }
    QuotaSnapshot { limits, heap, dma }
}

#[cfg(feature = "test-hooks")]
pub(crate) fn registered_limit_for_test(cell_id: CellId) -> Option<usize> {
    QUOTA_LIMITS.lock().get(&(cell_id.0 as usize)).copied()
}

#[cfg(feature = "test-hooks")]
pub(crate) fn reusable_cell_id_contract() -> bool {
    let before = snapshot();
    let mut held = alloc::vec::Vec::new();
    loop {
        match QuotaReservation::reserve_next(DEFAULT_QUOTA_BYTES) {
            Ok(reservation) => held.push(reservation),
            Err(types::ViError::PermissionDenied) => break,
            Err(_) => return false,
        }
    }
    let Some(released) = held.pop() else {
        return false;
    };
    let released_id = released.cell_id();
    drop(released);
    let reused = QuotaReservation::reserve_next(DEFAULT_QUOTA_BYTES)
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

    #[test]
    fn concurrent_workers_share_one_atomic_dma_quota() {
        let _guard = crate::TEST_STATE_LOCK.lock();
        const CELL_RAW: usize = MAX_CELLS - 1;
        const LIMIT: usize = 4096;
        let cell = CellId(CELL_RAW as u64);
        register(cell, LIMIT);

        let barrier = Arc::new(Barrier::new(2));
        let successes = Arc::new(AtomicUsize::new(0));
        let mut workers = alloc::vec::Vec::new();
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            let successes = Arc::clone(&successes);
            workers.push(thread::spawn(move || {
                let reservation = try_reserve_dma(CELL_RAW, LIMIT);
                if reservation.is_some() {
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
        assert_eq!(DMA_IN_USE[CELL_RAW].load(Ordering::Relaxed), 0);
        deregister(cell);
    }
}
