//! RISC-V IOMMU v1.0.1 command-queue encoders.
//!
//! Field positions follow `iommu_in_memory_queues.adoc`. Each command is two
//! 64-bit words; the commands used here have a zero second word.

const OPCODE_IOTINVAL: u64 = 0x1;
const OPCODE_IOFENCE: u64 = 0x2;
const OPCODE_IODIR: u64 = 0x3;

const PSCID_MASK: u64 = (1 << 20) - 1;
const DID_MASK: u64 = (1 << 24) - 1;
const PPN_MASK: u64 = (1 << 44) - 1;

/// CQB with PPN in bits 53:10 and LOG2SZ-1 in bits 4:0.
pub(crate) const fn encode_cqb(queue_phys: u64, log2_entries: u8) -> u64 {
    let ppn = (queue_phys >> 12) & PPN_MASK;
    (ppn << 10) | ((log2_entries.saturating_sub(1) as u64) & 0x1F)
}

/// IOTINVAL.VMA with PSCV=1 and AV=GV=0.
pub(crate) const fn encode_iotinval_vma(pscid: u32) -> (u64, u64) {
    let word0 = OPCODE_IOTINVAL | (((pscid as u64) & PSCID_MASK) << 12) | (1 << 32);
    (word0, 0)
}

/// IOFENCE.C with AV=WSI=PR=PW=0.
pub(crate) const fn encode_iofence_c() -> (u64, u64) {
    (OPCODE_IOFENCE, 0)
}

/// IODIR.INVAL_DDT with DV=1 and the exact 24-bit device ID.
pub(crate) const fn encode_iodir_inval_ddt(device_id: u32) -> (u64, u64) {
    let word0 = OPCODE_IODIR | (1 << 33) | (((device_id as u64) & DID_MASK) << 40);
    (word0, 0)
}
