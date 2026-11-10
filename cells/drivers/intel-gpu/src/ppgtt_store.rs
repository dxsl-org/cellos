#![allow(dead_code, unused_imports)]
//! Per-client PPGTT backing store and buffer binding implementation.
//!
//! Upstream reference: `drivers/gpu/drm/xe/xe_pt.c`, `xe_vm.c` (MIT).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use intel_gpu_core::ppgtt::{Layout, MapError, Ppgtt, TableStore, ENTRIES};
use intel_gpu_core::pte::{level_span, PatIndex};
use types::{ViError, ViResult};

pub const PAGE_SIZE_4K: u64 = 4096;

/// An in-memory page-table store for 4 KiB, 512-entry tables.
pub struct MemTableStore {
    next_phys: u64,
    pub tables: BTreeMap<u64, [u64; ENTRIES]>,
}

impl MemTableStore {
    pub fn new() -> Self {
        Self {
            next_phys: 0x1000_0000, // Simulated physical base at 256 MiB
            tables: BTreeMap::new(),
        }
    }

    pub fn allocated_count(&self) -> usize {
        self.tables.len()
    }
}

impl TableStore for MemTableStore {
    fn alloc_table(&mut self) -> Option<u64> {
        let pa = self.next_phys;
        self.next_phys = self.next_phys.checked_add(PAGE_SIZE_4K)?;
        self.tables.insert(pa, [0u64; ENTRIES]);
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
}

/// Active virtual mapping metadata inside a client GPU VM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientMapping {
    pub addr: u64,
    pub phys: u64,
    pub range: u64,
    pub writable: bool,
    pub pat: PatIndex,
}

/// A per-client GPU VM owning its multi-level PPGTT page tables.
pub struct ClientVm<S: TableStore> {
    pub vm_id: u32,
    pub ppgtt: Ppgtt,
    pub store: S,
    pub mappings: Vec<ClientMapping>,
}

impl ClientVm<MemTableStore> {
    /// Create a new client VM for Panther Lake (Xe3, 48-bit VA, 4-level walk).
    pub fn new_ptl(vm_id: u32) -> ViResult<Self> {
        let mut store = MemTableStore::new();
        let ppgtt = Ppgtt::new(&mut store, Layout::PTL).map_err(|_| ViError::OutOfMemory)?;

        Ok(Self {
            vm_id,
            ppgtt,
            store,
            mappings: Vec::new(),
        })
    }
}

impl<S: TableStore> ClientVm<S> {
    /// Root page table physical address (used in GuC context descriptor / LRCA).
    pub fn root_table(&self) -> u64 {
        self.ppgtt.root()
    }

    /// Bind a client buffer range into the GPU virtual address space.
    pub fn bind_range(
        &mut self,
        addr: u64,
        phys: u64,
        range: u64,
        writable: bool,
        pat: PatIndex,
    ) -> ViResult<()> {
        if range == 0
            || !range.is_multiple_of(PAGE_SIZE_4K)
            || !addr.is_multiple_of(PAGE_SIZE_4K)
            || !phys.is_multiple_of(PAGE_SIZE_4K)
            || addr.checked_add(range).is_none()
            || phys.checked_add(range).is_none()
        {
            return Err(ViError::InvalidArgument);
        }

        let pages = range / PAGE_SIZE_4K;
        for i in 0..pages {
            let offset = i * PAGE_SIZE_4K;
            let mapped =
                self.ppgtt
                    .map_4k(&mut self.store, addr + offset, phys + offset, pat, writable);
            if let Err(e) = mapped {
                // Undo this call's pages so a refused bind leaves no untracked leaves behind.
                for done in 0..i {
                    let _ = self
                        .ppgtt
                        .unmap_4k(&mut self.store, addr + done * PAGE_SIZE_4K);
                }
                return Err(match e {
                    MapError::BadVirtual => ViError::InvalidArgument,
                    MapError::AlreadyMapped => ViError::AlreadyExists,
                    MapError::OutOfTables => ViError::OutOfMemory,
                    _ => ViError::InvalidInput,
                });
            }
        }

        self.mappings.push(ClientMapping {
            addr,
            phys,
            range,
            writable,
            pat,
        });

        Ok(())
    }

    /// Unbind a mapped range from the GPU virtual address space.
    pub fn unbind_range(&mut self, addr: u64, range: u64) -> ViResult<()> {
        if range == 0 || !range.is_multiple_of(PAGE_SIZE_4K) || !addr.is_multiple_of(PAGE_SIZE_4K) {
            return Err(ViError::InvalidArgument);
        }
        let end = addr.checked_add(range).ok_or(ViError::InvalidArgument)?;

        let pages = range / PAGE_SIZE_4K;
        for i in 0..pages {
            let page_va = addr + i * PAGE_SIZE_4K;
            let _ = self.ppgtt.unmap_4k(&mut self.store, page_va);
        }

        // Keep the metadata in step with the leaves when only part of a binding is removed.
        let mut i = 0;
        while i < self.mappings.len() {
            let m = &mut self.mappings[i];
            let m_end = m.addr + m.range;
            if m_end <= addr || m.addr >= end {
                i += 1;
            } else if m.addr < addr {
                let tail = (m_end > end).then(|| ClientMapping {
                    addr: end,
                    phys: m.phys + (end - m.addr),
                    range: m_end - end,
                    ..m.clone()
                });
                m.range = addr - m.addr;
                self.mappings.extend(tail);
                i += 1;
            } else if m_end > end {
                m.phys += end - m.addr;
                m.range = m_end - end;
                m.addr = end;
                i += 1;
            } else {
                self.mappings.swap_remove(i);
            }
        }
        Ok(())
    }

    /// Translate a GPU virtual address to its backing physical address.
    pub fn translate(&mut self, va: u64) -> ViResult<Option<u64>> {
        self.ppgtt
            .translate(&mut self.store, va)
            .map_err(|_| ViError::InvalidArgument)
    }
}
