//! fatfs block streams over one block device of the VFS device table.
//!
//! Two types are exported:
//!  - `BlockStream`: raw sector I/O, no caching. Used internally by `PageCache`.
//!  - `CachedBlockStream`: wraps `BlockStream + PageCache`; implement all fatfs
//!    traits. VFS mounts FAT32 through this type.
//!
//! Every sector goes through `blk_router` to the stream's own [`DeviceRef`];
//! a flush flushes only that device.

use crate::blk_router::{blk_flush, blk_read, blk_write, DeviceRef};
use crate::page_cache::{PageCache, SectorDevice};
use types::block_wire::BlockOutcome;

const SECTOR_SIZE: u64 = 512;

/// Attempts per raw sector operation before the block path is called failed.
const RAW_IO_ATTEMPTS: usize = 3;

pub struct BlockStream {
    /// Device the volume lives on.
    dev: DeviceRef,
    /// Byte position within the volume (partition-relative; LBA 0 = byte 0).
    pos: u64,
    /// Absolute base LBA of this volume's partition, added to the relative
    /// sector before the block syscall so fatfs and the page cache only ever
    /// see partition-relative sectors. `api::disk::PART_FAT32_BASE_LBA` for the
    /// `/mnt/sd` volume; `PART_CELLSTORE_BASE_LBA` for the `/bin` cell-store.
    base_lba: u64,
    /// Partition length in sectors. fatfs derives sector numbers from the
    /// on-disk BPB and FAT chains, which are untrusted: without this bound a
    /// crafted volume reaches the partitions after it (cell table, `/data`,
    /// `/srv`, cell-store).
    sectors: u64,
}

impl BlockStream {
    pub fn new(dev: DeviceRef, base_lba: u64, sectors: u64) -> Self {
        Self {
            dev,
            pos: 0,
            base_lba,
            sectors,
        }
    }

    /// Absolute LBA of partition-relative `sector`, or `None` when it lies
    /// outside the partition. Every device access goes through here.
    fn abs_lba(&self, sector: u64) -> Option<u64> {
        if sector >= self.sectors {
            return None;
        }
        self.base_lba.checked_add(sector)
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
        let lba = self.abs_lba(sector).ok_or(())?;
        if !blk_read(self.dev, lba, &mut sec) {
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
            let lba = self.abs_lba(sector).ok_or(())?;
            if off == 0 && chunk == SECTOR_SIZE as usize {
                // Full-sector write — no need to read first.
                let mut full = [0u8; 512];
                full.copy_from_slice(&buf[written..written + 512]);
                if !blk_write(self.dev, lba, &full).is_completed() {
                    return Err(());
                }
            } else {
                // Partial sector — read-modify-write.
                let mut sec = [0u8; 512];
                if !blk_read(self.dev, lba, &mut sec) {
                    return Err(());
                }
                sec[off..off + chunk].copy_from_slice(&buf[written..written + chunk]);
                if !blk_write(self.dev, lba, &sec).is_completed() {
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
        if blk_flush(self.dev).is_completed() {
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
            fatfs::SeekFrom::Current(n) => self.pos.checked_add_signed(n).ok_or(())?,
            fatfs::SeekFrom::End(_) => return Err(()),
        };
        Ok(self.pos)
    }
}

impl SectorDevice for BlockStream {
    /// Read one 512-byte sector directly from disk, bypassing the page cache.
    /// Called by `PageCache` on a cache miss. `sector` is partition-relative.
    ///
    /// Retried: a single transient failure in the block path used to surface as
    /// a *short read* (fatfs breaks its loop and `read_at` reports fewer bytes),
    /// which the requesting device model then had to fail — a guest I/O error
    /// out of nowhere. Sector reads are idempotent, so absorbing the blip here
    /// is both safe and the narrowest place to do it.
    fn read_raw_sector(&mut self, sector: u64, buf: &mut [u8; 512]) -> bool {
        let Some(lba) = self.abs_lba(sector) else {
            return false;
        };
        for _ in 0..RAW_IO_ATTEMPTS {
            if blk_read(self.dev, lba, buf) {
                return true;
            }
        }
        false
    }

    /// Write one 512-byte sector directly to disk, bypassing the page cache.
    /// Called by `PageCache::write_sector`, which writes through device-first.
    /// `sector` is partition-relative.
    ///
    /// Retried only while the driver proves the write was never issued. An
    /// `Indeterminate` write is not replayed: the device may already hold it
    /// (or part of it), and a replay is a second, unrequested medium write.
    fn write_raw_sector(&mut self, sector: u64, data: &[u8; 512]) -> bool {
        let Some(lba) = self.abs_lba(sector) else {
            return false;
        };
        for _ in 0..RAW_IO_ATTEMPTS {
            match blk_write(self.dev, lba, data) {
                BlockOutcome::Completed => return true,
                BlockOutcome::NotIssued => continue,
                BlockOutcome::Indeterminate => return false,
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
    pub fn new(dev: DeviceRef, base_lba: u64, sectors: u64) -> Self {
        Self {
            inner: BlockStream::new(dev, base_lba, sectors),
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
        if blk_flush(self.inner.dev).is_completed() {
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

#[cfg(test)]
mod tests {
    use super::BlockStream;
    use crate::blk_router::DeviceRef;
    use crate::page_cache::SectorDevice;
    use api::disk::{PART_CELLTBL_BASE_LBA, PART_FAT32_BASE_LBA, PART_FAT32_SECTORS};

    #[test]
    fn sectors_past_the_partition_never_reach_the_device() {
        let mut bs = BlockStream::new(
            DeviceRef::Device(0),
            PART_FAT32_BASE_LBA,
            PART_FAT32_SECTORS,
        );
        assert_eq!(bs.abs_lba(0), Some(PART_FAT32_BASE_LBA));
        assert_eq!(
            bs.abs_lba(PART_FAT32_SECTORS - 1),
            Some(PART_CELLTBL_BASE_LBA - 1)
        );
        // First sector of P2 (cell table) is outside P1.
        assert_eq!(bs.abs_lba(PART_FAT32_SECTORS), None);
        let mut buf = [0u8; 512];
        assert!(!bs.read_raw_sector(PART_FAT32_SECTORS, &mut buf));
        assert!(!bs.write_raw_sector(u64::MAX, &buf));
    }

    #[test]
    fn base_plus_sector_overflow_is_refused() {
        let bs = BlockStream::new(DeviceRef::Device(0), u64::MAX - 1, u64::MAX);
        assert_eq!(bs.abs_lba(1), Some(u64::MAX));
        assert_eq!(bs.abs_lba(2), None);
    }
}
