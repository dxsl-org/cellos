//! VT-d Remapping Hardware Unit (Intel VT-d spec §§10.4, 8.3).

use crate::acpi::{BoundedVec, DeviceScope, DrhdEntry, DRHD_SCOPES_MAX};
use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, Ordering};

use super::context::{alloc_table, write_root_entry, ContextTable};
use super::domain::{did_limit, DidAllocator, VtdDomain};
use super::invalidate::{
    ctx_global_command, flush_context, flush_iotlb, iotlb_global_command, POLL_MAX,
};
use super::ir::{
    IrTable, DMAR_IRTA_REG, IRTA_EIM, VTD_GCMD_IRE, VTD_GCMD_SIRTP, VTD_GSTS_IRES, VTD_GSTS_IRTPS,
};
use super::rmrr::device_matches_scope;

pub const VTD_GCAP: usize = 0x00;
pub const VTD_ECAP: usize = 0x10;
pub const VTD_GCMD: usize = 0x18;
pub const VTD_GSTS: usize = 0x1C;
pub const VTD_RTADDR: usize = 0x20;

pub const TE: u32 = 1 << 31;
pub const SRTP: u32 = 1 << 30;

pub struct VtdUnit {
    pub base: usize,
    pub segment: u16,
    pub include_pci_all: bool,
    pub scopes: BoundedVec<DeviceScope, DRHD_SCOPES_MAX>,
    pub gcap: u64,
    pub ecap: u64,
    pub max_did: u32,
    pub iva_off: usize,
    pub root_virt: usize,
    pub root_phys: u64,
    pub contexts: Spinlock<BTreeMap<u8, ContextTable>>,
    pub domains: Spinlock<BTreeMap<u64, VtdDomain>>,
    pub dids: Spinlock<DidAllocator>,
    pub ir_supported: bool,
    pub ir_table: Option<IrTable>,
    pub active: AtomicBool,
}

impl VtdUnit {
    pub fn probe(drhd: &DrhdEntry) -> Option<Self> {
        let base = drhd.register_base as usize;
        if base == 0 {
            return None;
        }

        crate::memory::paging::map_mmio_x86(base, 0x1000);
        let gcap = unsafe { core::ptr::read_volatile(base as *const u64) };
        if gcap == 0 || gcap == u64::MAX {
            log::warn!("[vtd] unit at {:#x} GCAP={:#x} invalid", base, gcap);
            return None;
        }

        let max_did = did_limit(gcap)?;
        let mut did_alloc = DidAllocator::new();
        did_alloc.set_limit(max_did);

        let ecap = unsafe { core::ptr::read_volatile((base + VTD_ECAP) as *const u64) };
        let iro = ((ecap >> 8) & 0x3FF) as usize;
        let iva_off = iro * 16;
        let ir_supported = (ecap & (1 << 24)) != 0;

        let (root_virt, root_phys) = alloc_table(12);

        let mut scopes = BoundedVec::new(DeviceScope::ZERO);
        for s in drhd.scopes.as_slice() {
            let _ = scopes.push(*s);
        }

        log::info!(
            "[vtd] probed unit {:#x} seg={} all={} ir={}",
            base,
            drhd.segment,
            drhd.include_pci_all(),
            ir_supported
        );

        Some(Self {
            base,
            segment: drhd.segment,
            include_pci_all: drhd.include_pci_all(),
            scopes,
            gcap,
            ecap,
            max_did,
            iva_off,
            root_virt,
            root_phys,
            contexts: Spinlock::new(BTreeMap::new()),
            domains: Spinlock::new(BTreeMap::new()),
            dids: Spinlock::new(did_alloc),
            ir_supported,
            ir_table: None,
            active: AtomicBool::new(false),
        })
    }

    pub fn covers_device(&self, bdf: u16, segment: u16) -> bool {
        if self.segment != segment {
            return false;
        }
        for scope in self.scopes.as_slice() {
            if device_matches_scope(bdf, scope) {
                return true;
            }
        }
        self.include_pci_all
    }

