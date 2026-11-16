//! Intel VT-d DMA isolation and Interrupt Remapping driver for x86_64.

pub mod cache;
pub mod context;
pub mod domain;
pub mod gcmd;
pub mod invalidate;
pub mod ir;
pub mod qi;
pub mod remap;
pub mod rmrr;
pub mod slpt;
pub mod unit;

use crate::acpi::{BoundedVec, DeviceScope, DrhdEntry};
use crate::sync::Spinlock;
use alloc::vec::Vec;
use core::sync::atomic::{fence, AtomicUsize, Ordering};

use crate::dev::iommu::{DeviceKey, DmaMapResult, DmaRemapper, DomainKey};
use context::{
    canonical_bdf, clear_ctx_slot, context_entry, context_slot, read_ctx_slot, write_ctx_entry,
};
use domain::VtdDomain;
use invalidate::Scope;
use rmrr::apply_rmrr_to_slpt;
use slpt::VtdSlpt;
use unit::VtdUnit;

/// What the ACPI DMAR table told the boot path about VT-d.
#[derive(Clone, Copy, Debug, Default)]
pub struct Dmar {
    /// DRHD register base (0 = no DMAR table, or no usable unit).
    pub base: u64,
    /// DRHD unit count the DMAR table declared (0 = no DMAR table).
    pub units: u8,
    /// The selected DRHD covers PCI devices that declare no device scope.
    pub include_pci_all: bool,
}

/// Intel VT-d behind [`DmaRemapper`]: a domain per owner (one DID and one
/// [`VtdSlpt`] on each unit), context entries per requester.
pub struct Vtd;

impl DmaRemapper for Vtd {
    type Firmware = Dmar;
    const UNTRANSLATED_WITHOUT_UNIT: bool = true;
    const EXPORT_WINDOWS: bool = true;

    fn probe(&self, dmar: Dmar) {
        init_hw(dmar);
    }

    fn present(&self) -> bool {
        is_present()
    }

    fn activate(&self) {
        activate();
    }

    fn map(&self, owner: DomainKey, dev: DeviceKey, phys: u64, len: usize) -> DmaMapResult {
        map_range_for_cell(owner, dev, phys, len)
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
        map_window_for_cell(owner, dev, iova, phys, len, writable)
    }

    fn unmap_window(&self, owner: DomainKey, iova: u64, len: usize) -> bool {
        unmap_window_for_cell(owner, iova, len)
    }

    fn revoke(&self, owner: DomainKey) -> bool {
        unmap_cell_domain(owner)
    }

    fn translating(&self, dev: DeviceKey) -> bool {
        bdf_translating(dev)
    }

    #[cfg(feature = "test-hooks")]
    fn teardown_probe(&self) -> Option<crate::dev::iommu_pt::TeardownProbe> {
        Some(slpt::teardown_probe())
    }
}

const VTD_BASE: usize = 0xFED9_0000;
static PRIMARY_BASE: AtomicUsize = AtomicUsize::new(VTD_BASE);

static UNITS: Spinlock<Vec<VtdUnit>> = Spinlock::ranked(Vec::new(), crate::sync::rank::UNITS);
fn is_present() -> bool {
    !UNITS.lock().is_empty()
}

/// QEMU q35 I/O APIC requester id 0xF0F8 (`Q35_PSEUDO_BUS_PLATFORM` 0xF0,
/// `Q35_PSEUDO_DEVFN_IOAPIC` 0xF8 = 31.0): the scope QEMU's own DMAR
/// publishes. Used only by the no-DMAR q35 fallback, for the first MADT I/O
/// APIC.
fn q35_ioapic_scope() -> Option<DeviceScope> {
    let id = crate::acpi::with_topology(|topo| topo.ioapics.as_slice().first().map(|a| a.id))?;
    let mut scope = DeviceScope::ZERO;
    scope.kind = remap::SCOPE_IOAPIC;
    scope.enumeration_id = id;
    scope.start_bus = 0xF0;
    scope.path_len = 1;
    scope.path[0] = [0x1F, 0];
    Some(scope)
}

