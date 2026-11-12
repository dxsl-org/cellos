//! Device-reported geometry checked before the driver trusts it (audit 08 S-3, S-4):
//! the doorbell span inside the claimed BAR0 window and the namespace LBA format.

use types::block_wire::SECTOR_SIZE;

/// First doorbell register (NVMe 1.x §3.1.10).
const REG_DB_BASE: usize = 0x1000;
/// Highest queue id this driver creates: the admin pair (0) and one I/O pair (1).
pub(crate) const MAX_QID: usize = 1;
/// Memory page size programmed in CC.MPS (4 KiB) and the size of the bounce buffer.
pub(crate) const PAGE_SIZE: usize = 4096;

// Every I/O command uses PRP1 only, so its transfer must never cross into a second
// memory page, which would need PRP2: at most one page of 512-byte blocks.
const _: () = assert!(SECTOR_SIZE <= PAGE_SIZE);

/// Doorbell offsets for queues `0..=MAX_QID`, proven to lie inside the BAR0 window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Doorbells {
    stride: usize,
}

impl Doorbells {
    /// `CAP.DSTRD` is device-controlled and sets the stride to `4 << DSTRD`; a large
    /// stride puts the I/O CQ doorbell past the claimed window. `Err` carries the end
    /// offset the controller would need (`usize::MAX` for a DSTRD wider than 4 bits).
    pub(crate) fn new(dstrd: u32, window_len: usize) -> Result<Self, usize> {
        if dstrd > 0xF {
            return Err(usize::MAX);
        }
        let stride = 4usize << dstrd;
        let end = (2 * MAX_QID + 1)
            .checked_mul(stride)
            .and_then(|span| span.checked_add(REG_DB_BASE + 4))
            .unwrap_or(usize::MAX);
        if end > window_len {
            return Err(end);
        }
        Ok(Self { stride })
    }

    /// Submission-queue tail doorbell of `qid` (`qid <= MAX_QID`).
    pub(crate) fn sq_tail(self, qid: usize) -> usize {
        debug_assert!(qid <= MAX_QID);
        REG_DB_BASE + 2 * qid * self.stride
    }

    /// Completion-queue head doorbell of `qid` (`qid <= MAX_QID`).
    pub(crate) fn cq_head(self, qid: usize) -> usize {
        debug_assert!(qid <= MAX_QID);
        REG_DB_BASE + (2 * qid + 1) * self.stride
    }
}

/// Why namespace 1's current LBA format cannot be served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LbaFormatError {
    /// FLBAS names a format beyond NLBAF.
    Index { index: u8, count: u8 },
    /// LBADS outside the specification's 512 B..=4 KiB range NVM namespaces use.
    InvalidShift(u8),
    /// A valid block size other than the 512-byte wire sector.
    UnsupportedBlock(u32),
    /// Metadata the driver never supplies a buffer for (MPTR is 0).
    Metadata(u16),
}

/// Identify Namespace byte offsets of the fields read here.
pub(crate) const ID_NS_NLBAF: usize = 25;
pub(crate) const ID_NS_FLBAS: usize = 26;
const ID_NS_LBAF0: usize = 128;

/// Index of the current LBA format: FLBAS bits 3:0, plus bits 6:5 as the high bits
/// when the namespace lists more than 16 formats.
pub(crate) fn lba_format_index(flbas: u8) -> u8 {
    (flbas & 0x0F) | (((flbas >> 5) & 0x03) << 4)
}

/// Offset of LBA format descriptor `index` (`index < 64`, so inside one page).
pub(crate) fn lba_format_offset(index: u8) -> usize {
    ID_NS_LBAF0 + index as usize * 4
}

