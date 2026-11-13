//! Retirement verdicts for `QueryRetirement` (Phase 09B K1).
//!
//! A root retirement opens a `Pending` record keyed by its incarnation
//! `(cell_id, generation, root_tid)`, holding the death subscribers of the root
//! at that moment. The retirement funnel closes it with the post-quiescence
//! tally, classified by [`classify`]. Only a recorded watcher or a supervisor
//! may read it.
//!
//! The table is bounded: a full table evicts the oldest final record, never a
//! `Pending` one; with every slot `Pending`, a new retirement is not recorded and
//! its query reports no record, which callers treat as `Unproven`.
//!
//! Lock order: `TABLE` is a leaf. `open` runs under SCHEDULER and the death
//! subscriber lock; nothing here allocates or takes another lock.

use crate::sync::Spinlock;
use abi::cell_owner::CellOwner;
use abi::retirement::{reason, RetirementState, ViRetirementV1, RETIREMENT_RECORD_VERSION};

/// Records kept at once.
pub const MAX_RECORDS: usize = 64;
/// Watchers kept per record; later subscribers are not admitted to the query.
pub const MAX_WATCHERS: usize = 8;

/// What the retirement funnel observed for one incarnation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RetirementTally {
    pub member_threads: usize,
    pub remote_hart_mask: u64,
    pub grants_reclaimed: usize,
    pub grantee_refs_cleared: usize,
    pub writable_borrows: usize,
    pub pins_quarantined: usize,
    pub vfs_leases_pending: usize,
    pub shm_frames_released: usize,
    pub dma_deferred: usize,
    pub quarantined_pages: usize,
    pub leaked_pages: usize,
}

/// Why a query returned no record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueryError {
    /// No record for that root: unknown, evicted or never recorded.
    NotFound,
    /// The caller was not a watcher of that root and is not a supervisor.
    Denied,
}

#[derive(Clone, Copy)]
struct Slot {
    watchers: [usize; MAX_WATCHERS],
    watcher_count: usize,
    /// Order of the last open/close; the lowest final slot is evicted first.
    seq: u64,
    record: ViRetirementV1,
}

impl Slot {
    fn owns(&self, owner: CellOwner) -> bool {
        self.record.cell_id == owner.cell_id
            && self.record.generation == owner.generation
            && self.record.root_tid == owner.root_tid
    }

    fn pending(&self) -> bool {
        self.record.state == RetirementState::Pending as u16
    }
}

struct Table {
    slots: [Option<Slot>; MAX_RECORDS],
    next_seq: u64,
}

static TABLE: Spinlock<Table> = Spinlock::ranked(
    Table {
        slots: [None; MAX_RECORDS],
        next_seq: 0,
    },
    crate::sync::rank::RETIREMENT_VERDICTS,
);

/// State and reason bits for a final tally. Untracked SHM or a quarantine
/// overflow is `Unproven`; else deferred DMA or any bounded quarantine is
/// `Quarantined`; else `Contained`. `WRITABLE_BORROW` never changes the state.
pub(crate) fn classify(tally: &RetirementTally) -> (RetirementState, u32) {
    let mut reasons = 0;
    if tally.shm_frames_released > 0 {
        reasons |= reason::SHM_UNTRACKED;
    }
    if tally.leaked_pages > 0 {
        reasons |= reason::QUARANTINE_OVERFLOW;
    }
    if tally.writable_borrows > 0 {
        reasons |= reason::WRITABLE_BORROW;
    }
    if tally.dma_deferred > 0 {
        reasons |= reason::DMA_DEFERRED;
    }
    if tally.quarantined_pages > 0 || tally.pins_quarantined > 0 || tally.vfs_leases_pending > 0 {
        reasons |= reason::TRACKED_QUARANTINE;
    }
    let state = if reasons & (reason::SHM_UNTRACKED | reason::QUARANTINE_OVERFLOW) != 0 {
        RetirementState::Unproven
    } else if reasons & (reason::DMA_DEFERRED | reason::TRACKED_QUARANTINE) != 0 {
        RetirementState::Quarantined
    } else {
        RetirementState::Contained
    };
    (state, reasons)
}

