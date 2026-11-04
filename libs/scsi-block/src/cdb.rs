//! Command descriptor block builders (SPC-4 / SBC-3).
//!
//! Every multi-byte CDB field is big-endian. All flag bits (EVPD, FUA, DPO,
//! IMMED, PMI, group number) are zero and CONTROL is zero: the disk layer only
//! needs plain, synchronous semantics.

/// INQUIRY operation code.
pub const OP_INQUIRY: u8 = 0x12;
/// TEST UNIT READY operation code.
pub const OP_TEST_UNIT_READY: u8 = 0x00;
/// REPORT LUNS operation code.
pub const OP_REPORT_LUNS: u8 = 0xA0;
/// SERVICE ACTION IN(16) operation code (carries READ CAPACITY(16)).
pub const OP_SERVICE_ACTION_IN_16: u8 = 0x9E;
/// SERVICE ACTION IN(16) service action selecting READ CAPACITY(16).
pub const SA_READ_CAPACITY_16: u8 = 0x10;
/// READ(16) operation code.
pub const OP_READ_16: u8 = 0x88;
/// WRITE(16) operation code.
pub const OP_WRITE_16: u8 = 0x8A;
/// SYNCHRONIZE CACHE(10) operation code.
pub const OP_SYNCHRONIZE_CACHE_10: u8 = 0x35;

/// Allocation length used for READ CAPACITY(16): the full SBC-3 parameter data.
pub const READ_CAPACITY_16_LEN: usize = 32;

/// Builds a standard INQUIRY(6) CDB (EVPD = 0, page code 0).
///
/// `alloc_len` is the data-in buffer length in bytes; the device returns at
/// most this many bytes.
pub fn inquiry(alloc_len: u16) -> [u8; 6] {
    let len = alloc_len.to_be_bytes();
    [OP_INQUIRY, 0, 0, len[0], len[1], 0]
}

/// Builds a TEST UNIT READY(6) CDB. It carries no data phase.
pub fn test_unit_ready() -> [u8; 6] {
    [OP_TEST_UNIT_READY, 0, 0, 0, 0, 0]
}

/// Builds a REPORT LUNS(12) CDB with SELECT REPORT = 0 (all logical units
/// addressable by the initiator, excluding well-known LUNs).
///
/// `alloc_len` is the data-in buffer length in bytes; SPC-4 requires at
/// least 16.
pub fn report_luns(alloc_len: u32) -> [u8; 12] {
    let len = alloc_len.to_be_bytes();
    [
        OP_REPORT_LUNS,
        0,
        0,
        0,
        0,
        0,
        len[0],
        len[1],
        len[2],
        len[3],
        0,
        0,
    ]
}

/// Builds a READ CAPACITY(16) CDB requesting [`READ_CAPACITY_16_LEN`] bytes
/// (LOGICAL BLOCK ADDRESS = 0, PMI = 0).
pub fn read_capacity_16() -> [u8; 16] {
    let mut cdb = [0u8; 16];
    cdb[0] = OP_SERVICE_ACTION_IN_16;
    cdb[1] = SA_READ_CAPACITY_16;
    cdb[10..14].copy_from_slice(&(READ_CAPACITY_16_LEN as u32).to_be_bytes());
    cdb
}

/// Builds a READ(16) CDB for `blocks` logical blocks starting at `lba`.
///
/// A `blocks` of zero is a valid CDB that transfers nothing.
pub fn read_16(lba: u64, blocks: u32) -> [u8; 16] {
    rw_16(OP_READ_16, lba, blocks)
}

/// Builds a WRITE(16) CDB for `blocks` logical blocks starting at `lba`.
///
/// A `blocks` of zero is a valid CDB that transfers nothing.
pub fn write_16(lba: u64, blocks: u32) -> [u8; 16] {
    rw_16(OP_WRITE_16, lba, blocks)
}

fn rw_16(op: u8, lba: u64, blocks: u32) -> [u8; 16] {
    let mut cdb = [0u8; 16];
    cdb[0] = op;
    cdb[2..10].copy_from_slice(&lba.to_be_bytes());
    cdb[10..14].copy_from_slice(&blocks.to_be_bytes());
    cdb
}

/// Builds a SYNCHRONIZE CACHE(10) CDB covering the whole medium
/// (LBA = 0, NUMBER OF LOGICAL BLOCKS = 0) with IMMED = 0, so GOOD status
/// means the cache is on stable storage.
pub fn synchronize_cache_10() -> [u8; 10] {
    [OP_SYNCHRONIZE_CACHE_10, 0, 0, 0, 0, 0, 0, 0, 0, 0]
}
