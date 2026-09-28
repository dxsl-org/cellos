//! LRU sector cache for the VFS block stream.
//!
//! Lives entirely inside the VFS cell — one owner, no lock contention.
//! Cache budget is derived from the cell's heap; see `MAX_CACHE_BYTES`.

use crate::block_stream::BlockStream;
use alloc::collections::{BTreeMap, VecDeque};

/// Sector cache budget: a quarter of the cell's heap.
///
/// Each entry costs 512 B of payload plus BTreeMap/VecDeque bookkeeping, so the
/// resident cost is ~15% above `total_bytes`; the rest of the heap has to hold
/// the fatfs `FileSystem`, per-request buffers, and the mount table. The budget
/// used to be a flat 4 MiB — exactly the whole heap — so the cache filled the
/// arena before eviction could start and the cell died with
/// `OOM: cell heap exhausted` after ~1 MiB of distinct sectors (a guest write
/// burst longer than the boot's own I/O). Deriving it from the heap keeps the
/// two in step.
const MAX_CACHE_BYTES: usize = crate::VFS_HEAP_BYTES / 4;

/// Eviction watermark: entries are dropped once the cache reaches this fraction
/// of its budget, which leaves headroom instead of thrashing on the boundary.
const EVICT_NUMERATOR: usize = 9;
const EVICT_DENOMINATOR: usize = 10;

struct CachedSector {
    data: [u8; 512],
}

/// LRU sector cache keyed by sector number.
///
/// Write policy: write-through, and specifically *device first* — a cache entry
/// only ever holds bytes that are already on the device, so eviction can never
/// discard an unwritten sector. The earlier ordering (insert dirty, then flush,
/// and on flush failure leave the entry dirty) made eviction of unflushed data
/// possible; the `debug_assert` that was supposed to catch it is a no-op in
/// release builds, and a persisted guest marker did go missing that way.
pub struct PageCache {
    entries: BTreeMap<u64, CachedSector>,
    lru_order: VecDeque<u64>,
    total_bytes: usize,
    max_bytes: usize,
}

impl PageCache {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            lru_order: VecDeque::new(),
            total_bytes: 0,
            max_bytes: MAX_CACHE_BYTES,
        }
    }

    /// Serve `sector` from cache; fall back to a raw disk read on miss.
    pub fn read_sector(&mut self, dev: &mut BlockStream, sector: u64, buf: &mut [u8; 512]) -> bool {
        // Copy data out so we don't hold a borrow into self.entries past this point.
        let cached = self.entries.get(&sector).map(|e| e.data);
        if let Some(data) = cached {
            buf.copy_from_slice(&data);
            self.touch(sector);
            return true;
        }
        // Cache miss: read from disk, then populate cache.
        if !dev.read_raw_sector(sector, buf) {
            return false;
        }
        self.insert(sector, buf);
        true
    }

    /// Write a full sector: device first, cache second.
    ///
    /// FAT32 has no journal, so write-through is the durability model — and the
    /// order matters as much as the policy: writing the device before the cache
    /// keeps every resident entry clean, which is what makes eviction safe.
    pub fn write_sector(&mut self, dev: &mut BlockStream, sector: u64, data: &[u8; 512]) -> bool {
        if !dev.write_raw_sector(sector, data) {
            return false;
        }
        self.insert(sector, data);
        true
    }

    fn insert(&mut self, sector: u64, data: &[u8; 512]) {
        if let Some(entry) = self.entries.get_mut(&sector) {
            // Update in-place: no eviction, no total_bytes change.
            entry.data.copy_from_slice(data);
            self.touch(sector);
            return;
        }
        while self.total_bytes + 512
            > self.max_bytes * EVICT_NUMERATOR / EVICT_DENOMINATOR
        {
            let Some(lru) = self.lru_order.pop_back() else {
                break;
            };
            if self.entries.remove(&lru).is_some() {
                self.total_bytes -= 512;
            }
        }
        self.entries.insert(sector, CachedSector { data: *data });
        self.lru_order.push_front(sector);
        self.total_bytes += 512;
    }

    fn touch(&mut self, sector: u64) {
        // O(n) scan — acceptable while the cache holds a few thousand sectors.
        self.lru_order.retain(|&s| s != sector);
        self.lru_order.push_front(sector);
    }
}
