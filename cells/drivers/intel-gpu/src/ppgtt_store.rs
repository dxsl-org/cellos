//! Per-client PPGTT page tables in driver device memory, and the leaf writes of
//! `VM_BIND`.
//!
//! Upstream reference: `drivers/gpu/drm/xe/xe_pt.c`, `xe_vm.c` (MIT). Tables live in the
//! [`Pool`]; the GPU walks them at their device address, so a table's address is its IOVA.

use crate::devmem::{DmaMemory, Pool, Region};
use crate::ggtt::Ggtt;
use crate::regs::RegisterFile;
use intel_gpu_core::ppgtt::{Layout, MapError, Ppgtt, TableStore};
use intel_gpu_core::pte::PatIndex;
use types::{ViError, ViResult};

pub const PAGE_SIZE_4K: u64 = 4096;

/// [`TableStore`] over the driver's device-memory pool.
pub struct PoolTables<'a, M: DmaMemory, R: RegisterFile> {
    pub pool: &'a mut Pool,
    pub mem: &'a mut M,
    pub regs: &'a mut R,
    pub ggtt: &'a mut Ggtt,
}

impl<M: DmaMemory, R: RegisterFile> TableStore for PoolTables<'_, M, R> {
    fn alloc_table(&mut self) -> Option<u64> {
        self.pool
            .alloc(self.mem, self.regs, self.ggtt, 1)
            .map(|region| region.iova)
    }

    fn read(&self, table: u64, index: usize) -> u64 {
        let mut b = [0u8; 8];
        // A table address is one this store handed out, so the read cannot miss.
        self.mem.read(table + index as u64 * 8, &mut b);
        u64::from_le_bytes(b)
    }

    fn write(&mut self, table: u64, index: usize, value: u64) {
        self.mem
            .write(table + index as u64 * 8, &value.to_le_bytes());
    }

    fn free_table(&mut self, table: u64) {
        self.pool.free(
            self.mem,
            Region {
                iova: table,
                ggtt: 0,
                pages: 1,
            },
        );
    }
}

/// A per-client GPU VM: the root of its multi-level PPGTT.
pub struct ClientVm {
    pub ppgtt: Ppgtt,
}

impl ClientVm {
    /// A new, empty PTL/WCL/LNL address space (48-bit VA, `vm_max_level` 4).
    pub fn new(store: &mut impl TableStore) -> ViResult<Self> {
        let ppgtt = Ppgtt::new(store, Layout::PTL).map_err(|_| ViError::OutOfMemory)?;
        Ok(Self { ppgtt })
    }

    /// Root page table address (the context's `PDP0`).
    pub fn root_table(&self) -> u64 {
        self.ppgtt.root()
    }

    /// Map `[addr, addr + range)` to device addresses `[phys, phys + range)`. On failure
    /// the pages this call mapped are cleared again, so no untracked leaf stays behind.
    pub fn bind_range(
        &mut self,
        store: &mut impl TableStore,
        addr: u64,
        phys: u64,
        range: u64,
        writable: bool,
        pat: PatIndex,
    ) -> ViResult<()> {
        if !phys.is_multiple_of(PAGE_SIZE_4K) || phys.checked_add(range).is_none() {
            return Err(ViError::InvalidArgument);
        }
        self.bind_each(store, addr, range, |ppgtt, store, va, offset| {
            ppgtt.map_4k(store, va, phys + offset, pat, writable)
        })
    }

    /// Map `[addr, addr + range)` to `NULL` leaves.
    pub fn bind_null_range(
        &mut self,
        store: &mut impl TableStore,
        addr: u64,
        range: u64,
        pat: PatIndex,
    ) -> ViResult<()> {
        self.bind_each(store, addr, range, |ppgtt, store, va, _| {
            ppgtt.map_null_4k(store, va, pat)
        })
    }

    fn bind_each<S: TableStore>(
        &mut self,
        store: &mut S,
        addr: u64,
        range: u64,
        mut map: impl FnMut(&mut Ppgtt, &mut S, u64, u64) -> Result<(), MapError>,
    ) -> ViResult<()> {
        if range == 0
            || !range.is_multiple_of(PAGE_SIZE_4K)
            || !addr.is_multiple_of(PAGE_SIZE_4K)
            || addr.checked_add(range).is_none()
        {
            return Err(ViError::InvalidArgument);
        }
        for i in 0..range / PAGE_SIZE_4K {
            let offset = i * PAGE_SIZE_4K;
            if let Err(e) = map(&mut self.ppgtt, store, addr + offset, offset) {
                for done in 0..i {
                    let _ = self.ppgtt.unmap_4k(store, addr + done * PAGE_SIZE_4K);
                }
                return Err(match e {
                    MapError::BadVirtual => ViError::InvalidArgument,
                    MapError::AlreadyMapped => ViError::AlreadyExists,
                    MapError::OutOfTables => ViError::OutOfMemory,
                    _ => ViError::InvalidInput,
                });
            }
        }
        Ok(())
    }

    /// Clear every present leaf in `[addr, addr + range)`; absent ones are skipped. The
    /// caller invalidates the GPU TLBs afterwards.
    pub fn unbind_range(&mut self, store: &mut impl TableStore, addr: u64, range: u64) {
        for i in 0..range / PAGE_SIZE_4K {
            let _ = self.ppgtt.unmap_4k(store, addr + i * PAGE_SIZE_4K);
        }
    }

    /// Translate a GPU virtual address to the device address it reaches.
    #[cfg(test)]
    pub fn translate(&self, store: &mut impl TableStore, va: u64) -> ViResult<Option<u64>> {
        self.ppgtt
            .translate(store, va)
            .map_err(|_| ViError::InvalidArgument)
    }

    /// Free every table. The leaves are cleared and no context points at the root.
    pub fn destroy(self, store: &mut impl TableStore) {
        self.ppgtt.destroy(store);
    }
}

/// An in-memory table store at simulated device addresses, for host tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemTableStore {
    next_phys: u64,
    pub tables: alloc::collections::BTreeMap<u64, [u64; intel_gpu_core::ppgtt::ENTRIES]>,
}

#[cfg(test)]
impl TableStore for MemTableStore {
    fn alloc_table(&mut self) -> Option<u64> {
        let pa = 0x1000_0000 + self.next_phys;
        self.next_phys += PAGE_SIZE_4K;
        self.tables
            .insert(pa, [0u64; intel_gpu_core::ppgtt::ENTRIES]);
        Some(pa)
    }

    fn read(&self, table: u64, index: usize) -> u64 {
        self.tables.get(&table).map(|t| t[index]).unwrap_or(0)
    }

    fn write(&mut self, table: u64, index: usize, value: u64) {
        if let Some(t) = self.tables.get_mut(&table) {
            t[index] = value;
        }
    }

    fn free_table(&mut self, table: u64) {
        self.tables.remove(&table);
    }
}