/// Accept only the format the block protocol serves: 512-byte blocks without metadata.
/// `nlbaf` is 0-based; `lbaf` is the descriptor dword (MS 15:0, LBADS 23:16).
pub(crate) fn validate_lba_format(flbas: u8, nlbaf: u8, lbaf: u32) -> Result<(), LbaFormatError> {
    let index = lba_format_index(flbas);
    if index > nlbaf {
        return Err(LbaFormatError::Index {
            index,
            count: nlbaf.saturating_add(1),
        });
    }
    let lbads = ((lbaf >> 16) & 0xFF) as u8;
    if !(9..=12).contains(&lbads) {
        return Err(LbaFormatError::InvalidShift(lbads));
    }
    let block = 1u32 << lbads;
    if block as usize != SECTOR_SIZE {
        return Err(LbaFormatError::UnsupportedBlock(block));
    }
    let metadata = (lbaf & 0xFFFF) as u16;
    if metadata != 0 {
        return Err(LbaFormatError::Metadata(metadata));
    }
    Ok(())
}

/// Whole 512-byte blocks one PRP1-only transfer starting at `iova` can carry
/// before it would cross into the next memory page.
pub(crate) fn blocks_in_one_prp(iova: u64) -> usize {
    (PAGE_SIZE - iova as usize % PAGE_SIZE) / SECTOR_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR0_16K: usize = 0x4000;

    #[test]
    fn doorbells_reject_strides_past_the_window() {
        let db = Doorbells::new(0, BAR0_16K).unwrap();
        assert_eq!((db.sq_tail(0), db.cq_head(0)), (0x1000, 0x1004));
        assert_eq!((db.sq_tail(1), db.cq_head(1)), (0x1008, 0x100C));
        // DSTRD=9: the I/O CQ doorbell ends at 0x1000 + 3*2048 + 4 = 0x2804.
        assert_eq!(Doorbells::new(9, BAR0_16K).unwrap().cq_head(1), 0x2800);
        // DSTRD=10 already ends 4 bytes past 16 KiB; DSTRD=15 is the CAP maximum.
        assert_eq!(Doorbells::new(10, BAR0_16K), Err(0x4004));
        assert!(Doorbells::new(15, BAR0_16K).is_err());
        assert!(Doorbells::new(15, 0x1000 + 3 * (4 << 15) + 4).is_ok());
        assert!(Doorbells::new(u32::MAX, usize::MAX).is_err());
    }

    fn lbaf(metadata: u16, lbads: u8) -> u32 {
        metadata as u32 | (lbads as u32) << 16
    }

    #[test]
    fn lba_format_accepts_only_512_byte_blocks_without_metadata() {
        assert_eq!(validate_lba_format(0, 0, lbaf(0, 9)), Ok(()));
        assert_eq!(
            validate_lba_format(0, 0, lbaf(0, 12)),
            Err(LbaFormatError::UnsupportedBlock(4096))
        );
        // Shifts that would overflow `1 << lbads` are refused before the shift.
        for bad in [0, 8, 13, 31, 32, 255] {
            assert_eq!(
                validate_lba_format(0, 0, lbaf(0, bad)),
                Err(LbaFormatError::InvalidShift(bad))
            );
        }
        assert_eq!(
            validate_lba_format(0x10, 0, lbaf(8, 9)),
            Err(LbaFormatError::Metadata(8))
        );
        assert_eq!(
            validate_lba_format(0, 0, lbaf(8, 9)),
            Err(LbaFormatError::Metadata(8))
        );
    }

    #[test]
    fn lba_format_index_must_be_listed() {
        assert_eq!(
            validate_lba_format(3, 1, lbaf(0, 9)),
            Err(LbaFormatError::Index { index: 3, count: 2 })
        );
        assert_eq!(lba_format_index(0x6F), 63);
        assert!(lba_format_offset(63) + 4 <= PAGE_SIZE);
    }

    #[test]
    fn multi_block_transfers_stop_at_the_page_end() {
        assert_eq!(blocks_in_one_prp(0x1_0000), 8);
        assert_eq!(blocks_in_one_prp(0x1_0000 + 512), 7);
        assert_eq!(blocks_in_one_prp(0x1_0000 + 3584), 1);
        assert_eq!(blocks_in_one_prp(0x1_0000 + 3585), 0);
    }
}
