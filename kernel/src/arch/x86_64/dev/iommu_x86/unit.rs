//! VT-d Remapping Hardware Unit (Intel VT-d spec §§10.4, 8.3).

use crate::acpi::{BoundedVec, DeviceScope, DrhdEntry, DRHD_SCOPES_MAX};
use crate::sync::Spinlock;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, Ordering};

use super::cache::sync_for;
use super::context::{
    alloc_table, write_root_entry, ContextTable, TABLE_KIND_CONTEXT, TABLE_KIND_ROOT,
};
use super::domain::{did_limit, DidAllocator, VtdDomain};
use super::gcmd::{issue_gcmd, wait_status};
use super::invalidate::Scope;
use super::ir::table::IrTable;
use super::ir::ECAP_IR;
use super::qi::{InvQueue, ECAP_QI};
use super::rmrr::device_matches_scope;

pub const VTD_GCAP: usize = 0x00;
pub const VTD_ECAP: usize = 0x10;
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
    /// `ECAP.IR` and the platform (DMAR `INTR_REMAP`) allow remapping.
    pub ir_supported: bool,
    /// Present only while `GSTS.IRES` is latched.
    pub ir_table: Option<IrTable>,
    /// The invalidation queue once `GSTS.QIES` latched; register-based
    /// invalidation is used only while this is `None`.
    pub qi: Spinlock<Option<InvQueue>>,
    pub active: AtomicBool,
}

impl VtdUnit {
    pub fn probe(drhd: &DrhdEntry) -> Option<Self> {
        let base = drhd.register_base as usize;
        if base == 0 {
            return None;
        }

        // A register page that cannot be mapped fails this unit: `None` is the
        // same outcome as a unit whose GCAP reads invalid.
        if let Err(error) = crate::memory::paging::map_mmio_x86(base, 0x1000) {
            log::warn!(
                "[vtd] unit at {:#x} skipped: register page not mapped ({error:?})",
                base
            );
            return None;
        }
        // SAFETY: the register page was just identity-mapped.
        let gcap = unsafe { core::ptr::read_volatile((base + VTD_GCAP) as *const u64) };
        if gcap == 0 || gcap == u64::MAX {
            log::warn!("[vtd] unit at {:#x} GCAP={:#x} invalid", base, gcap);
            return None;
        }

        let max_did = did_limit(gcap)?;
        let mut did_alloc = DidAllocator::new();
        did_alloc.set_limit(max_did);

        // SAFETY: as above.
        let ecap = unsafe { core::ptr::read_volatile((base + VTD_ECAP) as *const u64) };
        let iro = ((ecap >> 8) & 0x3FF) as usize;
        let iva_off = iro * 16;
        let ir_supported = ecap & ECAP_IR != 0;

        let (root_virt, root_phys) = alloc_table(TABLE_KIND_ROOT);
        sync_for(ecap, root_virt, 4096);

        let mut scopes = BoundedVec::new(DeviceScope::ZERO);
        for s in drhd.scopes.as_slice() {
            let _ = scopes.push(*s);
        }

        log::info!(
            "[vtd] probed unit {:#x} seg={} all={} ir={} qi={} coherent={}",
            base,
            drhd.segment,
            drhd.include_pci_all(),
            ir_supported,
            ecap & ECAP_QI != 0,
            ecap & super::cache::ECAP_C != 0
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
            qi: Spinlock::new(None),
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

    /// Write `[virt, virt + len)` back to memory if this unit does not snoop.
    pub fn sync(&self, virt: usize, len: usize) {
        sync_for(self.ecap, virt, len);
    }

    /// The bus's context table, published in the root table and globally
    /// invalidated (caching-mode hardware may hold the not-present root).
    /// `None` when that invalidation timed out; the next call retries it.
    pub fn ensure_context_table(&self, bus: u8) -> Option<usize> {
        let mut contexts = self.contexts.lock();
        let table = contexts.entry(bus).or_insert_with(|| {
            let (virt, phys) = alloc_table(TABLE_KIND_CONTEXT);
            sync_for(self.ecap, virt, 4096);
            ContextTable {
                virt,
                phys,
                publication_flushed: false,
            }
        });
        if !table.publication_flushed {
            // SAFETY: both tables are 4 KiB pages allocated by alloc_table().
            unsafe { write_root_entry(self.root_virt, bus, table.phys) };
            self.sync(self.root_virt + bus as usize * 16, 16);
            let context_done = self.invalidate_context(Scope::Global);
            let iotlb_done = self.invalidate_iotlb(Scope::Global);
            if !(context_done && iotlb_done) {
                return None;
            }
            table.publication_flushed = true;
        }
        Some(table.virt)
    }

    /// Turn on queued invalidation when the unit supports it. Without it the
    /// unit keeps register-based invalidation and cannot remap interrupts.
    pub fn enable_qi(&self) {
        if self.ecap & ECAP_QI == 0 {
            return;
        }
        let queue = InvQueue::enable(self.base);
        if queue.is_none() {
            log::warn!(
                "[vtd] unit {:#x} queued invalidation did not enable",
                self.base
            );
        }
        *self.qi.lock() = queue;
    }

    pub fn activate_translation(&mut self) -> bool {
        if self.root_virt == 0 {
            return false;
        }
        let base = self.base;

        // SAFETY: the register page is identity-mapped; root_phys is 4 KiB aligned.
        unsafe {
            core::ptr::write_volatile((base + VTD_RTADDR) as *mut u64, self.root_phys);
        }
        issue_gcmd(base, SRTP);
        if !wait_status(base, SRTP, true, "RTPS") {
            return false;
        }

        // A new root pointer requires global context then IOTLB invalidation
        // before translation is enabled.
        let context_done = self.invalidate_context(Scope::Global);
        let iotlb_done = self.invalidate_iotlb(Scope::Global);
        if !(context_done && iotlb_done) {
            log::warn!("[vtd] unit {:#x} root-table invalidation incomplete", base);
            return false;
        }

        issue_gcmd(base, TE);
        if !wait_status(base, TE, true, "TES") {
            return false;
        }

        self.active.store(true, Ordering::Release);
        log::warn!(
            "[vtd] Intel VT-d: DMA isolation ACTIVE (per-Cell domains, Sv39 SLPT) unit={:#x}",
            base
        );
        true
    }
}
