//! IOMMU common API — three-phase DMA isolation behind one [`DmaRemapper`].
//!
//! Phase 1 `init()`             — probe hardware, allocate page tables, stay passthrough.
//! Phase 2 `activate_isolation()` — switch from passthrough to enforced page-table mode.
//! Then `map_dma_for_cell()` publishes each granted DMA buffer in its Cell's domain.
//!
//! Call order in the arch boot (x86_64) and `try_deferred_init` (riscv64),
//! enforced by tokens: `init` returns a [`Probed`] that `activate_isolation`
//! consumes, and its [`Isolation`] result is what `IommuReady` carries:
//!   `iommu::init(firmware)` → `iommu::activate_isolation(probed)`
//!
//! Each architecture names its remapper as `crate::arch::iommu::Backend`
//! (VT-d on x86_64, the RISC-V IOMMU on riscv64, [`NoRemapper`] elsewhere);
//! every call here goes through that one [`DmaRemapper`].

use crate::arch::iommu::{Backend, BACKEND};
use cellos_boards::DmaIsolation;
use core::sync::atomic::{AtomicBool, Ordering};

/// The owner of a DMA domain: the thread that owns the device's record.
pub type DomainKey = u64;

/// A PCI function: segment in bits 31:16, `bus:device.function` below.
pub type DeviceKey = u32;

/// One family of DMA remapping hardware.
///
/// A domain holds the translations granted to one owner; a device joins its
/// owner's domain on the first mapping and leaves it when the owner's domain is
/// revoked. Firmware tables, page-table formats, cache write-back and
/// interrupt remapping stay inside the implementation.
pub trait DmaRemapper: Sync {
    /// What [`Self::probe`] reads from firmware; `()` for a unit found
    /// otherwise.
    type Firmware: Default;

    /// With no unit found, whether the board's `dma_isolation` may grant
    /// untranslated (identity) DMA. When `false`, every request is refused.
    const UNTRANSLATED_WITHOUT_UNIT: bool;

    /// Whether [`Self::map_window`] can publish export windows.
    const EXPORT_WINDOWS: bool;

    /// Find the hardware and allocate its tables; translation stays off.
    fn probe(&self, firmware: Self::Firmware);

    /// Whether [`Self::probe`] found a usable unit.
    fn present(&self) -> bool;

    /// Turn translation on. Calls [`set_active`] once DMA is translated.
    fn activate(&self);

    /// Publish the identity range `[phys, phys + len)` in `owner`'s domain and
    /// attach `dev` to it.
    fn map(&self, owner: DomainKey, dev: DeviceKey, phys: u64, len: usize) -> DmaMapResult;

    /// Publish `[phys, phys + len)` at the export window `[iova, iova + len)`
    /// of `owner`'s domain for `dev`, read-only unless `writable`.
    fn map_window(
        &self,
        owner: DomainKey,
        dev: DeviceKey,
        iova: u64,
        phys: u64,
        len: usize,
        writable: bool,
    ) -> DmaMapResult;

    /// Clear export window `[iova, iova + len)` from `owner`'s domain and flush
    /// its cached translations. `true` only when the flush was acknowledged.
    fn unmap_window(&self, owner: DomainKey, iova: u64, len: usize) -> bool;

    /// Detach every device from `owner`'s domain, flush it and drop it. `true`
    /// only when hardware acknowledged the teardown.
    fn revoke(&self, owner: DomainKey) -> bool;

    /// Whether DMA from `dev` is translated.
    fn translating(&self, dev: DeviceKey) -> bool;

    /// The boot-time teardown probe of this remapper's page table.
    #[cfg(feature = "test-hooks")]
    fn teardown_probe(&self) -> Option<crate::dev::iommu_pt::TeardownProbe> {
        None
    }
}

/// No DMA remapping hardware: every mapping is refused.
#[cfg_attr(any(target_arch = "x86_64", target_arch = "riscv64"), allow(dead_code))]
pub struct NoRemapper;

