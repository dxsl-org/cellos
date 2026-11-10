//! Per-client GPU page tables (PPGTT) with 4 KiB leaves, mirroring the level
//! arithmetic of `xe_pt.c` (`XE_PTE_SHIFT + XE_PDE_SHIFT * level`) and the
//! `va_bits` / `vm_max_level` platform values of `xe_pci.c` (all MIT).
//!
//! Table memory is owned by the caller through [`TableStore`]; this module never
//! allocates. Unmapping leaves empty intermediate tables in place: they are
//! reclaimed by [`Ppgtt::destroy`] with the whole VM, matching the lifetime of a client.

use crate::pte::{self, EncodeError, PatIndex, ADDR_MASK_4K, PAGE_PRESENT, PDE_PS_LEAF};
use alloc::vec::Vec;

pub const ENTRIES: usize = 512;

/// Backing store for 4 KiB, 512-entry tables addressed by physical address.
pub trait TableStore {
    /// A zeroed, 4 KiB-aligned table, or `None` when out of memory.
    fn alloc_table(&mut self) -> Option<u64>;
    fn read(&self, table: u64, index: usize) -> u64;
    fn write(&mut self, table: u64, index: usize, value: u64);
    /// Return a table [`Self::alloc_table`] handed out; no entry of it is referenced.
    fn free_table(&mut self, table: u64);
}

/// Address-space shape of one platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub va_bits: u32,
    /// Level of the root table (`vm_max_level`).
    pub max_level: u32,
    /// Device DMA width (`dma_mask_size`).
    pub dma_bits: u32,
}

impl Layout {
    /// Panther Lake / Wildcat Lake (`ptl_desc`: va_bits 48, vm_max_level 4, dma 46).
    pub const PTL: Self = Self {
        va_bits: 48,
        max_level: 4,
        dma_bits: 46,
    };

