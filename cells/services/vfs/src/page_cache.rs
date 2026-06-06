//! LRU sector cache for the VFS block stream.
//!
//! Lives entirely inside the VFS cell — one owner, no lock contention.
//! Cache budget is a compile-time constant; change and recompile for G2.

use alloc::collections::{BTreeMap, VecDeque};
use crate::block_stream::BlockStream;

/// Sector cache budget — 4 MB = 8192 512-byte sectors.
/// G2 deployments: change this const and recompile. No feature flag needed.
const MAX_CACHE_BYTES: usize = 4 * 1024 * 1024;

struct CachedSector {
    data:  [u8; 512],
    dirty: bool,
}

/// LRU sector cache keyed by sector number.
///
/// Write policy: write-through on FAT32 (no journal) — every `write_sector`
/// flushes dirty entries to disk immediately. Switch to write-back when
/// viFS2 (WAL) becomes the backend.
pub struct PageCache {
    entries:     BTreeMap<u64, CachedSector>,
    lru_order:   VecDeque<u64>,
    total_bytes: usize,
    max_bytes:   usize,
}

impl PageCache {
    pub fn new() -> Self {
        Self {
            entries:     BTreeMap::new(),
            lru_order:   VecDeque::new(),
            total_bytes: 0,
            max_bytes:   MAX_CACHE_BYTES,
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
        self.insert(sector, buf, false);
        true
    }

    /// Write `data` to cache (marked dirty) and flush synchronously.
    ///
    /// Write-through for FAT32: durability on every write avoids silent data
    /// loss since FAT has no journal to recover from a torn write.
    pub fn write_sector(&mut self, dev: &mut BlockStream, sector: u64, data: &[u8; 512]) -> bool {
        self.insert(sector, data, true);
        self.flush_dirty(dev)
    }

    /// Write all dirty entries to disk and clear their dirty flags.
    pub fn flush_dirty(&mut self, dev: &mut BlockStream) -> bool {
        for (&sector, entry) in self.entries.iter_mut() {
            if entry.dirty {
                if !dev.write_raw_sector(sector, &entry.data) {
                    return false;
                }
                entry.dirty = false;
            }
        }
        true
    }

    fn insert(&mut self, sector: u64, data: &[u8; 512], dirty: bool) {
        if self.entries.contains_key(&sector) {
            // Update in-place: no eviction, no total_bytes change.
            let e = self.entries.get_mut(&sector).unwrap();
            e.data.copy_from_slice(data);
            e.dirty |= dirty;
            self.touch(sector);
            return;
        }
        // Evict LRU entries until we have room. Eviction kicks in at 90% capacity
        // (max_bytes * 9/10 ≈ 3.6MB for the default 4MB cache); this leaves headroom
        // to avoid thrashing on the boundary. Effective live size = ~3.6MB, not 4MB.
        while self.total_bytes + 512 > self.max_bytes * 9 / 10 {
            if let Some(lru) = self.lru_order.pop_back() {
                if let Some(e) = self.entries.remove(&lru) {
                    // Write-through invariant: dirty entries are always flushed
                    // in write_sector before we can hit capacity here.
                    debug_assert!(!e.dirty, "dirty eviction without prior flush");
                    self.total_bytes -= 512;
                }
            } else {
                break;
            }
        }
        self.entries.insert(sector, CachedSector { data: *data, dirty });
        self.lru_order.push_front(sector);
        self.total_bytes += 512;
    }

    fn touch(&mut self, sector: u64) {
        // O(n) scan — acceptable for ≤8192 entries (~32 KB of sector numbers).
        self.lru_order.retain(|&s| s != sector);
        self.lru_order.push_front(sector);
    }
}
