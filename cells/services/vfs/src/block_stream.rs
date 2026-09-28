//! fatfs block stream backed by the kernel block-I/O syscalls (ids 500/501).
//!
//! Two types are exported:
//!  - `BlockStream`: raw sector I/O, no caching. Used internally by `PageCache`.
//!  - `CachedBlockStream`: wraps `BlockStream + PageCache`; implement all fatfs
//!    traits. VFS mounts FAT32 through this type.
//!
//! Data-plane routing: on first access, the stream probes the service registry for
//! a registered NVMe Driver Cell (`service::BLOCK_DRIVER`). If one is present,
//! sector I/O goes through IPC (DrvRequest protocol). Otherwise the kernel VirtIO
//! path (`sys_blk_read`/`sys_blk_write`) is used as the fallback.  On QEMU/VirtIO
//! builds the NVMe Driver Cell exits early (no PCIe NVMe device), so `nvme_tid()`
//! always returns `None` there and the VirtIO path is unchanged.

use crate::blk_router::{blk_flush, blk_read, blk_write};
use crate::page_cache::PageCache;

const SECTOR_SIZE: u64 = 512;

/// Attempts per raw sector operation before the block path is called failed.
const RAW_IO_ATTEMPTS: usize = 3;

pub struct BlockStream {
    /// Byte position within the volume (partition-relative; LBA 0 = byte 0).
    pos: u64,
    /// Absolute base LBA of this volume's partition, added to the relative
    /// sector before the block syscall so fatfs and the page cache only ever
    /// see partition-relative sectors. `api::disk::PART_FAT32_BASE_LBA` for the
    /// `/mnt/sd` volume; `PART_CELLSTORE_BASE_LBA` for the `/bin` cell-store.
    base_lba: u64,
}

impl BlockStream {
    pub fn new(base_lba: u64) -> Self {
        Self { pos: 0, base_lba }
    }
}

impl fatfs::IoBase for BlockStream {
    type Error = ();
}

