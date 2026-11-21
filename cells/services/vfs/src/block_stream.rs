//! fatfs block streams over one block device of the VFS device table.
//!
//! Two types are exported:
//!  - `BlockStream`: raw sector I/O, no caching. Used internally by `PageCache`.
//!  - `CachedBlockStream`: wraps `BlockStream + PageCache`; implement all fatfs
//!    traits. VFS mounts FAT32 through this type.
//!
//! Every sector goes through `blk_router` to the stream's own [`DeviceRef`];
//! a flush flushes only that device.

use crate::blk_router::{blk_flush, blk_read, blk_read_sectors, blk_write, DeviceRef};
use crate::page_cache::{CacheBudget, PageCache, SectorDevice};
use alloc::vec::Vec;
use types::block_wire::{BlockOutcome, MAX_RUN_SECTORS};

const SECTOR_SIZE: u64 = 512;

/// Why a [`CachedBlockStream`] operation failed, as fatfs carries it back to
/// the FAT backend inside `fatfs::Error::Io`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamError {
    /// The device failed, the request lay outside the partition, or nothing
    /// was issued.
    Io,
    /// A write or flush may have reached the medium; it is not replayed.
    Indeterminate,
}

impl StreamError {
    /// The failure of a write or flush that did not complete.
    pub(crate) fn of(outcome: BlockOutcome) -> Result<(), Self> {
        match outcome {
            BlockOutcome::Completed => Ok(()),
            BlockOutcome::NotIssued => Err(Self::Io),
            BlockOutcome::Indeterminate => Err(Self::Indeterminate),
        }
    }
}

impl fatfs::IoError for StreamError {
    fn is_interrupted(&self) -> bool {
        false
    }
    fn new_unexpected_eof_error() -> Self {
        Self::Io
    }
    fn new_write_zero_error() -> Self {
        Self::Io
    }
}

/// Attempts per raw sector operation before the block path is called failed.
const RAW_IO_ATTEMPTS: usize = 3;

pub struct BlockStream {
    /// Device the volume lives on.
    dev: DeviceRef,
    /// Byte position within the volume (partition-relative; LBA 0 = byte 0).
    pos: u64,
    /// Absolute base LBA of this volume's partition, added to the relative
    /// sector before the block syscall so fatfs and the page cache only ever
    /// see partition-relative sectors. `abi::disk::PART_FAT32_BASE_LBA` for the
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