    pub fn ensure_context_table(&self, bus: u8) -> Option<usize> {
        let mut contexts = self.contexts.lock();
        if let Some(table) = contexts.get(&bus) {
            return Some(table.virt);
        }
        let (virt, phys) = alloc_table(12);
        unsafe { write_root_entry(self.root_virt, bus, phys) };
        contexts.insert(
            bus,
            ContextTable {
                virt,
                phys,
                publication_flushed: false,
            },
        );
        Some(virt)
    }

    pub fn activate_translation(&mut self) -> bool {
        if self.root_virt == 0 {
            return false;
        }
        let base = self.base;

        // Invalidate stale caches before enabling
        flush_context(base, ctx_global_command());
        flush_iotlb(base, self.iva_off, iotlb_global_command());

        // Step 1: Program root table address
        unsafe {
            core::ptr::write_volatile((base + VTD_RTADDR) as *mut u64, self.root_phys);
            core::ptr::write_volatile((base + VTD_GCMD) as *mut u32, SRTP);
        }

        // Wait for RTPS
        let mut n = 0u64;
        while unsafe { core::ptr::read_volatile((base + VTD_GSTS) as *const u32) } & SRTP == 0 {
            n += 1;
            if n >= POLL_MAX {
                log::warn!("[vtd] unit {:#x} GSTS.RTPS timeout", base);
                return false;
            }
            core::hint::spin_loop();
        }

        // Step 2: Enable Translation
        unsafe {
            core::ptr::write_volatile((base + VTD_GCMD) as *mut u32, TE | SRTP);
        }
        n = 0;
        while unsafe { core::ptr::read_volatile((base + VTD_GSTS) as *const u32) } & TE == 0 {
            n += 1;
            if n >= POLL_MAX {
                log::warn!("[vtd] unit {:#x} GSTS.TES timeout", base);
                return false;
            }
            core::hint::spin_loop();
        }

        self.active.store(true, Ordering::Release);
        log::warn!(
            "[vtd] Intel VT-d: DMA isolation ACTIVE (per-Cell domains, Sv39 SLPT) unit={:#x}",
            base
        );
        true
    }

    pub fn activate_ir(&mut self) -> bool {
        if !self.ir_supported {
            return false;
        }
        let base = self.base;
        let ir_table = IrTable::new(7); // 256 entries

        // Program IRTA register: base address | size power | EIM if x2apic
        let val = (ir_table.phys & !0xFFF) | (ir_table.size_power as u64) | IRTA_EIM;
        unsafe {
            core::ptr::write_volatile((base + DMAR_IRTA_REG) as *mut u64, val);
            core::ptr::write_volatile((base + VTD_GCMD) as *mut u32, VTD_GCMD_SIRTP);
        }

        let mut n = 0u64;
        while unsafe { core::ptr::read_volatile((base + VTD_GSTS) as *const u32) } & VTD_GSTS_IRTPS
            == 0
        {
            n += 1;
            if n >= POLL_MAX {
                log::warn!("[vtd] unit {:#x} IRTPS timeout", base);
                return false;
            }
            core::hint::spin_loop();
        }

        // Enable IR (and leave CFI = 0 to block compatibility interrupts)
        unsafe {
            let cur_gcmd = core::ptr::read_volatile((base + VTD_GSTS) as *const u32);
            core::ptr::write_volatile((base + VTD_GCMD) as *mut u32, cur_gcmd | VTD_GCMD_IRE);
        }
        n = 0;
        while unsafe { core::ptr::read_volatile((base + VTD_GSTS) as *const u32) } & VTD_GSTS_IRES
            == 0
        {
            n += 1;
            if n >= POLL_MAX {
                log::warn!("[vtd] unit {:#x} IRES timeout", base);
                return false;
            }
            core::hint::spin_loop();
        }

        self.ir_table = Some(ir_table);
        log::info!("[vtd] unit {:#x} interrupt remapping enabled", base);
        true
    }
}
