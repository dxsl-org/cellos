//! Memory management interfaces.

use crate::*;

#[cfg(all(feature = "native-domains", cellos_wide))]
pub mod address_space;
pub mod cell_quota;
/// A cell image's segment frames and VA slot, released as one operation.
pub mod cell_segments;
/// Bounded deferred release of page-table frames whose tag invalidation was not
/// confirmed synchronously, plus the reaper that completes them.
#[cfg(all(feature = "native-domains", cellos_wide))]
pub mod deferred_release;
#[cfg(all(feature = "native-domains", cellos_wide))]
#[path = "memory/domain-supervisor-registry.rs"]
pub(crate) mod domain_supervisor_registry;
pub mod frame;
/// One owner type for runs of physical frames and the release contract they
/// follow: unpublish, flush once per operation, normalise, free — or quarantine.
pub mod frame_run;
pub mod heap;
pub mod kaslr;
/// Permission changes on already-mapped pages (`protect_page` / `protect_range`).
/// Re-exported through `paging`; kept separate so `paging.rs` stops growing.
pub mod page_protect;
pub mod paging;
/// Regions an in-flight asynchronous operation still reads or writes, and the
/// quarantine that withholds their frames when the owner dies mid-operation.
pub mod pin;
/// The one record (owner, rights, receiver, pin state, frames) behind the grant,
/// SHM and DMA export tables.
pub mod region;
pub mod rt_heap;
pub mod stage2;
/// One allocator for architectural tags (private-root ASIDs, VMIDs, SVM
/// ASIDs) with a per-owner limit.
pub(crate) mod tag_pool;
// Uncalled allocator test runner; never compiled into a release kernel.
#[cfg(not(cellos_posture = "release"))]
pub mod tests;
/// Completion boundary for changed translations before execution or memory reuse.
pub mod tlb_shootdown;
pub mod vma;

/// Ownership registry entry.
pub struct AllocationInfo {
    /// Address of allocation.
    pub address: VAddr,
    /// Size in bytes.
    pub size: usize,
    /// Owning Cell ID.
    pub owner: CellId,
}

/// Global memory management trait (to be implemented).
pub trait ViGlobalMemoryManager {
    /// Allocate memory for a Cell.
    fn alloc(&self, size: usize, owner: CellId) -> ViResult<VAddr>;

    /// Free memory owned by a Cell.
    fn free(&self, addr: VAddr) -> ViResult<()>;

    /// Transfer ownership of an allocation.
    fn transfer_ownership(&self, addr: VAddr, new_owner: CellId) -> ViResult<()>;
}