/// Record that `owner` began retiring, admitting the first [`MAX_WATCHERS`] of
/// `watchers` to its query. Returns `false` when every slot is `Pending`.
pub(crate) fn open(owner: CellOwner, watchers: &[usize]) -> bool {
    let mut table = TABLE.lock();
    let seq = table.next_seq;
    let index = table
        .slots
        .iter()
        .position(|s| s.is_some_and(|s| s.owns(owner)))
        .or_else(|| table.slots.iter().position(Option::is_none))
        .or_else(|| {
            (0..MAX_RECORDS)
                .filter(|&i| table.slots[i].is_some_and(|s| !s.pending()))
                .min_by_key(|&i| table.slots[i].map_or(u64::MAX, |s| s.seq))
        });
    let Some(index) = index else {
        return false;
    };
    let mut slot = Slot {
        watchers: [0; MAX_WATCHERS],
        watcher_count: watchers.len().min(MAX_WATCHERS),
        seq,
        record: ViRetirementV1 {
            version: RETIREMENT_RECORD_VERSION,
            state: RetirementState::Pending as u16,
            cell_id: owner.cell_id,
            generation: owner.generation,
            root_tid: owner.root_tid,
            ..ViRetirementV1::default()
        },
    };
    slot.watchers[..slot.watcher_count].copy_from_slice(&watchers[..slot.watcher_count]);
    table.slots[index] = Some(slot);
    table.next_seq = seq.wrapping_add(1);
    true
}

/// Finalize `owner`'s `Pending` record with `tally`. No-op when it has none.
pub(crate) fn close(owner: CellOwner, tally: RetirementTally) {
    let count = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
    let (state, reasons) = classify(&tally);
    let mut table = TABLE.lock();
    let seq = table.next_seq;
    let Some(slot) = table
        .slots
        .iter_mut()
        .flatten()
        .find(|s| s.owns(owner) && s.pending())
    else {
        return;
    };
    slot.seq = seq;
    slot.record = ViRetirementV1 {
        state: state as u16,
        reasons,
        member_threads: count(tally.member_threads),
        remote_hart_mask: tally.remote_hart_mask,
        grants_reclaimed: count(tally.grants_reclaimed),
        grantee_refs_cleared: count(tally.grantee_refs_cleared),
        writable_borrows: count(tally.writable_borrows),
        pins_quarantined: count(tally.pins_quarantined),
        vfs_leases_pending: count(tally.vfs_leases_pending),
        shm_frames_released: count(tally.shm_frames_released),
        dma_deferred: count(tally.dma_deferred),
        quarantined_pages: tally.quarantined_pages as u64,
        leaked_pages: tally.leaked_pages as u64,
        ..slot.record
    };
    table.next_seq = seq.wrapping_add(1);
}

/// The most recent record for `root_tid`, if `caller_tid` was one of its
/// watchers or `caller_is_supervisor`.
pub(crate) fn query(
    caller_tid: usize,
    caller_is_supervisor: bool,
    root_tid: usize,
) -> Result<ViRetirementV1, QueryError> {
    let table = TABLE.lock();
    let slot = table
        .slots
        .iter()
        .flatten()
        .filter(|s| s.record.root_tid == root_tid as u64)
        .max_by_key(|s| s.seq)
        .ok_or(QueryError::NotFound)?;
    if caller_is_supervisor || slot.watchers[..slot.watcher_count].contains(&caller_tid) {
        Ok(slot.record)
    } else {
        Err(QueryError::Denied)
    }
}

#[cfg(test)]
#[path = "retirement_verdict_tests.rs"]
pub(crate) mod tests;