fn init_hw(dmar: Dmar) {
    let mut units = UNITS.lock();
    units.clear();

    if dmar.base != 0 {
        log::info!(
            "[vtd] register base {:#x} from ACPI DMAR (units={} include_pci_all={})",
            dmar.base,
            dmar.units,
            dmar.include_pci_all
        );
        PRIMARY_BASE.store(dmar.base as usize, Ordering::Relaxed);

        crate::acpi::with_topology(|topo| {
            // DMAR INTR_REMAP clear: the platform does not support remapping
            // even where a unit's ECAP.IR says it could.
            let platform_ir = topo.dmar_intr_remap();
            for drhd in topo.drhds.as_slice() {
                if let Some(mut u) = VtdUnit::probe(drhd) {
                    u.ir_supported &= platform_ir;
                    units.push(u);
                }
            }
        });
    } else if crate::board::active().soc == cellos_boards::SocId::QemuX86Q35 {
        log::warn!(
            "[vtd] no ACPI DMAR unit; using the board-declared q35 register base {:#x} (fallback)",
            VTD_BASE
        );
        PRIMARY_BASE.store(VTD_BASE, Ordering::Relaxed);
        let mut fallback_drhd = DrhdEntry {
            flags: 1, // INCLUDE_PCI_ALL
            segment: 0,
            register_base: VTD_BASE as u64,
            scopes: BoundedVec::new(DeviceScope::ZERO),
        };
        if let Some(scope) = q35_ioapic_scope() {
            let _ = fallback_drhd.scopes.push(scope);
        }
        if let Some(u) = VtdUnit::probe(&fallback_drhd) {
            units.push(u);
        }
    } else {
        log::warn!("[vtd] no DMAR-discovered register base; refusing q35 fallback");
        return;
    }

    if units.is_empty() {
        log::info!("[vtd] no usable VT-d units found");
        return;
    }

    log::info!("[vtd] initialized {} VT-d unit(s)", units.len());
}

/// Current software references only; caller excludes local preemption before
/// taking these locks and never holds FRAME_ALLOCATOR or the heap lock.
#[cfg(feature = "p01-trace")]
pub(crate) fn p01_visit_roots(mut visit: impl FnMut(u64, u64, u64, u64, u64)) {
    let units = UNITS.lock();
    for u in units.iter() {
        let root = u.root_phys;
        visit(root, 2, 0, 0, 0);
        // Mutators take DOMAINS before CONTEXTS; keep that order.
        let domains = u.domains.lock();
        let contexts = u.contexts.lock();
        for (&bus, table) in contexts.iter() {
            visit(table.phys, 3, bus as u64, 0, root);
        }
        for (&holder, domain) in domains.iter() {
            visit(domain.slpt.root_phys(), 4, holder, domain.did as u64, root);
        }
    }
}

/// `(canonical requester id, segment)` of a `segment:bus:dev.func` value.
fn split_bdf(bdf: u32) -> (u16, u16) {
    let bus = ((bdf >> 8) & 0xFF) as u8;
    let dev = ((bdf >> 3) & 0x1F) as u8;
    let func = (bdf & 0x07) as u8;
    (canonical_bdf(bus, dev, func), ((bdf >> 16) & 0xFFFF) as u16)
}

pub fn unit_index_for_bdf(units: &[VtdUnit], bdf: u16, segment: u16) -> Option<usize> {
    // 1. Explicit endpoint/bridge scope on matching segment
    for (idx, u) in units.iter().enumerate() {
        if u.segment == segment {
            for scope in u.scopes.as_slice() {
                if rmrr::device_matches_scope(bdf, scope) {
                    return Some(idx);
                }
            }
        }
    }
    // 2. INCLUDE_PCI_ALL unit on matching segment
    for (idx, u) in units.iter().enumerate() {
        if u.segment == segment && u.include_pci_all {
            return Some(idx);
        }
    }
    None
}

fn map_range_for_cell(tid: u64, bdf: u32, phys: u64, size: usize) -> DmaMapResult {
    publish_for_cell(tid, bdf, phys, |_| true, |slpt| slpt.map_range(phys, size))
}

/// Map the contiguous frames `[phys, phys + size)` at the export window
/// `[iova, iova + size)` of Cell `tid`'s domain for `bdf` (ADR-0033 CP1 §5.2,
/// `MapDmaExport`). `writable` clear publishes `R`-only leaves.
///
/// Refused ([`DmaMapResult::Rejected`]) before any change when a leaf in the
/// window is already present: a window never overwrites an identity or RMRR
/// translation. Publication and invalidation are exactly `GrantDma`'s.
pub fn map_window_for_cell(
    tid: u64,
    bdf: u32,
    iova: u64,
    phys: u64,
    size: usize,
    writable: bool,
) -> DmaMapResult {
    publish_for_cell(
        tid,
        bdf,
        iova,
        |slpt| slpt.window_free(iova, size),
        |slpt| slpt.map_window(iova, phys, size, writable),
    )
}

