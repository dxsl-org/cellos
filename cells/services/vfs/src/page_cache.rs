//! LRU sector cache for the VFS block stream.
//!
//! Lives entirely inside the VFS cell — one owner, no lock contention.
//! Every cache draws on one [`CacheBudget`]; see `MAX_CACHE_BYTES`.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};
use types::block_wire::BlockOutcome;

/// Raw partition-relative sector I/O beneath the cache.
pub trait SectorDevice {
    /// Read `sector`; `false` when the device returned no data.
    fn read_raw_sector(&mut self, sector: u64, buf: &mut [u8; 512]) -> bool;
    /// Write `sector`: `Completed` only when the device reported the write
    /// completed; `Indeterminate` when the medium may hold it.
    fn write_raw_sector(&mut self, sector: u64, data: &[u8; 512]) -> BlockOutcome;
}

/// Sector cache budget: a quarter of the cell's heap, for every cache together.
///
/// Each entry costs 512 B of payload plus BTreeMap/VecDeque bookkeeping, so the
/// resident cost is ~15% above `total_bytes`; the rest of the heap has to hold
/// the fatfs `FileSystem`, per-request buffers, and the mount table. The budget
/// used to be a flat 4 MiB — exactly the whole heap — so the cache filled the
/// arena before eviction could start and the cell died with
/// `OOM: cell heap exhausted` after ~1 MiB of distinct sectors (a guest write
/// burst longer than the boot's own I/O). Deriving it from the heap keeps the
/// two in step. It was later per cache, and every FAT mount (`/mnt/sd`, the
/// `/bin` store, each secondary disk) has one, so two attached disks were
/// enough to exceed the heap; [`CacheBudget`] makes it one budget.
const MAX_CACHE_BYTES: usize = crate::VFS_HEAP_BYTES / 4;

/// Eviction watermark: entries are dropped once the cache reaches this fraction
/// of its budget, which leaves headroom instead of thrashing on the boundary.
const EVICT_NUMERATOR: usize = 9;
const EVICT_DENOMINATOR: usize = 10;

struct CachedSector {
    data: [u8; 512],
}

/// Cached bytes across every [`PageCache`] sharing this handle. Clones share
/// the count; the cell is single-threaded, so the atomic only makes it `Send`.
#[derive(Clone, Default)]
pub struct CacheBudget(Arc<AtomicUsize>);

impl CacheBudget {
    pub fn new() -> Self {
        Self::default()
    }

    fn used(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }

    fn take(&self, bytes: usize) {
        self.0.fetch_add(bytes, Ordering::Relaxed);
    }