impl DmaRemapper for NoRemapper {
    type Firmware = ();
    const UNTRANSLATED_WITHOUT_UNIT: bool = false;
    const EXPORT_WINDOWS: bool = false;

    fn probe(&self, _: ()) {}

    fn present(&self) -> bool {
        false
    }

    fn activate(&self) {}

    fn map(&self, owner: DomainKey, dev: DeviceKey, phys: u64, len: usize) -> DmaMapResult {
        let _ = (owner, dev, phys, len);
        DmaMapResult::Rejected
    }

    fn map_window(
        &self,
        owner: DomainKey,
        dev: DeviceKey,
        iova: u64,
        phys: u64,
        len: usize,
        writable: bool,
    ) -> DmaMapResult {
        let _ = (owner, dev, iova, phys, len, writable);
        DmaMapResult::Rejected
    }

    fn unmap_window(&self, owner: DomainKey, iova: u64, len: usize) -> bool {
        let _ = (owner, iova, len);
        true
    }

    fn revoke(&self, owner: DomainKey) -> bool {
        let _ = owner;
        true
    }

    fn translating(&self, dev: DeviceKey) -> bool {
        let _ = dev;
        false
    }
}

/// What the kernel does with a DMA request when no remapper was discovered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
pub const fn dma_without_remapper(required: DmaIsolation) -> DmaWithoutRemapper {
    match required {
        DmaIsolation::Required => DmaWithoutRemapper::Refuse,
        DmaIsolation::Optional => DmaWithoutRemapper::Identity,
    }
}

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
/// Set once when a machine without a remapper is allowed identity DMA, so the
/// condition appears in the boot log without repeating on every buffer.
static IDENTITY_DMA_LOGGED: AtomicBool = AtomicBool::new(false);

/// Phase 1: probe IOMMU hardware and allocate isolation data structures.
///
/// Must be called after `pcie_ecam::init()` and before any DMA allocation.
/// Does NOT enable enforcement yet — hardware stays in passthrough mode.
/// [`activate_isolation`] takes the returned token, so it runs after a probe.
pub fn init(firmware: <Backend as DmaRemapper>::Firmware) -> Probed {
    BACKEND.probe(firmware);
    Probed { _private: () }
}

/// [`init`] ran. Only `init` makes one; [`activate_isolation`] consumes it.
#[must_use]
pub struct Probed {
    _private: (),
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
        return map_inactive(tid, bdf, phys, size);
    }
    BACKEND.map(tid, bdf, phys, size)
}

/// [`map_dma_for_cell`] while isolation is not active.
///
/// A unit that is present but not translating fails closed. With no unit, the
/// board decides between refusal and identity DMA where the remapper allows
/// untranslated DMA at all ([`DmaRemapper::UNTRANSLATED_WITHOUT_UNIT`]).
fn map_inactive(tid: u64, bdf: u32, phys: u64, size: usize) -> DmaMapResult {
    if !Backend::UNTRANSLATED_WITHOUT_UNIT || BACKEND.present() {
        return DmaMapResult::Rejected;
    }
    let board = crate::board::active();
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
}

/// Whether DMA export windows can be published: the remapper supports them and
/// translation is on.
pub fn export_windows_active() -> bool {
    Backend::EXPORT_WINDOWS && is_active()
}

/// Map export window `[iova, iova + size)` of Cell `tid`'s domain for `bdf` to
/// the contiguous frames `[phys, phys + size)`; see [`DmaRemapper::map_window`].
pub fn map_window_for_cell(
    tid: u64,
    bdf: u32,
    iova: u64,
    phys: u64,
    size: usize,
    writable: bool,
) -> DmaMapResult {
    BACKEND.map_window(tid, bdf, iova, phys, size, writable)
}

/// Clear export window `[iova, iova + size)` from Cell `tid`'s domain; see
/// [`DmaRemapper::unmap_window`].
pub fn unmap_window_for_cell(tid: u64, iova: u64, size: usize) -> bool {
    BACKEND.unmap_window(tid, iova, size)
}