    /// Fill `dst` from partition byte `offset` with multi-sector device reads,
    /// independent of the stream position. `false` when any of the range lies
    /// outside the partition or the device failed [`RAW_IO_ATTEMPTS`] times.
    pub fn read_bytes_at(&self, offset: u64, dst: &mut [u8]) -> bool {
        if dst.is_empty() {
            return true;
        }
        let first = offset / SECTOR_SIZE;
        let skip = (offset % SECTOR_SIZE) as usize;
        let count = (skip + dst.len()).div_ceil(SECTOR_SIZE as usize) as u64;
        let (Some(lba), Some(_)) = (self.abs_lba(first), self.abs_lba(first + count - 1)) else {
            return false;
        };
        let Ok(count) = u32::try_from(count) else {
            return false;
        };
        let aligned = skip == 0 && dst.len().is_multiple_of(SECTOR_SIZE as usize);
        let mut scratch = if aligned {
            Vec::new()
        } else {
            alloc::vec![0u8; count as usize * SECTOR_SIZE as usize]
        };
        let target: &mut [u8] = if aligned { &mut *dst } else { &mut scratch };
        if !(0..RAW_IO_ATTEMPTS).any(|_| blk_read_sectors(self.dev, lba, count, target)) {
            return false;
        }
        if !aligned {
            dst.copy_from_slice(&scratch[skip..skip + dst.len()]);
        }
        true
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
    fn write_raw_sector(&mut self, sector: u64, data: &[u8; 512]) -> BlockOutcome {
        let Some(lba) = self.abs_lba(sector) else {
            return BlockOutcome::NotIssued;
        };
        for _ in 0..RAW_IO_ATTEMPTS {
            match blk_write(self.dev, lba, data) {
                BlockOutcome::NotIssued => continue,
                outcome => return outcome,
            }
        }
        BlockOutcome::NotIssued
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
    /// Read-ahead window for file data (see [`Self::read_bulk`]).
    ahead: ReadAhead,
    /// The sector the last small read came from. fatfs walks a cluster chain
    /// one 4-byte FAT entry per read, and re-walks it from the file's first
    /// cluster on every seek; without this, each entry cost a cache lookup, an
    /// LRU touch and a 512-byte copy. Dropped on any write, like `ahead`.
    last: Option<(u64, [u8; 512])>,
}

/// Fills are whole `OP_READ_RUN`s: the sectors past the request in the last
/// run cost no extra round trip, and reading further ahead would make a random
/// read pay for data nobody asked for.
const RUN_SECTORS: u64 = MAX_RUN_SECTORS as u64;

/// The last bulk read: `data` holds the partition-relative sectors from `first`.
/// Filled from the device and dropped on any write, so it never differs from the
/// medium (the same rule the write-through page cache keeps).
#[derive(Default)]
struct ReadAhead {
    first: u64,
    data: Vec<u8>,
}

impl ReadAhead {
    /// Bytes of `pos` onward that the window holds.
    fn hit(&self, pos: u64) -> Option<&[u8]> {
        let start = self.first * SECTOR_SIZE;
        let offset = usize::try_from(pos.checked_sub(start)?).ok()?;
        self.data.get(offset..).filter(|rest| !rest.is_empty())
    }
}

impl CachedBlockStream {
    /// `budget` is shared by every mount's cache (see `page_cache`).
    pub fn new(dev: DeviceRef, base_lba: u64, sectors: u64, budget: CacheBudget) -> Self {
        Self {
            inner: BlockStream::new(dev, base_lba, sectors),
            cache: PageCache::new(budget),
            ahead: ReadAhead::default(),
            last: None,
        }
    }

    /// File data: fatfs reads a sector-aligned run of whole sectors (up to a
    /// cluster) straight into the caller's buffer. Serve it from the read-ahead
    /// window, refilling it with the requested sectors rounded up to whole
    /// [`RUN_SECTORS`] runs, instead of one device round trip per 512 bytes. The data
    /// bypasses the sector cache, which stays for metadata (FAT, directories).
    fn read_bulk(&mut self, buf: &mut [u8]) -> Result<usize, StreamError> {
        let pos = self.inner.pos;
        if self.ahead.hit(pos).is_none() {
            let wanted = (buf.len() as u64).div_ceil(SECTOR_SIZE);
            self.fill_ahead(pos / SECTOR_SIZE, wanted.next_multiple_of(RUN_SECTORS))?;
        }
        let window = self.ahead.hit(pos).ok_or(StreamError::Io)?;
        let n = window.len().min(buf.len());
        buf[..n].copy_from_slice(&window[..n]);
        self.inner.pos += n as u64;
        Ok(n)
    }

    fn fill_ahead(&mut self, sector: u64, count: u64) -> Result<(), StreamError> {
        self.ahead.data.clear();
        let count = count.min(
            self.inner
                .sectors
                .checked_sub(sector)
                .ok_or(StreamError::Io)?,
        );
        let lba = self.inner.abs_lba(sector).ok_or(StreamError::Io)?;
        // The last sector of the window is inside the partition by `count`.
        self.inner
            .abs_lba(sector + count - 1)
            .ok_or(StreamError::Io)?;
        let mut data = core::mem::take(&mut self.ahead.data);
        data.resize(count as usize * SECTOR_SIZE as usize, 0);
        for _ in 0..RAW_IO_ATTEMPTS {
            if blk_read_sectors(self.inner.dev, lba, count as u32, &mut data) {
                self.ahead = ReadAhead {
                    first: sector,
                    data,
                };
                return Ok(());
            }
        }
        Err(StreamError::Io)
    }
}

impl fatfs::IoBase for CachedBlockStream {
    type Error = StreamError;
}

impl fatfs::Read for CachedBlockStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, StreamError> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.inner.pos.is_multiple_of(SECTOR_SIZE) && buf.len() >= 2 * SECTOR_SIZE as usize {
            return self.read_bulk(buf);
        }
        let sector = self.inner.pos / SECTOR_SIZE;
        let off = (self.inner.pos % SECTOR_SIZE) as usize;
        if self.last.as_ref().map(|(s, _)| *s) != Some(sector) {
            let mut sec = [0u8; 512];
            if !self.cache.read_sector(&mut self.inner, sector, &mut sec) {
                return Err(StreamError::Io);
            }
            self.last = Some((sector, sec));
        }
        let (_, sec) = self.last.as_ref().ok_or(StreamError::Io)?;
        let n = core::cmp::min(SECTOR_SIZE as usize - off, buf.len());
        buf[..n].copy_from_slice(&sec[off..off + n]);
        self.inner.pos += n as u64;
        Ok(n)
    }
}