/// Clear export window `[iova, iova + size)` from Cell `tid`'s domain on every
/// unit and drain that domain's IOTLB (drain-read/drain-write).
///
/// The invalidation is issued even when the leaves are already zero: a retry
/// after a timed-out drain must re-drain, not conclude "nothing mapped". A unit
/// without a domain for `tid` has nothing to drain (its teardown was
/// acknowledged, or it never held one). Returns `true` only when every drain
/// was acknowledged.
pub fn unmap_window_for_cell(tid: u64, iova: u64, size: usize) -> bool {
    let units = UNITS.lock();
    let mut confirmed = true;
    for u in units.iter() {
        let domains = u.domains.lock();
        let Some(domain) = domains.get(&tid) else {
            continue;
        };
        domain.slpt.unmap_range(iova, size);
        // Publish the zeroed leaves before the invalidation command.
        fence(Ordering::SeqCst);
        confirmed &= u.invalidate_iotlb(Scope::Domain(domain.did));
    }
    confirmed
}

/// Whether the unit covering `bdf` (segment in bits 31:16) has DMA
/// translation enabled.
pub fn bdf_translating(bdf: u32) -> bool {
    let (canonical, segment) = split_bdf(bdf);
    let units = UNITS.lock();
    unit_index_for_bdf(&units, canonical, segment)
        .is_some_and(|idx| units[idx].active.load(Ordering::Acquire))
}

/// Publish leaves into Cell `tid`'s domain for `bdf`: reserve a DID, break
/// before make on a requester changing domains, run `write` on the domain's
/// SLPT, then write the context entry and invalidate. `fits` runs first, on an
/// existing domain only, and refuses before any change. `iova` is reported on
/// success.
fn publish_for_cell(
    tid: u64,
    bdf: u32,
    iova: u64,
    fits: impl Fn(&VtdSlpt) -> bool,
    write: impl FnOnce(&VtdSlpt),
) -> DmaMapResult {
    let (canonical, segment) = split_bdf(bdf);
    let bus = (canonical >> 8) as u8;

    let units = UNITS.lock();
    let Some(u_idx) = unit_index_for_bdf(&units, canonical, segment) else {
        log::warn!("[vtd] no unit covers bdf {:04x}:{:04x}", segment, canonical);
        return DmaMapResult::Rejected;
    };
    let u = &units[u_idx];

    let Some(ctx_virt) = u.ensure_context_table(bus) else {
        return DmaMapResult::Rejected;
    };

    let mut domains = u.domains.lock();
    if domains.get(&tid).is_some_and(|d| !fits(&d.slpt)) {
        log::warn!(
            "[vtd] Cell {} DMA map refused: window already holds a translation",
            tid
        );
        return DmaMapResult::Rejected;
    }
    // Reserve a bounded DID before any descriptor is touched: exhaustion must
    // refuse the map instead of issuing a colliding DID.
    let new_did = if domains.contains_key(&tid) {
        None
    } else {
        let Some(did) = u.dids.lock().allocate() else {
            log::warn!(
                "[vtd] Cell {} DMA map refused: DIDs exhausted on unit {:#x}",
                tid,
                u.base
            );
            return DmaMapResult::Rejected;
        };
        Some(did)
    };

    let slot = context_slot(ctx_virt, canonical);
    // SAFETY: the slot lies in this bus's allocated context page; DOMAINS
    // serializes every context mutator of this unit.
    let current = unsafe { read_ctx_slot(slot) };
    let owned_here = domains
        .get(&tid)
        .is_some_and(|d| current == context_entry(d.slpt.root_phys(), d.did));
    if !owned_here {
        // Break before make: a requester changing domains must not keep the
        // previous DID's cached context or translations.
        // SAFETY: as above.
        unsafe { clear_ctx_slot(slot) };
        u.sync(slot, 16);
        fence(Ordering::SeqCst);
        let context_done = u.invalidate_context(Scope::Global);
        let iotlb_done = u.invalidate_iotlb(Scope::Global);
        if !(context_done && iotlb_done) {
            if let Some(did) = new_did {
                u.dids.lock().release(did);
            }
            return DmaMapResult::Rejected;
        }
        // A requester ID names one domain; later teardown must stay exact.
        for (&owner, domain) in domains.iter_mut() {
            if owner != tid {
                domain.bdfs.remove(&canonical);
            }
        }
    }

    if let Some(did) = new_did {
        let coherent = u.ecap & cache::ECAP_C != 0;
        domains.insert(tid, VtdDomain::new(VtdSlpt::new(coherent), did));
    }
    let Some(domain) = domains.get_mut(&tid) else {
        return DmaMapResult::Rejected;
    };
    if domain.bdfs.insert(canonical) {
        apply_rmrr_to_slpt(&domain.slpt, canonical, segment);
    }
    write(&domain.slpt);
    let did = domain.did;
    let slpt_root = domain.slpt.root_phys();

    if !owned_here {
        // SAFETY: the slot was cleared and drained above.
        unsafe { write_ctx_entry(ctx_virt, canonical, slpt_root, did) };
        u.sync(slot, 16);
    }
    fence(Ordering::SeqCst);

    // Global context invalidation: caching-mode hardware may hold the
    // not-present entry under a DID other than the new one.
    let context_done = u.invalidate_context(Scope::Global);
    let iotlb_done = u.invalidate_iotlb(Scope::Domain(did));
    if !(context_done && iotlb_done) {
        return DmaMapResult::PublishedUnconfirmed;
    }
    // warn, not info: this line is the only record of which requester may DMA
    // into which domain, and the iommu-dmar-x86 lane checks its `domains=`
    // count to prove per-Cell domains are live (info-level lines at this site
    // never reached the serial log on the Tier-3 lane).
    log::warn!(
        "[vtd] Cell {} BDF {:02x}:{:02x}.{} DID={} SLPT={:#x} iova={:#x} domains={}",
        tid,
        bus,
        (canonical >> 3) & 0x1f,
        canonical & 0x7,
        did,
        slpt_root,
        iova,
        domains.len()
    );
    DmaMapResult::Mapped(iova)
}

