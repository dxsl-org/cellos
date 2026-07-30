//! Memory management interfaces.

use crate::*;

pub mod cell_quota;
/// x86 nested paging (EPT/NPT) — Tier 3b x86 VMM. x86_64 only.
#[cfg(target_arch = "x86_64")]
pub mod ept;
pub mod frame;
pub mod heap;
pub mod kaslr;
/// Permission changes on already-mapped pages (`protect_page` / `protect_range`).
/// Re-exported through `paging`; kept separate so `paging.rs` stops growing.
pub mod page_protect;
pub mod paging;
pub mod rt_heap;
pub mod stage2;
pub mod tests;
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