    fn give(&self, bytes: usize) {
        self.0.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// LRU sector cache keyed by sector number.
///
/// Write policy: write-through, and specifically *device first* — a cache entry
/// only ever holds bytes that are already on the device, so eviction can never
/// discard an unwritten sector. The earlier ordering (insert dirty, then flush,
/// and on flush failure leave the entry dirty) made eviction of unflushed data
/// possible; the `debug_assert` that was supposed to catch it is a no-op in
/// release builds, and a persisted guest marker did go missing that way.
///
/// A cache evicts only its own entries. When the shared budget is spent and it
/// has none left to evict, it serves sectors uncached rather than grow.
pub struct PageCache {
    entries: BTreeMap<u64, CachedSector>,
    lru_order: VecDeque<u64>,
    total_bytes: usize,
    budget: CacheBudget,
}

impl PageCache {
    pub fn new(budget: CacheBudget) -> Self {
        Self {
            entries: BTreeMap::new(),
            lru_order: VecDeque::new(),
            total_bytes: 0,
            budget,
        }
    }

    /// Serve `sector` from cache; fall back to a raw disk read on miss.
    pub fn read_sector(
        &mut self,
        dev: &mut impl SectorDevice,
        sector: u64,
        buf: &mut [u8; 512],
    ) -> bool {
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
    ///
    /// A write that did not complete drops the resident entry: the medium may
    /// hold the old, the new or a torn sector, so the next read must come from
    /// the device rather than from the pre-write image.
    pub fn write_sector(
        &mut self,
        dev: &mut impl SectorDevice,
        sector: u64,
        data: &[u8; 512],
    ) -> BlockOutcome {
        let outcome = dev.write_raw_sector(sector, data);
        if outcome.is_completed() {
            self.insert(sector, data);
        } else {
            self.evict(sector);
        }
        outcome
    }

    fn evict(&mut self, sector: u64) {
        if self.entries.remove(&sector).is_some() {
            self.total_bytes -= 512;
            self.budget.give(512);
            if let Some(pos) = self.lru_order.iter().position(|&s| s == sector) {
                self.lru_order.remove(pos);
            }
        }
    }

    fn insert(&mut self, sector: u64, data: &[u8; 512]) {
        if let Some(entry) = self.entries.get_mut(&sector) {
            // Update in-place: no eviction, no total_bytes change.
            entry.data.copy_from_slice(data);
            self.touch(sector);
            return;
        }
        while self.budget.used() + 512 > MAX_CACHE_BYTES * EVICT_NUMERATOR / EVICT_DENOMINATOR {
            let Some(lru) = self.lru_order.pop_back() else {
                // Nothing of ours left to evict: other caches hold the budget.
                return;
            };
            if self.entries.remove(&lru).is_some() {
                self.total_bytes -= 512;
                self.budget.give(512);
            }
        }
        self.entries.insert(sector, CachedSector { data: *data });
        self.lru_order.push_front(sector);
        self.total_bytes += 512;
        self.budget.take(512);
    }

    fn touch(&mut self, sector: u64) {
        // If the sector is already the most-recently used (front), avoid any scan.
        if self.lru_order.front() == Some(&sector) {
            return;
        }
        if let Some(pos) = self.lru_order.iter().position(|&s| s == sector) {
            self.lru_order.remove(pos);
            self.lru_order.push_front(sector);
        }
    }
}

impl Drop for PageCache {
    fn drop(&mut self) {
        self.budget.give(self.total_bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::{CacheBudget, PageCache, SectorDevice, MAX_CACHE_BYTES};
    use alloc::collections::BTreeMap;
    use types::block_wire::BlockOutcome;

    /// In-memory medium whose writes can be made to fail.
    #[derive(Default)]
    struct Medium {
        sectors: BTreeMap<u64, [u8; 512]>,
        fail_writes: bool,
        reads: usize,
    }

    impl SectorDevice for Medium {
        fn read_raw_sector(&mut self, sector: u64, buf: &mut [u8; 512]) -> bool {
            self.reads += 1;
            *buf = self.sectors.get(&sector).copied().unwrap_or([0; 512]);
            true
        }
        fn write_raw_sector(&mut self, sector: u64, data: &[u8; 512]) -> BlockOutcome {
            if self.fail_writes {
                // The device may have applied the write before failing to report it.
                self.sectors.insert(sector, [0xEE; 512]);
                return BlockOutcome::Indeterminate;
            }
            self.sectors.insert(sector, *data);
            BlockOutcome::Completed
        }
    }

    #[test]
    fn failed_write_evicts_the_resident_sector() {
        let mut cache = PageCache::new(CacheBudget::new());
        let mut medium = Medium::default();
        assert!(cache.write_sector(&mut medium, 7, &[1; 512]).is_completed());
        assert!(cache.write_sector(&mut medium, 8, &[2; 512]).is_completed());
        assert_eq!(cache.total_bytes, 1024);

        medium.fail_writes = true;
        assert_eq!(
            cache.write_sector(&mut medium, 7, &[3; 512]),
            BlockOutcome::Indeterminate
        );
        assert!(!cache.entries.contains_key(&7));
        assert!(!cache.lru_order.contains(&7));
        assert_eq!(cache.total_bytes, 512);

        let mut buf = [0u8; 512];
        let reads = medium.reads;
        assert!(cache.read_sector(&mut medium, 7, &mut buf));
        assert_eq!(
            buf, [0xEE; 512],
            "read after a failed write comes from the medium"
        );
        assert_eq!(medium.reads, reads + 1);
        assert!(cache.read_sector(&mut medium, 8, &mut buf));
        assert_eq!(buf, [2; 512], "other sectors stay resident");
        assert_eq!(medium.reads, reads + 1);
    }

    #[test]
    fn failed_write_of_absent_sector_leaves_cache_unchanged() {
        let mut cache = PageCache::new(CacheBudget::new());
        let mut medium = Medium {
            fail_writes: true,
            ..Medium::default()
        };
        assert!(!cache.write_sector(&mut medium, 3, &[9; 512]).is_completed());
        assert!(cache.entries.is_empty());
        assert!(cache.lru_order.is_empty());
        assert_eq!(cache.total_bytes, 0);
    }

    #[test]
    fn every_mounts_cache_shares_one_budget() {
        let budget = CacheBudget::new();
        let mut caches: alloc::vec::Vec<PageCache> =
            (0..4).map(|_| PageCache::new(budget.clone())).collect();
        let mut medium = Medium::default();
        let mut buf = [0u8; 512];
        // Each cache alone reads more distinct sectors than the whole budget.
        let sectors = (MAX_CACHE_BYTES / 512) as u64 + 64;
        for cache in &mut caches {
            for sector in 0..sectors {
                assert!(cache.read_sector(&mut medium, sector, &mut buf));
            }
        }
        let held: usize = caches.iter().map(|cache| cache.total_bytes).sum();
        assert_eq!(held, budget.used());
        assert!(held <= MAX_CACHE_BYTES, "{held} > {MAX_CACHE_BYTES}");
        // A cache with nothing to evict serves reads uncached, correctly.
        let reads = medium.reads;
        assert!(caches[3].read_sector(&mut medium, 1, &mut buf));
        assert_eq!(medium.reads, reads + 1);
        // Dropping a cache returns its share.
        let first = caches[0].total_bytes;
        drop(caches.remove(0));
        assert_eq!(budget.used(), held - first);
    }
}
