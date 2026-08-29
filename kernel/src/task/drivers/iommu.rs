//! IOMMU common API — three-phase DMA isolation.
//!
//! Phase 1 `init()`             — probe hardware, allocate page tables, stay passthrough.
//! Phase 2 `map_dma()`          — drivers register each DMA buffer's physical range.
//! Phase 3 `activate_isolation()` — switch from passthrough to enforced page-table mode.
//!
//! Call order in `main.rs`:
//!   `iommu::init()` → driver DMA allocs (call `map_dma()`) → `iommu::activate_isolation()`

use core::sync::atomic::{AtomicBool, Ordering};

static IOMMU_ISOLATED: AtomicBool = AtomicBool::new(false);
/// Set when `pcie_ecam::init()` is removed from the boot path.
/// `try_deferred_init()` (called from `RegisterPciDevice` handler) checks this
/// and runs `init()` once PCI_DEVICES is populated with the IOMMU device entry.
static IOMMU_DEFERRED: AtomicBool = AtomicBool::new(false);

/// Phase 1: probe IOMMU hardware and allocate isolation data structures.
///
/// Must be called after `pcie_ecam::init()` and before any DMA allocation.
/// Does NOT enable enforcement yet — hardware stays in passthrough mode.
pub fn init() {
    #[cfg(target_arch = "riscv64")]
    super::iommu_riscv::init_hw();
    #[cfg(target_arch = "x86_64")]
    super::iommu_x86::init_hw();
}

/// Register a DMA physical range in the IOMMU page table.
///
/// Returns the identity IOVA only after the active architecture backend
/// confirms that it installed the mapping.
#[inline]
pub fn map_dma(phys: u64, size: usize) -> Option<u64> {
    map_dma_for_cell(0, 0, phys, size)
}

/// Register `[phys, phys+size)` for Cell `tid` owning device `bdf`.
///
/// Returns `None` unless translation enforcement is already active.
pub fn map_dma_for_cell(tid: u64, bdf: u32, phys: u64, size: usize) -> Option<u64> {
    if size == 0 || !is_active() {
        return None;
    }
    #[cfg(target_arch = "riscv64")]
    let mapped = super::iommu_riscv::map_range_for_cell(tid, bdf, phys, size);
    #[cfg(target_arch = "x86_64")]
    let mapped = super::iommu_x86::map_range_for_cell(tid, bdf, phys, size);
    #[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
    let mapped = {
        let _ = (tid, bdf, phys, size);
        false
    };
    mapped.then_some(phys)
}

/// No-op stub. Per-Cell IOTLB invalidation is handled by `cleanup_cell` on Cell exit.
#[inline]
pub fn unmap_dma(_iova: u64, _size: usize) {}

/// Flush IOTLB and zero DDT/context entries for `tid`'s DMA domain.
///
/// MUST be called on Cell exit BEFORE DMA frames are returned to the frame allocator.
pub fn cleanup_cell(tid: u64) {
    #[cfg(target_arch = "riscv64")]
    super::iommu_riscv::unmap_cell(tid);
    #[cfg(target_arch = "x86_64")]
    super::iommu_x86::unmap_cell_domain(tid); // Phase 02 will implement
    #[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
    let _ = tid; // no IOMMU backend on this arch yet
}

/// Phase 3: switch IOMMU from passthrough to page-table enforcement.
///
/// On RISC-V: writes DDTP with MODE=1LVL + pre-built Sv39 DDT → faults any
///   IOVA not in a registered DMA range.
/// On x86_64: fills VT-d context entries with TT=TRANSLATED+SLPT, enables TE.
///
/// Call after all driver DMA buffers are registered via `map_dma()`.
pub fn activate_isolation() {
    #[cfg(target_arch = "riscv64")]
    super::iommu_riscv::activate();
    #[cfg(target_arch = "x86_64")]
    super::iommu_x86::activate();
}

/// Returns `true` once `activate_isolation()` has completed successfully.
#[inline]
pub fn is_active() -> bool {
    IOMMU_ISOLATED.load(Ordering::Relaxed)
}

/// Mark DMA isolation as active. Called by arch backends on successful activation.
#[cfg(any(target_arch = "riscv64", target_arch = "x86_64"))]
pub(super) fn set_active() {
    IOMMU_ISOLATED.store(true, Ordering::Relaxed);
}

/// Arm deferred IOMMU init.
///
/// Call from `main.rs` instead of `init()` when the Platform Cell owns PCIe
/// enumeration. `try_deferred_init()` will call `init()` + `activate_isolation()`
/// once the IOMMU device entry appears in `PCI_DEVICES` via `RegisterPciDevice`.
pub fn set_deferred_init_pending() {
    IOMMU_DEFERRED.store(true, Ordering::Relaxed);
}

/// Attempt IOMMU init if deferred and the IOMMU device has been registered.
///
/// Called from the `RegisterPciDevice` syscall handler after each device is added
/// to `PCI_DEVICES`. Returns immediately if already initialized or not deferred.
///
/// Phase 3 (`activate_isolation`) runs immediately after `init_hw` here: by the
/// time the IOMMU device is registered, no Driver Cell DMA has occurred yet
/// (Driver Cells spawn after Platform Cell completes enumeration). Subsequent
/// `map_dma_for_cell` calls add DDT entries lazily and take effect immediately.
pub fn try_deferred_init() {
    if !IOMMU_DEFERRED.load(Ordering::Relaxed) {
        return;
    }
    if IOMMU_ISOLATED.load(Ordering::Relaxed) {
        return;
    }

    // init() calls arch init_hw() which calls find_class() — succeeds only once
    // the IOMMU device has been registered in PCI_DEVICES.
    init();

    // If init_hw() found the IOMMU hardware (BAR0 != 0), activate isolation.
    // activate() is a no-op when init_hw() returned early (device not found yet).
    activate_isolation();

    if IOMMU_ISOLATED.load(Ordering::Relaxed) {
        IOMMU_DEFERRED.store(false, Ordering::Relaxed);
        log::info!("[iommu] deferred init complete — DMA isolation active");
    }
}
