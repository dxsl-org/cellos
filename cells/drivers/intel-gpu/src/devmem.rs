//! Driver-owned device memory: GuC log, ADS, CT buffers, LRCs and page tables.
//!
//! Memory comes from the kernel in large chunks (`GrantAlloc` + `GrantDma`, one pin each)
//! because a Driver Cell holds a small, fixed number of pins (`MAX_PINS_PER_TASK`), and a
//! DMA authorization cannot be undone before the Cell dies (`ostd::dma` has no unmap).
//! Each chunk is mapped once into the GGTT so the GuC can reach any allocation in it;
//! allocations are page runs inside chunks and go back to the pool, zeroed, when freed.
//! This plays the part of `xe_bo_create_pin_map(..., XE_BO_FLAG_GGTT)` in upstream `xe`.

use crate::ggtt::Ggtt;
use crate::regs::RegisterFile;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use intel_gpu_core::pte::PatIndex;
use ostd::dma::DmaBuf;

pub const PAGE: usize = 4096;
/// Pages per chunk: 4 MiB, enough for the largest single object (the 1.1 MiB GuC log).
pub const CHUNK_PAGES: usize = 1024;

/// Memory the device may read and write, addressed by device address (IOVA).
pub trait DmaMemory {
    /// `pages` zeroed, physically contiguous pages the device is authorized to reach.
    /// Returns their device address. The memory is never returned to the kernel.
    fn alloc(&mut self, pages: usize) -> Option<u64>;
    /// Copy `bytes` to device address `iova`; `false` outside any allocation.
    fn write(&mut self, iova: u64, bytes: &[u8]) -> bool;
    /// Copy from device address `iova`; `false` outside any allocation.
    fn read(&self, iova: u64, out: &mut [u8]) -> bool;
}

/// A page run handed out by the [`Pool`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    /// Device (VT-d domain) address of the first byte.
    pub iova: u64,
    /// GGTT address of the first byte.
    pub ggtt: u32,
    pub pages: usize,
}

struct Chunk {
    iova: u64,
    ggtt: u32,
    used: Vec<bool>,
}

/// Page-run allocator over GGTT-mapped chunks.
#[derive(Default)]
pub struct Pool {
    chunks: Vec<Chunk>,
}

impl Pool {
    /// Allocate `pages` zeroed pages, growing the pool by a chunk when no run fits.
    pub fn alloc(
        &mut self,
        mem: &mut impl DmaMemory,
        regs: &mut impl RegisterFile,
        ggtt: &mut Ggtt,
        pages: usize,
    ) -> Option<Region> {
        if pages == 0 {
            return None;
        }
        for chunk in &mut self.chunks {
            if let Some(first) = find_run(&chunk.used, pages) {
                chunk.used[first..first + pages].fill(true);
                return Some(Region {
                    iova: chunk.iova + (first * PAGE) as u64,
                    ggtt: chunk.ggtt + (first * PAGE) as u32,
                    pages,
                });
            }
        }
        let size = pages.max(CHUNK_PAGES);
        let iova = mem.alloc(size)?;
        let ggtt = ggtt.map(regs, iova, size * PAGE, PatIndex::WB).ok()?;
        let ggtt = u32::try_from(ggtt).ok()?;
        let mut used = vec![false; size];
        used[..pages].fill(true);
        self.chunks.push(Chunk { iova, ggtt, used });
        Some(Region { iova, ggtt, pages })
    }

    /// Zero `region` and make its pages available again. The caller guarantees the device
    /// no longer reaches them (tables unlinked and TLB invalidated, contexts deregistered).
    pub fn free(&mut self, mem: &mut impl DmaMemory, region: Region) {
        let Some(chunk) = self
            .chunks
            .iter_mut()
            .find(|c| region.iova >= c.iova && region.iova < c.iova + (c.used.len() * PAGE) as u64)
        else {
            return;
        };
        let zero = [0u8; PAGE];
        for page in 0..region.pages {
            mem.write(region.iova + (page * PAGE) as u64, &zero);
        }
        let first = ((region.iova - chunk.iova) as usize) / PAGE;
        chunk.used[first..first + region.pages].fill(false);
    }

    /// Pages currently handed out.
    #[cfg(test)]
    pub fn used_pages(&self) -> usize {
        self.chunks
            .iter()
            .map(|c| c.used.iter().filter(|&&u| u).count())
            .sum()
    }
}

fn find_run(used: &[bool], pages: usize) -> Option<usize> {
    let mut run = 0;
    for (i, &u) in used.iter().enumerate() {
        run = if u { 0 } else { run + 1 };
        if run == pages {
            return Some(i + 1 - pages);
        }
    }
    None
}

/// [`DmaMemory`] from the kernel: a contiguous Grant per allocation (`GrantAlloc`, zeroed
/// by the kernel), authorized for the GPU's BDF (`GrantDma`).
pub struct KernelDma {
    bdf: u32,
    /// Device address → buffer. In SAS the IOVA equals the CPU address, but the driver
    /// only ever relies on the address `GrantDma` returned.
    buffers: BTreeMap<u64, DmaBuf>,
}

impl KernelDma {
    pub fn new(bdf: u32) -> Self {
        Self {
            bdf,
            buffers: BTreeMap::new(),
        }
    }

    fn locate(&self, iova: u64, len: usize) -> Option<(&DmaBuf, usize)> {
        let (&base, buf) = self.buffers.range(..=iova).next_back()?;
        let offset = (iova - base) as usize;
        (offset.checked_add(len)? <= buf.size()).then_some((buf, offset))
    }
}

impl DmaMemory for KernelDma {
    fn alloc(&mut self, pages: usize) -> Option<u64> {
        let buf = DmaBuf::alloc(pages)?;
        match buf.authorize(self.bdf) {
            Ok(iova) => {
                self.buffers.insert(iova, buf);
                Some(iova)
            }
            Err(_) => {
                // Never authorized for the device, so the frames can go back at once.
                buf.free();
                None
            }
        }
    }

    fn write(&mut self, iova: u64, bytes: &[u8]) -> bool {
        self.locate(iova, bytes.len())
            .is_some_and(|(buf, offset)| buf.write_bytes(offset, bytes))
    }

    fn read(&self, iova: u64, out: &mut [u8]) -> bool {
        self.locate(iova, out.len())
            .is_some_and(|(buf, offset)| buf.read_bytes(offset, out))
    }
}

/// Little-endian dword helpers over device memory.
pub fn write_u32s(mem: &mut impl DmaMemory, iova: u64, words: &[u32]) -> bool {
    let mut bytes = Vec::with_capacity(words.len() * 4);
    for w in words {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    mem.write(iova, &bytes)
}

/// Read one little-endian dword; `None` outside any allocation.
pub fn read_u32(mem: &impl DmaMemory, iova: u64) -> Option<u32> {
    let mut b = [0u8; 4];
    mem.read(iova, &mut b).then_some(u32::from_le_bytes(b))
}
