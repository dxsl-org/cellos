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

use core::sync::atomic::{AtomicUsize, Ordering};
use crate::page_cache::PageCache;
use ostd::syscall::{
    sys_blk_flush, sys_blk_read, sys_blk_write,
    sys_lookup_service, sys_recv, sys_send, SyscallResult,
};

const SECTOR_SIZE: u64 = 512;

/// Sentinel: not yet probed.
const NOT_PROBED: usize = 0;
/// Sentinel: probed and absent (no PCIe NVMe registered).
const ABSENT: usize = usize::MAX;

/// Cached NVMe Driver Cell TID. NOT_PROBED (0) on first access.
static NVME_TID: AtomicUsize = AtomicUsize::new(NOT_PROBED);

/// Returns the NVMe Driver Cell TID if one has registered, else `None`.
/// Caches the result so only the first call performs the syscall.
fn nvme_tid() -> Option<usize> {
    let cached = NVME_TID.load(Ordering::Relaxed);
    if cached == ABSENT    { return None; }
    if cached != NOT_PROBED { return Some(cached); }

    match sys_lookup_service(api::syscall::service::BLOCK_DRIVER) {
        Some(tid) if tid != 0 => {
            NVME_TID.store(tid, Ordering::Relaxed);
            Some(tid)
        }
        _ => {
            NVME_TID.store(ABSENT, Ordering::Relaxed);
            None
        }
    }
}

/// Read one 512-byte sector from the active block device.
///
/// Routes to the NVMe Driver Cell (DrvRequest IPC) when registered,
/// falls back to the kernel VirtIO path via `sys_blk_read`.
fn blk_read(abs_lba: u64, buf: &mut [u8; 512]) -> bool {
    if let Some(tid) = nvme_tid() {
        // Read request: [op=0 (2B)] [sector (8B)] — 10 bytes total.
        let mut req = [0u8; 10];
        req[0..2].copy_from_slice(&0u16.to_le_bytes());
        req[2..10].copy_from_slice(&abs_lba.to_le_bytes());
        match sys_send(tid, &req) {
            SyscallResult::Err(_) => return false,
            SyscallResult::Ok(_)  => {}
        }
        // Reply: [status (1B)] [data (512B)] = 513 bytes.
        // mask=tid ensures we only accept the NVMe cell's reply even if
        // another cell sends to VFS while we are blocked here.
        let mut reply = [0u8; 513];
        sys_recv(tid, &mut reply);
        if reply[0] != 0 { return false; }
        buf.copy_from_slice(&reply[1..513]);
        true
    } else {
        sys_blk_read(abs_lba, buf)
    }
}

/// Write one 512-byte sector to the active block device.
fn blk_write(abs_lba: u64, data: &[u8; 512]) -> bool {
    if let Some(tid) = nvme_tid() {
        // Write request: [op=1 (2B)] [sector (8B)] [data (512B)] = 522 bytes.
        let mut req = [0u8; 522];
        req[0..2].copy_from_slice(&1u16.to_le_bytes());
        req[2..10].copy_from_slice(&abs_lba.to_le_bytes());
        req[10..522].copy_from_slice(data);
        match sys_send(tid, &req) {
            SyscallResult::Err(_) => return false,
            SyscallResult::Ok(_)  => {}
        }
        let mut reply = [0u8; 1];
        sys_recv(tid, &mut reply);
        reply[0] == 0
    } else {
        sys_blk_write(abs_lba, data)
    }
}

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
        let off    = (self.pos % SECTOR_SIZE) as usize;
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
            let off    = (self.pos % SECTOR_SIZE) as usize;
            let chunk  = core::cmp::min(buf.len() - written, SECTOR_SIZE as usize - off);

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

            written    += chunk;
            self.pos   += chunk as u64;
        }
        Ok(written)
    }

    /// Issue a flush command so prior writes reach the backing disk image.
    fn flush(&mut self) -> Result<(), ()> {
        if sys_blk_flush() { Ok(()) } else { Err(()) }
    }
}

impl fatfs::Seek for BlockStream {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, ()> {
        self.pos = match pos {
            fatfs::SeekFrom::Start(n)   => n,
            fatfs::SeekFrom::Current(n) => {
                let result = self.pos as i64 + n;
                if result < 0 { return Err(()); }
                result as u64
            }
            fatfs::SeekFrom::End(_)     => return Err(()),
        };
        Ok(self.pos)
    }
}

impl BlockStream {
    /// Read one 512-byte sector directly from disk, bypassing the page cache.
    /// Called by `PageCache` on a cache miss. `sector` is partition-relative.
    pub fn read_raw_sector(&mut self, sector: u64, buf: &mut [u8; 512]) -> bool {
        blk_read(self.base_lba + sector, buf)
    }

    /// Write one 512-byte sector directly to disk, bypassing the page cache.
    /// Called by `PageCache::flush_dirty`. `sector` is partition-relative.
    pub fn write_raw_sector(&mut self, sector: u64, data: &[u8; 512]) -> bool {
        blk_write(self.base_lba + sector, data)
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
        let off    = (self.inner.pos % SECTOR_SIZE) as usize;
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
            let off    = (self.inner.pos % SECTOR_SIZE) as usize;
            let chunk  = core::cmp::min(buf.len() - written, SECTOR_SIZE as usize - off);

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

            written        += chunk;
            self.inner.pos += chunk as u64;
        }
        Ok(written)
    }

    fn flush(&mut self) -> Result<(), ()> {
        if sys_blk_flush() { Ok(()) } else { Err(()) }
    }
}

impl fatfs::Seek for CachedBlockStream {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, ()> {
        self.inner.seek(pos)
    }
}
