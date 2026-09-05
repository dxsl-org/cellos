//! IOMMU common API — three-phase DMA isolation.
//!
//! Phase 1 `init()`             — probe hardware, allocate page tables, stay passthrough.
//! Phase 2 `map_dma()`          — drivers register each DMA buffer's physical range.
//! Phase 3 `activate_isolation()` — switch from passthrough to enforced page-table mode.
//!
//! Call order in `main.rs`:
//!   `iommu::init()` → driver DMA allocs (call `map_dma()`) → `iommu::activate_isolation()`

use core::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmaMapResult {
    Mapped(u64),
    Rejected,
    /// Hardware may observe the published mapping; its pin must be retained.
    PublishedUnconfirmed,
}

/// Classify a mapping whose device context was written before invalidation.
///
/// Either a command-queue publication failure or a missing IOFENCE
/// acknowledgement leaves hardware visibility uncertain and keeps the pin.
#[cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]
pub(crate) const fn classify_dma_publication(
    iova: u64,
    invalidation_published: bool,
    fence_acknowledged: bool,
) -> DmaMapResult {
    if invalidation_published && fence_acknowledged {
        DmaMapResult::Mapped(iova)
    } else {
        DmaMapResult::PublishedUnconfirmed
    }
}

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

/// Register a DMA physical range or authorize identity DMA when x86 has no
/// remapping hardware. A present-but-inactive remapper rejects the request.
#[inline]
pub fn map_dma(phys: u64, size: usize) -> Option<u64> {
    match map_dma_for_cell(0, 0, phys, size) {
        DmaMapResult::Mapped(iova) => Some(iova),
        DmaMapResult::Rejected | DmaMapResult::PublishedUnconfirmed => None,
    }
}

/// Register `[phys, phys+size)` for Cell `tid` owning device `bdf`.
///
/// Distinguishes a clean rejection from a mapping that was published before an
/// invalidation timeout; the latter requires the caller to retain the DMA pin.
pub fn map_dma_for_cell(tid: u64, bdf: u32, phys: u64, size: usize) -> DmaMapResult {
    if size == 0 {
        return DmaMapResult::Rejected;
    }
    if !is_active() {
        #[cfg(target_arch = "x86_64")]
        return if super::iommu_x86::is_present() {
            DmaMapResult::Rejected
        } else {
            // No remapping hardware exists; ownership/pinning still succeeded,
            // and the machine's DMA contract is identity addressing.
            DmaMapResult::Mapped(phys)
        };
        #[cfg(not(target_arch = "x86_64"))]
        return DmaMapResult::Rejected;
    }
    #[cfg(target_arch = "riscv64")]
    let mapped = super::iommu_riscv::map_range_for_cell(tid, bdf, phys, size);
    #[cfg(target_arch = "x86_64")]
    let mapped = super::iommu_x86::map_range_for_cell(tid, bdf, phys, size);
    #[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
    let mapped = {
        let _ = (tid, bdf, phys, size);
        DmaMapResult::Rejected
    };
    mapped
}

/// No-op stub. Per-Cell IOTLB invalidation is handled by `cleanup_cell` on Cell exit.
#[inline]
pub fn unmap_dma(_iova: u64, _size: usize) {}

/// Flush IOTLB and zero DDT/context entries for `tid`'s DMA domain.
///
/// Returns `true` only when hardware acknowledged teardown. Callers must keep
/// pinned frames quarantined when it returns `false`.
pub fn cleanup_cell(tid: u64) -> bool {
    #[cfg(target_arch = "riscv64")]
    {
        super::iommu_riscv::unmap_cell(tid)
    }
    #[cfg(target_arch = "x86_64")]
    {
        super::iommu_x86::unmap_cell_domain(tid)
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
    {
        let _ = tid;
        true
    }
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
/// Phase 3 (`activate_isolation`) runs immediately after `init_hw` here. Until
/// activation succeeds, a present remapper causes DMA grants to fail closed;
/// later mappings take effect through the active per-Cell backend.
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