/// Whether DMA from `bdf` (segment in bits 31:16) is translated.
pub fn bdf_translating(bdf: u32) -> bool {
    BACKEND.translating(bdf)
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
    BACKEND.revoke(tid)
}

/// What [`activate_isolation`] found. Only that function makes one, so a holder
/// knows [`init`] and the activation ran, and whether a remapper translates DMA.
#[must_use]
pub struct Isolation {
    remapper: Remapper,
}

impl Isolation {
    /// The remapper the activation found, and whether it translates.
    pub fn remapper(&self) -> Remapper {
        self.remapper
    }
}

/// The DMA remapper [`activate_isolation`] found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Remapper {
    /// Found and translating: DMA goes through the per-Cell domains.
    Translating,
    /// Found, but translation did not come up: DMA requests fail closed.
    Inactive,
    /// None found: DMA follows the architecture's no-remapper contract
    /// (x86_64: untranslated or refused, as the board declares).
    NotFound,
}

/// Phase 3: switch IOMMU from passthrough to page-table enforcement
/// ([`DmaRemapper::activate`]); afterwards any IOVA outside a registered DMA
/// range faults.
///
/// Call with the token of the [`init`] that probed the hardware, before any
/// Driver Cell can request DMA.
pub fn activate_isolation(probed: Probed) -> Isolation {
    let Probed { _private: () } = probed;
    BACKEND.activate();
    let remapper = if is_active() {
        Remapper::Translating
    } else if BACKEND.present() {
        Remapper::Inactive
    } else {
        Remapper::NotFound
    };
    Isolation { remapper }
}

/// Returns `true` once `activate_isolation()` has completed successfully.
#[inline]
pub fn is_active() -> bool {
    IOMMU_ISOLATED.load(Ordering::Relaxed)
}

/// Mark DMA isolation as active. Called by arch backends on successful activation.
#[cfg(any(target_arch = "riscv64", target_arch = "x86_64"))]
pub(crate) fn set_active() {
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
/// Phase 3 (`activate_isolation`) runs immediately after the probe here. Until
/// activation succeeds, a present remapper causes DMA grants to fail closed;
/// later mappings take effect through the active per-Cell backend.
pub fn try_deferred_init() {
    if !IOMMU_DEFERRED.load(Ordering::Relaxed) {
        return;
    }
    if IOMMU_ISOLATED.load(Ordering::Relaxed) {
        return;
    }

    // The probe succeeds only once the IOMMU device has been registered in
    // PCI_DEVICES. The deferred path is RISC-V only, whose remapper reads no
    // firmware table (`Firmware` is `()` there, a DMAR summary on x86_64).
    #[allow(clippy::unit_arg)]
    let probed = init(Default::default());

    // If the probe found the IOMMU hardware, activate isolation. Activation is
    // a no-op when the probe returned early (device not found yet).
    if activate_isolation(probed).remapper() == Remapper::Translating {
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

    /// Without hardware, nothing is mapped, nothing is translated, and teardown
    /// has nothing to wait for.
    #[test]
    fn no_remapper_refuses_every_mapping() {
        let r = NoRemapper;
        r.probe(());
        r.activate();
        assert!(!r.present());
        assert_eq!(
            r.map(7, 0x0010, 0x4000_0000, 0x1000),
            DmaMapResult::Rejected
        );
        assert_eq!(
            r.map_window(7, 0x0010, 1 << 38, 0x4000_0000, 0x1000, true),
            DmaMapResult::Rejected
        );
        assert!(r.unmap_window(7, 1 << 38, 0x1000));
        assert!(r.revoke(7));
        assert!(!r.translating(0x0010));
        const { assert!(!NoRemapper::UNTRANSLATED_WITHOUT_UNIT && !NoRemapper::EXPORT_WINDOWS) };
    }
}
