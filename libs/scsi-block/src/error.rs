//! Error types. Device-supplied input never panics; it lands in one of these.

use crate::sense::Sense;

/// A device response that does not match the SPC/SBC layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// INQUIRY returned fewer than the 5-byte standard header.
    InquiryTooShort(usize),
    /// REPORT LUNS returned fewer than the 8-byte list header.
    ReportLunsTooShort(usize),
    /// READ CAPACITY(16) returned fewer than the 12 bytes holding the
    /// last LBA and block length.
    CapacityTooShort(usize),
    /// READ CAPACITY(16) reported a last LBA of `u64::MAX`, so the block
    /// count does not fit in `u64`.
    CapacityOverflow,
    /// READ CAPACITY(16) reported a logical block length that is not a power
    /// of two of at least 512 bytes.
    BadBlockSize(u32),
}

/// Errors from probing or using a [`crate::ScsiDisk`].
///
/// `T` is the executor's transport error ([`crate::ScsiExecutor::Error`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScsiError<T> {
    /// The executor failed to obtain a SAM status.
    Transport(T),
    /// CHECK CONDITION with decoded sense; use [`Sense::class`] to decide on
    /// a retry.
    CheckCondition(Sense),
    /// CHECK CONDITION without decodable sense data.
    CheckConditionNoSense,
    /// A SAM status other than GOOD or CHECK CONDITION (see
    /// [`crate::status`]).
    Status(u8),
    /// The device reported a residual larger than the data buffer.
    BadResidual {
        /// Data buffer length in bytes.
        len: usize,
        /// Residual the device reported.
        residual: u32,
    },
    /// READ/WRITE transferred fewer bytes than requested.
    ShortTransfer {
        /// Bytes requested.
        expected: usize,
        /// Bytes actually transferred.
        transferred: usize,
    },
    /// Device data-in failed to parse.
    Parse(ParseError),
    /// No reported LUN answered INQUIRY as a connected direct-access disk.
    NoDisk,
    /// TEST UNIT READY still returned Retry-class sense after the
    /// caller-supplied number of retries; carries the last sense.
    NotReady(Sense),
    /// [`crate::ProbeConfig::max_transfer_bytes`] is smaller than one block.
    TransferLimitBelowBlockSize {
        /// Configured limit in bytes.
        max_transfer_bytes: u32,
        /// Device logical block length.
        block_size: u32,
    },
    /// Buffer length is not a multiple of the block size.
    Misaligned {
        /// Buffer length in bytes.
        len: usize,
        /// Device logical block length.
        block_size: u32,
    },
    /// The request extends past the last block.
    OutOfRange {
        /// First block requested.
        lba: u64,
        /// Number of blocks requested.
        blocks: u64,
        /// Device block count.
        block_count: u64,
    },
    /// The request exceeds the per-command transfer limit.
    TooLarge {
        /// Number of blocks requested.
        blocks: u64,
        /// Most blocks one command may carry.
        max_blocks: u32,
    },
}

impl<T> From<ParseError> for ScsiError<T> {
    fn from(err: ParseError) -> Self {
        ScsiError::Parse(err)
    }
}
