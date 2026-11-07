//! Intel VT-d DMA isolation and Interrupt Remapping driver for x86_64.

pub mod context;
pub mod domain;
pub mod invalidate;
pub mod ir;
pub mod rmrr;
pub mod unit;

use crate::acpi::{BoundedVec, DeviceScope, DrhdEntry};
use crate::sync::Spinlock;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::iommu::{DmaIsolationInput, DmaMapResult, DmaUnmapResult};
use super::iommu_pt::VtdSlpt;
use context::{canonical_bdf, clear_ctx_slot, context_slot, write_ctx_entry};
use domain::VtdDomain;
use invalidate::{ctx_domain_command, flush_context, flush_iotlb, iotlb_domain_command};
use rmrr::apply_rmrr_to_slpt;
use unit::VtdUnit;

const VTD_BASE: usize = 0xFED9_0000;
static PRIMARY_BASE: AtomicUsize = AtomicUsize::new(VTD_BASE);

static UNITS: Spinlock<Vec<VtdUnit>> = Spinlock::new(Vec::new());
pub fn is_present() -> bool {
    !UNITS.lock().is_empty()
}

pub fn init_hw(dma: DmaIsolationInput) {
    let mut units = UNITS.lock();
    units.clear();

    if dma.vtd_base != 0 {
        log::info!(
            "[vtd] register base {:#x} from ACPI DMAR (units={} include_pci_all={})",
            dma.vtd_base,
            dma.vtd_units,
            dma.vtd_include_pci_all
        );
        PRIMARY_BASE.store(dma.vtd_base as usize, Ordering::Relaxed);

        crate::acpi::with_topology(|topo| {
            for drhd in topo.drhds.as_slice() {
                if let Some(u) = VtdUnit::probe(drhd) {
                    units.push(u);
                }
            }
        });
    } else if crate::board::selected().soc == cellos_boards::SocId::QemuX86Q35 {
        log::warn!(
            "[vtd] no ACPI DMAR unit; using the board-declared q35 register base {:#x} (fallback)",
            VTD_BASE
        );
        PRIMARY_BASE.store(VTD_BASE, Ordering::Relaxed);
        let fallback_drhd = DrhdEntry {
            flags: 1, // INCLUDE_PCI_ALL
            segment: 0,
            register_base: VTD_BASE as u64,
            scopes: BoundedVec::new(DeviceScope::ZERO),
        };
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

pub fn map_range_for_cell(tid: u64, bdf: u32, phys: u64, size: usize) -> DmaMapResult {
    let bus = ((bdf >> 8) & 0xFF) as u8;
    let dev = ((bdf >> 3) & 0x1F) as u8;
    let func = (bdf & 0x07) as u8;
    let canonical = canonical_bdf(bus, dev, func);
    let segment = ((bdf >> 16) & 0xFFFF) as u16;

    let mut units = UNITS.lock();
    let Some(u_idx) = unit_index_for_bdf(&units, canonical, segment) else {
        log::warn!("[vtd] no unit covers bdf {:04x}:{:04x}", segment, canonical);
        return DmaMapResult::Rejected;
    };
    let u = &mut units[u_idx];

    let Some(ctx_virt) = u.ensure_context_table(bus) else {
        return DmaMapResult::Rejected;
    };

    let mut domains = u.domains.lock();
    let did = if let Some(domain) = domains.get(&tid) {
        domain.did
    } else {
        let Some(new_did) = u.dids.lock().allocate() else {
            log::warn!(
                "[vtd] Cell {} DMA map refused: DIDs exhausted on unit {:#x}",
                tid,
                u.base
            );
            return DmaMapResult::Rejected;
        };
        let slpt = VtdSlpt::new();
        apply_rmrr_to_slpt(&slpt, canonical, segment);
        domains.insert(tid, VtdDomain::new(slpt, new_did));
        new_did
    };

    let domain = domains.get_mut(&tid).unwrap();
    domain.slpt.map_range(phys, size);
    domain.bdfs.insert(canonical);

    unsafe {
        write_ctx_entry(ctx_virt, canonical, domain.slpt.root_phys(), did);
    }
    flush_context(u.base, ctx_domain_command(did));
    flush_iotlb(u.base, u.iva_off, iotlb_domain_command(did));

    DmaMapResult::Mapped(phys)
}

pub fn unmap_cell_domain(tid: u64) -> bool {
    let mut units = UNITS.lock();
    let mut all_ok = true;

    for u in units.iter_mut() {
        let mut domains = u.domains.lock();
        let Some(domain) = domains.remove(&tid) else {
            continue;
        };
        let did = domain.did;
        let contexts = u.contexts.lock();
        for &bdf in domain.bdfs.iter() {
            let bus = ((bdf >> 8) & 0xFF) as u8;
            if let Some(ctx) = contexts.get(&bus) {
                let slot = context_slot(ctx.virt, bdf);
                unsafe { clear_ctx_slot(slot) };
            }
        }
        let c_ok = flush_context(u.base, ctx_domain_command(did));
        let i_ok = flush_iotlb(u.base, u.iva_off, iotlb_domain_command(did));
        if c_ok && i_ok {
            u.dids.lock().release(did);
        } else {
            all_ok = false;
        }
    }
    all_ok
}

pub fn unmap_range_for_cell(tid: u64, iova: u64, size: usize) -> DmaUnmapResult {
    let units = UNITS.lock();
    let mut total_pages = 0;

    for u in units.iter() {
        let domains = u.domains.lock();
        if let Some(domain) = domains.get(&tid) {
            let p = domain.slpt.unmap_range(iova, size);
            total_pages += p;
            flush_iotlb(u.base, u.iva_off, iotlb_domain_command(domain.did));
        }
    }
    if total_pages > 0 {
        DmaUnmapResult::Unmapped { pages: total_pages }
    } else {
        DmaUnmapResult::NothingMapped
    }
}

pub fn activate() {
    let mut units = UNITS.lock();
    if units.is_empty() {
        return;
    }

    let mut any_active = false;
    for u in units.iter_mut() {
        if u.activate_translation() {
            any_active = true;
            // Activate IR if supported
            if u.ir_supported {
                u.activate_ir();
            }
        }
    }

    if any_active {
        super::iommu::set_active();
        ir::set_ir_active(true);
    }
}

pub fn program_ir_entry(vector: u8, dest_apic_id: u32, sid: u16, level: bool) -> Option<u16> {
    if !ir::is_ir_active() {
        return None;
    }
    let idx = ir::alloc_irte_index()?;
    let irte = ir::Irte::encode(vector, dest_apic_id, sid, level);
    let units = UNITS.lock();
    for u in units.iter() {
        if let Some(ir_table) = &u.ir_table {
            ir_table.program_entry(idx, irte);
        }
    }
    Some(idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acpi::{BoundedVec, DeviceScope, DRHD_SCOPES_MAX};

    #[test]
    fn multi_unit_scope_lookup() {
        let mut u0 = VtdUnit {
            base: 0x1000,
            segment: 0,
            include_pci_all: false,
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
            active: core::sync::atomic::AtomicBool::new(false),
        };
        let mut s0 = DeviceScope::ZERO;
        s0.kind = 1;
        s0.start_bus = 0;
        s0.path_len = 1;
        s0.path[0] = [2, 0]; // 00:02.0
        assert!(u0.scopes.push(s0));

        let u1 = VtdUnit {
            base: 0x2000,
            segment: 0,
            include_pci_all: true,
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
            active: core::sync::atomic::AtomicBool::new(false),
        };

        let units = [u0, u1];
        let igpu_bdf = (2 << 3) | 0;
        let nic_bdf = (3 << 3) | 0;

        assert_eq!(unit_index_for_bdf(&units, igpu_bdf, 0), Some(0));
        assert_eq!(unit_index_for_bdf(&units, nic_bdf, 0), Some(1));
        assert_eq!(unit_index_for_bdf(&units, nic_bdf, 1), None);
    }
}
