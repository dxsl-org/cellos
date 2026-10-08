//! IOMMU common API — three-phase DMA isolation.
//!
//! Phase 1 `init()`             — probe hardware, allocate page tables, stay passthrough.
//! Phase 2 `map_dma()`          — drivers register each DMA buffer's physical range.
//! Phase 3 `activate_isolation()` — switch from passthrough to enforced page-table mode.
//!
//! Call order in `main.rs`:
//!   `iommu::init(firmware)` → driver DMA allocs (call `map_dma()`) → `iommu::activate_isolation()`

use cellos_boards::DmaIsolation;
use core::sync::atomic::{AtomicBool, Ordering};

/// Firmware-discovered remapping input for the boot path.
///
/// x86_64 fills this from the parsed ACPI DMAR table. Architectures with no
/// equivalent table pass [`Default::default`], which leaves their own backends
/// (ARM EL2 stage-2, RISC-V IOMMU) untouched.
#[derive(Clone, Copy, Debug, Default)]
pub struct DmaIsolationInput {
    /// Intel VT-d DRHD register base (0 = no DMAR table, or no usable unit).
    pub vtd_base: u64,
    /// DRHD unit count the DMAR table declared (0 = no DMAR table).
    pub vtd_units: u8,
    /// The selected DRHD covers PCI devices that declare no device scope.
    pub vtd_include_pci_all: bool,
}

/// What the kernel does with a DMA request when no remapper was discovered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
pub enum DmaWithoutRemapper {
    /// Refuse the request: this profile requires translation.
    Refuse,
    /// Allow identity addressing: this profile declares no remapper.
    Identity,
}

/// Decide the DMA contract when no remapping hardware is present.
///
/// Never resolved silently: the caller logs the arm it took, so a machine that
/// runs untranslated DMA says so, and a profile that requires isolation refuses
/// instead of degrading to identity addressing.
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
pub const fn dma_without_remapper(required: DmaIsolation) -> DmaWithoutRemapper {
    match required {
        DmaIsolation::Required => DmaWithoutRemapper::Refuse,
        DmaIsolation::Optional => DmaWithoutRemapper::Identity,
    }
}

/// Set once when a machine without a remapper is allowed identity DMA, so the
/// condition appears in the boot log without repeating on every buffer.
///
/// Only the x86_64 arm reaches the identity-DMA branch, so every other target
/// would otherwise carry a dead static — and `-D warnings` turns that into a
/// build failure for the whole lane (e.g. `scripts/build-aarch64-test-hooks-ci.sh`).
/// Same idiom as `dma_without_remapper` above.
#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
static IDENTITY_DMA_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmaMapResult {
    Mapped(u64),
    Rejected,
    /// Hardware may observe the published mapping; its pin must be retained.
    PublishedUnconfirmed,
}

/// Outcome of a per-range DMA teardown.
///
/// Mirrors [`DmaMapResult`]: a teardown that zeroed leaves the hardware may
/// still translate is reported as unconfirmed, never as done — the caller must
/// keep the frames quarantined until the invalidation is retried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmaUnmapResult {
    /// Nothing was mapped in the range: no teardown was needed.
    NothingMapped,
    /// Every cleared leaf's translation is gone from hardware.
    Unmapped { pages: usize },
    /// Leaves were zeroed but the IOMMU did not acknowledge the invalidation.
    PublishedUnconfirmed { pages: usize },
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
pub fn init(dma: DmaIsolationInput) {
    #[cfg(target_arch = "riscv64")]
    {
        let _ = dma;
        super::iommu_riscv::init_hw();
    }
    #[cfg(target_arch = "x86_64")]
    super::iommu_x86::init_hw(dma);
    #[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
    let _ = dma;
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
            // Present but not enforcing yet: fail closed (unchanged).
            DmaMapResult::Rejected
        } else {
            let board = crate::board::selected();
            match dma_without_remapper(board.dma_isolation) {
                DmaWithoutRemapper::Refuse => {
                    log::error!(
                        "[iommu] DMA isolation REQUIRED by board {} but no remapper was \
                         discovered — refusing untranslated DMA (tid={} bdf={:#x} \
                         phys={:#x} size={})",
                        board.slug,
                        tid,
                        bdf,
                        phys,
                        size
                    );
                    DmaMapResult::Rejected
                }
                DmaWithoutRemapper::Identity => {
                    if !IDENTITY_DMA_LOGGED.swap(true, Ordering::Relaxed) {
                        log::warn!(
                            "[iommu] board {} declares no DMA remapper; DMA is untranslated \
                             (identity addressing)",
                            board.slug
                        );
                    }
                    DmaMapResult::Mapped(phys)
                }
            }
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

/// Tear down `[iova, iova+size)` from Cell `tid`'s DMA domain.
///
/// Zeros the leaf entries and invalidates the translations covering them, so a
/// device the Cell authorised faults on its next access to the range. Cell-death
/// teardown uses [`revoke_dma_for_cell`] instead: same invalidation, but it also
/// drops the requester entries and the domain itself.
#[inline]
pub fn unmap_dma(tid: u64, iova: u64, size: usize) -> DmaUnmapResult {
    #[cfg(target_arch = "riscv64")]
    {
        super::iommu_riscv::unmap_range_for_cell(tid, iova, size)
    }
    #[cfg(target_arch = "x86_64")]
    {
        super::iommu_x86::unmap_range_for_cell(tid, iova, size)
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "x86_64")))]
    {
        let _ = (tid, iova, size);
        DmaUnmapResult::NothingMapped
    }
}

/// Tear down `tid`'s whole DMA domain: clear its requester entries, zero its
/// DDT/context, and invalidate its cached translations.
///
/// The single domain-teardown path, reached from two callers: Cell death (the
/// task-exit sequence, before grant frames are released) and a runtime revoke
/// of `pcie_driver` — a Cell that loses DMA authority mid-life gets exactly the
/// teardown it would get by dying.
///
/// Returns `true` only when hardware acknowledged teardown. Callers must keep
/// pinned frames quarantined when it returns `false`.
pub fn revoke_dma_for_cell(tid: u64) -> bool {
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
    // the IOMMU device has been registered in PCI_DEVICES. The deferred path is
    // RISC-V only, whose remapper is not discovered from ACPI DMAR.
    init(DmaIsolationInput::default());

    // If init_hw() found the IOMMU hardware (BAR0 != 0), activate isolation.
    // activate() is a no-op when init_hw() returned early (device not found yet).
    activate_isolation();

    if IOMMU_ISOLATED.load(Ordering::Relaxed) {
        IOMMU_DEFERRED.store(false, Ordering::Relaxed);
        log::info!("[iommu] deferred init complete — DMA isolation active");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine with no remapper either refuses DMA or declares identity
    /// addressing — there is no third, silent outcome.
    #[test]
    fn no_remapper_decision_follows_the_profile_requirement() {
        assert_eq!(
            dma_without_remapper(DmaIsolation::Required),
            DmaWithoutRemapper::Refuse
        );
        assert_eq!(
            dma_without_remapper(DmaIsolation::Optional),
            DmaWithoutRemapper::Identity
        );
    }
}