/// Clear Cell `tid`'s requester entries on every unit, then invalidate its
/// domain. A unit whose invalidation times out keeps the domain (and its DID)
/// so teardown can be retried; returns whether every unit completed.
fn unmap_cell_domain(tid: u64) -> bool {
    let units = UNITS.lock();
    let mut all_ok = true;

    for u in units.iter() {
        let mut domains = u.domains.lock();
        let Some(domain) = domains.remove(&tid) else {
            continue;
        };
        let did = domain.did;
        {
            let contexts = u.contexts.lock();
            for &bdf in domain.bdfs.iter() {
                let Some(ctx) = contexts.get(&((bdf >> 8) as u8)) else {
                    continue;
                };
                let slot = context_slot(ctx.virt, bdf);
                // Never clear a slot since published for another domain.
                // SAFETY: slot is in an allocated context page.
                let [lo, hi] = unsafe { read_ctx_slot(slot) };
                if lo & context::CTX_PRESENT != 0 && ((hi >> 8) & 0xFFFF) as u16 == did {
                    // SAFETY: as above.
                    unsafe { clear_ctx_slot(slot) };
                    u.sync(slot, 16);
                }
            }
        }
        fence(Ordering::SeqCst);
        let c_ok = u.invalidate_context(Scope::Domain(did));
        // Drain the IOTLB even when the context invalidation timed out.
        let i_ok = u.invalidate_iotlb(Scope::Domain(did));
        if c_ok && i_ok {
            u.dids.lock().release(did);
        } else {
            log::warn!(
                "[vtd] Cell {} DID={} teardown invalidation incomplete on unit {:#x}; domain retained",
                tid,
                did,
                u.base
            );
            domains.insert(tid, domain);
            all_ok = false;
        }
    }
    all_ok
}

/// Enable DMA translation on every unit, then interrupt remapping where the
/// unit and platform support it (see [`remap`] for the ordering). Queued
/// invalidation goes on first: interrupt remapping needs it, and the same
/// mechanism must then serve every later context/IOTLB invalidation.
fn activate() {
    let mut units = UNITS.lock();
    if units.is_empty() {
        return;
    }

    let mut any_active = false;
    for u in units.iter_mut() {
        u.enable_qi();
        any_active |= u.activate_translation();
    }
    if any_active {
        crate::dev::iommu::set_active();
    }
    // Remappable-format MSIs are only meaningful once a unit decodes them.
    ir::set_ir_active(remap::activate(&mut units));
}

