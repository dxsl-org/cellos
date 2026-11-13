//! A probed SCSI disk: block I/O on one LUN.

use crate::cdb;
use crate::error::ScsiError;
use crate::exec::{run, DataPhase, ScsiExecutor};
use crate::parse::Capacity;
use crate::probe::ReportedLuns;

/// One direct-access LUN, ready for I/O. Built by [`ScsiDisk::probe`].
///
/// Each `read`/`write`/`flush` issues exactly one command; the layer never
/// retries, so the caller decides what to do with Retry-class sense.
pub struct ScsiDisk<E> {
    pub(crate) exec: E,
    pub(crate) target: u8,
    pub(crate) lun: [u8; 8],
    pub(crate) capacity: Capacity,
    pub(crate) max_blocks: u32,
    pub(crate) luns: ReportedLuns,
}

impl<E: ScsiExecutor> ScsiDisk<E> {
    /// Logical block length in bytes (power of two, at least 512).
    pub fn block_size(&self) -> u32 {
        self.capacity.block_size
    }

    /// Number of logical blocks on the LUN.
    pub fn block_count(&self) -> u64 {
        self.capacity.block_count
    }

    /// Target this disk lives on.
    pub fn target(&self) -> u8 {
        self.target
    }

    /// 8-byte SAM LUN of this disk.
    pub fn lun(&self) -> [u8; 8] {
        self.lun
    }

    /// Most blocks one `read`/`write` may carry.
    pub fn max_transfer_blocks(&self) -> u32 {
        self.max_blocks
    }

    /// LUNs the target reported other than this disk's, in device order:
    /// non-disk LUNs and disks after the first. Empty when REPORT LUNS was
    /// unsupported.
    pub fn other_luns(&self) -> impl Iterator<Item = [u8; 8]> + '_ {
        let chosen = self.lun;
        self.luns
            .as_slice()
            .iter()
            .copied()
            .filter(move |&lun| lun != chosen)
    }

    /// Whether the target reported more than [`crate::MAX_REPORTED_LUNS`]
    /// LUNs, so [`ScsiDisk::other_luns`] is incomplete.
    pub fn luns_truncated(&self) -> bool {
        self.luns.truncated
    }

    /// Returns the executor, ending use of the disk.
    pub fn into_executor(self) -> E {
        self.exec
    }

    /// Reads `buf.len() / block_size()` blocks starting at `lba` into `buf`.
    /// An empty `buf` issues no command.
    ///
    /// # Errors
    /// [`ScsiError::Misaligned`], [`ScsiError::TooLarge`], or
    /// [`ScsiError::OutOfRange`] before any command is issued;
    /// [`ScsiError::ShortTransfer`] when the device moved fewer bytes than
    /// requested (buffer contents then unspecified); otherwise any command
    /// error.
    pub fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), ScsiError<E::Error>> {
        let blocks = self.check(lba, buf.len())?;
        if blocks == 0 {
            return Ok(());
        }
        let expected = buf.len();
        let cdb = cdb::read_16(lba, blocks);
        let moved = run(
            &mut self.exec,
            self.target,
            self.lun,
            &cdb,
            DataPhase::In(buf),
        )?;
        complete(expected, moved)
    }

    /// [`ScsiDisk::read`] for a request that may exceed
    /// [`ScsiDisk::max_transfer_blocks`]: one READ(16) per limit, in order.
    ///
    /// # Errors
    /// As [`ScsiDisk::read`], from the first command that fails; earlier parts
    /// of `buf` are then filled, later ones unspecified.
    pub fn read_split(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), ScsiError<E::Error>> {
        let block_size = self.capacity.block_size as usize;
        // max_blocks >= 1 is guaranteed by probe.
        let mut next = lba;
        for part in buf.chunks_mut(self.max_blocks as usize * block_size) {
            self.read(next, part)?;
            next += (part.len() / block_size) as u64;
        }
        Ok(())
    }

    /// Writes `buf` (`buf.len() / block_size()` blocks) starting at `lba`.
    /// An empty `buf` issues no command. Data may sit in the device's
    /// volatile cache until [`ScsiDisk::flush`].
    ///
    /// # Errors
    /// Same as [`ScsiDisk::read`]; after [`ScsiError::ShortTransfer`] the
    /// target range holds an unspecified mix of old and new data.
    pub fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), ScsiError<E::Error>> {
        let blocks = self.check(lba, buf.len())?;
        if blocks == 0 {
            return Ok(());
        }
        let cdb = cdb::write_16(lba, blocks);
        let moved = run(
            &mut self.exec,
            self.target,
            self.lun,
            &cdb,
            DataPhase::Out(buf),
        )?;
        complete(buf.len(), moved)
    }

    /// Commits the device's volatile write cache for the whole medium.
    ///
    /// # Errors
    /// Any command error ([`ScsiError::Transport`],
    /// [`ScsiError::CheckCondition`], ...).
    pub fn flush(&mut self) -> Result<(), ScsiError<E::Error>> {
        let cdb = cdb::synchronize_cache_10();
        run(&mut self.exec, self.target, self.lun, &cdb, DataPhase::None).map(|_| ())
    }

    /// Validates a request of `len` bytes at `lba`; returns its block count.
    fn check(&self, lba: u64, len: usize) -> Result<u32, ScsiError<E::Error>> {
        let block_size = self.capacity.block_size;
        // block_size >= 512 is guaranteed by parse::read_capacity_16.
        let size = block_size as usize;
        if !len.is_multiple_of(size) {
            return Err(ScsiError::Misaligned { len, block_size });
        }
        let blocks = (len / size) as u64;
        if blocks > u64::from(self.max_blocks) {
            return Err(ScsiError::TooLarge {
                blocks,
                max_blocks: self.max_blocks,
            });
        }
        let block_count = self.capacity.block_count;
        if lba.checked_add(blocks).is_none_or(|end| end > block_count) {
            return Err(ScsiError::OutOfRange {
                lba,
                blocks,
                block_count,
            });
        }
        // blocks <= max_blocks, which is a u32.
        Ok(blocks as u32)
    }
}

fn complete<T>(expected: usize, transferred: usize) -> Result<(), ScsiError<T>> {
    if transferred == expected {
        Ok(())
    } else {
        Err(ScsiError::ShortTransfer {
            expected,
            transferred,
        })
    }
}