impl fatfs::Write for CachedBlockStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, StreamError> {
        // Any write may change sectors the read-ahead window or `last` holds.
        self.ahead.data.clear();
        self.last = None;
        let mut written = 0usize;
        while written < buf.len() {
            let sector = self.inner.pos / SECTOR_SIZE;
            let off = (self.inner.pos % SECTOR_SIZE) as usize;
            let chunk = core::cmp::min(buf.len() - written, SECTOR_SIZE as usize - off);

            if off == 0 && chunk == SECTOR_SIZE as usize {
                // Full-sector write: no read-before-write needed.
                let mut full = [0u8; 512];
                full.copy_from_slice(&buf[written..written + 512]);
                StreamError::of(self.cache.write_sector(&mut self.inner, sector, &full))?;
            } else {
                // Partial sector: read-modify-write through cache.
                let mut sec = [0u8; 512];
                if !self.cache.read_sector(&mut self.inner, sector, &mut sec) {
                    return Err(StreamError::Io);
                }
                sec[off..off + chunk].copy_from_slice(&buf[written..written + chunk]);
                StreamError::of(self.cache.write_sector(&mut self.inner, sector, &sec))?;
            }

            written += chunk;
            self.inner.pos += chunk as u64;
        }
        Ok(written)
    }

    fn flush(&mut self) -> Result<(), StreamError> {
        StreamError::of(blk_flush(self.inner.dev))
    }
}

impl fatfs::Seek for CachedBlockStream {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, StreamError> {
        self.inner.seek(pos).map_err(|()| StreamError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::{BlockStream, StreamError};
    use crate::blk_router::DeviceRef;
    use crate::page_cache::SectorDevice;
    use abi::disk::{PART_CELLTBL_BASE_LBA, PART_FAT32_BASE_LBA, PART_FAT32_SECTORS};
    use types::block_wire::BlockOutcome;

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
        assert_eq!(bs.write_raw_sector(u64::MAX, &buf), BlockOutcome::NotIssued);
    }

    #[test]
    fn only_an_indeterminate_outcome_is_an_indeterminate_stream_error() {
        assert_eq!(StreamError::of(BlockOutcome::Completed), Ok(()));
        assert_eq!(
            StreamError::of(BlockOutcome::NotIssued),
            Err(StreamError::Io)
        );
        assert_eq!(
            StreamError::of(BlockOutcome::Indeterminate),
            Err(StreamError::Indeterminate)
        );
    }

    #[test]
    fn the_read_ahead_window_serves_only_the_bytes_it_holds() {
        let ahead = super::ReadAhead {
            first: 10,
            data: alloc::vec![7u8; 4 * 512],
        };
        assert!(ahead.hit(9 * 512).is_none(), "before the window");
        assert_eq!(ahead.hit(10 * 512).map(<[u8]>::len), Some(4 * 512));
        assert_eq!(ahead.hit(13 * 512 + 100).map(<[u8]>::len), Some(412));
        assert!(ahead.hit(14 * 512).is_none(), "past the window");
        assert!(super::ReadAhead::default().hit(0).is_none(), "empty window");
    }

    #[test]
    fn base_plus_sector_overflow_is_refused() {
        let bs = BlockStream::new(DeviceRef::Device(0), u64::MAX - 1, u64::MAX);
        assert_eq!(bs.abs_lba(1), Some(u64::MAX));
        assert_eq!(bs.abs_lba(2), None);
    }
}