/// How an MSI block from one PCI function must be addressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsiRemap {
    /// The covering unit does not remap interrupts: compatibility format.
    Compatible,
    /// Remappable format: handle of the first of the block's IRTEs.
    Handle(u16),
}

/// Install IRTEs delivering vectors `first_vector..first_vector+count` from
/// function `bdf` (segment in bits 31:16) to APIC `dest_apic_id`, at the unit
/// that covers the function, and invalidate the interrupt entry cache for
/// them. `None` when that unit remaps interrupts but cannot hold the entries
/// or the invalidation failed.
pub fn remap_msi(bdf: u32, first_vector: u8, count: usize, dest_apic_id: u8) -> Option<MsiRemap> {
    let (canonical, segment) = split_bdf(bdf);
    let mut units = UNITS.lock();
    let Some(u_idx) = unit_index_for_bdf(&units, canonical, segment) else {
        return Some(MsiRemap::Compatible);
    };
    let unit = &mut units[u_idx];
    let Some(table) = unit.ir_table.as_mut() else {
        return Some(MsiRemap::Compatible);
    };
    let slot = table.install(first_vector, count, dest_apic_id, canonical)?;
    let scope = (count == 1).then_some(slot.index);
    if slot.written && !unit.invalidate_iec(scope) {
        return None;
    }
    Some(MsiRemap::Handle(slot.index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(base: usize, include_pci_all: bool) -> VtdUnit {
        VtdUnit {
            base,
            segment: 0,
            include_pci_all,
            scopes: BoundedVec::new(DeviceScope::ZERO),
            gcap: 0,
            ecap: 0,
            max_did: 256,
            iva_off: 0,
            root_virt: 0,
            root_phys: 0,
            contexts: Spinlock::new(alloc::collections::BTreeMap::new()),
            domains: Spinlock::new(alloc::collections::BTreeMap::new()),
            dids: Spinlock::new(domain::DidAllocator::new()),
            ir_supported: false,
            ir_table: None,
            qi: Spinlock::new(None),
            active: core::sync::atomic::AtomicBool::new(false),
        }
    }

    #[test]
    fn multi_unit_scope_lookup() {
        let mut u0 = unit(0x1000, false);
        let mut s0 = DeviceScope::ZERO;
        s0.kind = 1;
        s0.start_bus = 0;
        s0.path_len = 1;
        s0.path[0] = [2, 0]; // 00:02.0
        assert!(u0.scopes.push(s0));

        let units = [u0, unit(0x2000, true)];
        let igpu_bdf = (2 << 3) | 0;
        let nic_bdf = (3 << 3) | 0;

        assert_eq!(unit_index_for_bdf(&units, igpu_bdf, 0), Some(0));
        assert_eq!(unit_index_for_bdf(&units, nic_bdf, 0), Some(1));
        assert_eq!(unit_index_for_bdf(&units, nic_bdf, 1), None);
    }

    #[test]
    fn ioapic_source_comes_from_the_dmar_ioapic_scope() {
        // QEMU q35 publishes I/O APIC 0 as bus 0xF0, path 31.0.
        let mut u1 = unit(0x2000, true);
        let mut scope = DeviceScope::ZERO;
        scope.kind = remap::SCOPE_IOAPIC;
        scope.enumeration_id = 2;
        scope.start_bus = 0xF0;
        scope.path_len = 1;
        scope.path[0] = [0x1F, 0];
        assert!(u1.scopes.push(scope));
        assert_eq!(remap::scope_sid(&scope), Some(0xF0F8));

        let units = [unit(0x1000, false), u1];
        assert_eq!(
            remap::source_for_id(&units, 2),
            Some(remap::IoapicSource {
                id: 2,
                unit: 1,
                sid: 0xF0F8
            })
        );
        assert_eq!(remap::source_for_id(&units, 0), None, "unlisted I/O APIC");
        // An I/O APIC scope never matches a PCI requester.
        assert!(!rmrr::device_matches_scope(0xF0F8, &scope));
    }
}