impl fatfs::Read for BlockStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        if buf.is_empty() {
            return Ok(0);
        }
        let sector = self.pos / SECTOR_SIZE;
        let off = (self.pos % SECTOR_SIZE) as usize;
        let mut sec = [0u8; 512];
        if !blk_read(self.base_lba + sector, &mut sec) {
            return Err(());
        }
        let n = core::cmp::min(SECTOR_SIZE as usize - off, buf.len());
        buf[..n].copy_from_slice(&sec[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl fatfs::Write for BlockStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, ()> {
        let mut written = 0usize;
        while written < buf.len() {
            let sector = self.pos / SECTOR_SIZE;
            let off = (self.pos % SECTOR_SIZE) as usize;
            let chunk = core::cmp::min(buf.len() - written, SECTOR_SIZE as usize - off);

            if off == 0 && chunk == SECTOR_SIZE as usize {
                // Full-sector write — no need to read first.
                let mut full = [0u8; 512];
                full.copy_from_slice(&buf[written..written + 512]);
                if !blk_write(self.base_lba + sector, &full) {
                    return Err(());
                }
            } else {
                // Partial sector — read-modify-write.
                let mut sec = [0u8; 512];
                if !blk_read(self.base_lba + sector, &mut sec) {
                    return Err(());
                }
                sec[off..off + chunk].copy_from_slice(&buf[written..written + chunk]);
                if !blk_write(self.base_lba + sector, &sec) {
                    return Err(());
                }
            }

            written += chunk;
            self.pos += chunk as u64;
        }
        Ok(written)
    }

    /// Issue a flush command so prior writes reach the backing disk image.
    fn flush(&mut self) -> Result<(), ()> {
        if blk_flush() {
            Ok(())
        } else {
            Err(())
        }
    }
}

impl fatfs::Seek for BlockStream {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, ()> {
        self.pos = match pos {
            fatfs::SeekFrom::Start(n) => n,
            fatfs::SeekFrom::Current(n) => {
                let result = self.pos as i64 + n;
                if result < 0 {
                    return Err(());
                }
                result as u64
            }
            fatfs::SeekFrom::End(_) => return Err(()),
        };
        Ok(self.pos)
    }
}

impl BlockStream {
    /// Read one 512-byte sector directly from disk, bypassing the page cache.
    /// Called by `PageCache` on a cache miss. `sector` is partition-relative.
    ///
    /// Retried: a single transient failure in the block path used to surface as
    /// a *short read* (fatfs breaks its loop and `read_at` reports fewer bytes),
    /// which the requesting device model then had to fail — a guest I/O error
    /// out of nowhere. Sector reads are idempotent, so absorbing the blip here
    /// is both safe and the narrowest place to do it.
    pub fn read_raw_sector(&mut self, sector: u64, buf: &mut [u8; 512]) -> bool {
        for _ in 0..RAW_IO_ATTEMPTS {
            if blk_read(self.base_lba + sector, buf) {
                return true;
            }
        }
        false
    }

    /// Write one 512-byte sector directly to disk, bypassing the page cache.
    /// Called by `PageCache::write_sector`, which writes through device-first.
    /// `sector` is partition-relative. Retried for the same reason as
    /// [`BlockStream::read_raw_sector`]; a re-write of identical bytes is safe.
    pub fn write_raw_sector(&mut self, sector: u64, data: &[u8; 512]) -> bool {
        for _ in 0..RAW_IO_ATTEMPTS {
            if blk_write(self.base_lba + sector, data) {
                return true;
            }
        }
        false
    }
}

// ── CachedBlockStream ─────────────────────────────────────────────────────────

/// fatfs block stream with an LRU sector cache.
///
/// Replaces `BlockStream` as the fatfs I/O backend. All sector reads are
/// served from `PageCache` on hit; misses fall through to `BlockStream`.
/// Writes use write-through policy (flush on every write) while backed by FAT32.
pub struct CachedBlockStream {
    inner: BlockStream,
    cache: PageCache,
}

impl CachedBlockStream {
    pub fn new(base_lba: u64) -> Self {
        Self {
            inner: BlockStream::new(base_lba),
            cache: PageCache::new(),
        }
    }
}

impl fatfs::IoBase for CachedBlockStream {
    type Error = ();
}

impl fatfs::Read for CachedBlockStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        if buf.is_empty() {
            return Ok(0);
        }
        let sector = self.inner.pos / SECTOR_SIZE;
        let off = (self.inner.pos % SECTOR_SIZE) as usize;
        let mut sec = [0u8; 512];
        if !self.cache.read_sector(&mut self.inner, sector, &mut sec) {
            return Err(());
        }
        let n = core::cmp::min(SECTOR_SIZE as usize - off, buf.len());
        buf[..n].copy_from_slice(&sec[off..off + n]);
        self.inner.pos += n as u64;
        Ok(n)
    }
}

impl fatfs::Write for CachedBlockStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, ()> {
        let mut written = 0usize;
        while written < buf.len() {
            let sector = self.inner.pos / SECTOR_SIZE;
            let off = (self.inner.pos % SECTOR_SIZE) as usize;
            let chunk = core::cmp::min(buf.len() - written, SECTOR_SIZE as usize - off);

            if off == 0 && chunk == SECTOR_SIZE as usize {
                // Full-sector write: no read-before-write needed.
                let mut full = [0u8; 512];
                full.copy_from_slice(&buf[written..written + 512]);
                if !self.cache.write_sector(&mut self.inner, sector, &full) {
                    return Err(());
                }
            } else {
                // Partial sector: read-modify-write through cache.
                let mut sec = [0u8; 512];
                if !self.cache.read_sector(&mut self.inner, sector, &mut sec) {
                    return Err(());
                }
                sec[off..off + chunk].copy_from_slice(&buf[written..written + chunk]);
                if !self.cache.write_sector(&mut self.inner, sector, &sec) {
                    return Err(());
                }
            }

            written += chunk;
            self.inner.pos += chunk as u64;
        }
        Ok(written)
    }

    fn flush(&mut self) -> Result<(), ()> {
        if blk_flush() {
            Ok(())
        } else {
            Err(())
        }
    }
}

impl fatfs::Seek for CachedBlockStream {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, ()> {
        self.inner.seek(pos)
    }
}