    fn is_valid(&self) -> bool {
        (1..=4).contains(&self.max_level)
            && self.va_bits > 12 + 9 * self.max_level - 9
            && self.va_bits <= 12 + 9 * (self.max_level + 1)
            && self.dma_bits <= 52
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapError {
    BadLayout,
    /// Virtual address not 4 KiB aligned or outside `va_bits`.
    BadVirtual,
    Encode(EncodeError),
    /// A leaf (4 KiB or larger) already covers this address.
    AlreadyMapped,
    NotMapped,
    OutOfTables,
}

pub struct Ppgtt {
    layout: Layout,
    root: u64,
}

fn index(va: u64, level: u32) -> usize {
    ((va >> (12 + 9 * level)) as usize) & (ENTRIES - 1)
}

impl Ppgtt {
    pub fn new(store: &mut impl TableStore, layout: Layout) -> Result<Self, MapError> {
        if !layout.is_valid() {
            return Err(MapError::BadLayout);
        }
        let root = store.alloc_table().ok_or(MapError::OutOfTables)?;
        Ok(Self { layout, root })
    }

    /// Physical address of the root table, for the GuC context descriptor.
    pub fn root(&self) -> u64 {
        self.root
    }

    fn check_va(&self, va: u64) -> Result<(), MapError> {
        if !va.is_multiple_of(pte::level_span(0)) || va >> self.layout.va_bits != 0 {
            return Err(MapError::BadVirtual);
        }
        Ok(())
    }

    /// Walk to the level-0 table for `va`, creating tables when `create` is set.
    fn leaf_table(
        &self,
        store: &mut impl TableStore,
        va: u64,
        create: bool,
    ) -> Result<Option<u64>, MapError> {
        let mut table = self.root;
        for level in (1..=self.layout.max_level).rev() {
            let slot = index(va, level);
            let entry = store.read(table, slot);
            if entry & PAGE_PRESENT == 0 {
                if !create {
                    return Ok(None);
                }
                let next = store.alloc_table().ok_or(MapError::OutOfTables)?;
                let pde = pte::encode_table(next, PatIndex::WB, self.layout.dma_bits)
                    .map_err(MapError::Encode)?;
                store.write(table, slot, pde);
                table = next;
            } else if level <= 2 && entry & PDE_PS_LEAF != 0 {
                return Err(MapError::AlreadyMapped);
            } else {
                table = entry & ADDR_MASK_4K;
            }
        }
        Ok(Some(table))
    }

    pub fn map_4k(
        &mut self,
        store: &mut impl TableStore,
        va: u64,
        phys: u64,
        pat: PatIndex,
        writable: bool,
    ) -> Result<(), MapError> {
        let leaf = pte::encode_leaf(phys, 0, pat, writable, self.layout.dma_bits)
            .map_err(MapError::Encode)?;
        self.install_4k(store, va, leaf)
    }

    /// Map `va` to a `NULL` leaf ([`pte::encode_null_leaf`]).
    pub fn map_null_4k(
        &mut self,
        store: &mut impl TableStore,
        va: u64,
        pat: PatIndex,
    ) -> Result<(), MapError> {
        self.install_4k(store, va, pte::encode_null_leaf(pat))
    }

    fn install_4k(
        &mut self,
        store: &mut impl TableStore,
        va: u64,
        leaf: u64,
    ) -> Result<(), MapError> {
        self.check_va(va)?;
        let table = self
            .leaf_table(store, va, true)?
            .expect("created on demand");
        let slot = index(va, 0);
        if store.read(table, slot) & PAGE_PRESENT != 0 {
            return Err(MapError::AlreadyMapped);
        }
        store.write(table, slot, leaf);
        Ok(())
    }

    /// Clear the leaf for `va` and return the physical page it pointed at. The
    /// caller must invalidate the GPU TLB before reusing that page.
    pub fn unmap_4k(&mut self, store: &mut impl TableStore, va: u64) -> Result<u64, MapError> {
        self.check_va(va)?;
        let table = self
            .leaf_table(store, va, false)?
            .ok_or(MapError::NotMapped)?;
        let slot = index(va, 0);
        let entry = store.read(table, slot);
        if entry & PAGE_PRESENT == 0 {
            return Err(MapError::NotMapped);
        }
        store.write(table, slot, 0);
        Ok(entry & ADDR_MASK_4K)
    }

    /// Physical address `va` maps to, if any.
    pub fn translate(&self, store: &mut impl TableStore, va: u64) -> Result<Option<u64>, MapError> {
        let page = va & !(pte::level_span(0) - 1);
        self.check_va(page)?;
        let Some(table) = self.leaf_table(store, page, false)? else {
            return Ok(None);
        };
        let entry = store.read(table, index(page, 0));
        Ok((entry & PAGE_PRESENT != 0).then_some((entry & ADDR_MASK_4K) | (va & 0xfff)))
    }

    /// Free every table of this address space, root last. The caller has cleared the
    /// leaves and invalidated the GPU TLBs, and no context still points at the root.
    pub fn destroy(self, store: &mut impl TableStore) {
        // (table, level) pairs still to visit; a level-1+ entry without the PS bit
        // points at a child table.
        let mut pending = Vec::from([(self.root, self.layout.max_level)]);
        let mut order = Vec::new();
        while let Some((table, level)) = pending.pop() {
            order.push(table);
            if level == 0 {
                continue;
            }
            for slot in 0..ENTRIES {
                let entry = store.read(table, slot);
                let huge_leaf = level <= 2 && entry & PDE_PS_LEAF != 0;
                if entry & PAGE_PRESENT != 0 && !huge_leaf {
                    pending.push((entry & ADDR_MASK_4K, level - 1));
                }
            }
        }
        // Children before parents, so no freed table is ever read again.
        for table in order.into_iter().rev() {
            store.free_table(table);
        }
    }
}

#[cfg(test)]
mod tests;
