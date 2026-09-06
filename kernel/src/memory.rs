//! Memory management interfaces.

use crate::*;

#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
pub mod address_space;
pub mod cell_quota;
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
#[path = "memory/domain-supervisor-registry.rs"]
pub(crate) mod domain_supervisor_registry;
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
/// Regions an in-flight asynchronous operation still reads or writes, and the
/// quarantine that withholds their frames when the owner dies mid-operation.
pub mod pin;
pub mod rt_heap;
pub mod stage2;
pub mod tests;
/// Completion boundary for changed translations before execution or memory reuse.
pub mod tlb_shootdown;
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub mod tlb_shootdown_selftest;
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
