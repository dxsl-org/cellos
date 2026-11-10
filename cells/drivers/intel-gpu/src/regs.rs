//! MMIO register map and abstraction for Intel Xe2/Xe3 integrated GPUs.
//!
//! Upstream reference: `drivers/gpu/drm/xe/regs/xe_gt_regs.h`, `regs/xe_guc_regs.h`,
//! `xe_mmio.c` (MIT).

use crate::clock::Clock;
use ostd::mmio::MmioRegion;
use types::ViResult;

// ── Register Offsets ──────────────────────────────────────────────────────────

/// Forcewake control for GT core.
pub const FORCEWAKE_GT: usize = 0x00A188;
/// Forcewake acknowledgement for the GT domain. Graphics IP 12.70+ (so every Xe2/Xe3 part)
/// acks at `FORCEWAKE_ACK_GT_MTL`; the legacy 0x130044 ack is never written there.
pub const FORCEWAKE_ACK_GT: usize = 0x000DFC;

/// Graphics, Media, Display ID register (GMD_ID).
pub const GMD_ID: usize = 0x000D8C;
/// Timestamp and reference clock configuration.
pub const RPM_CONFIG0: usize = 0x000D00;

/// Geometry DSS enable, bits 31:0 of the Xe2/Xe3 geometry mask.
pub const XELP_GT_GEOMETRY_DSS_ENABLE: usize = 0x00913C;
/// Xe2/Xe3 geometry DSS mask, bits 63:32.
pub const XE2_GT_GEOMETRY_DSS_1: usize = 0x009150;
/// Xe2/Xe3 geometry DSS mask, bits 95:64.
pub const XE2_GT_GEOMETRY_DSS_2: usize = 0x009154;
/// EU enable mask.
pub const XELP_EU_ENABLE: usize = 0x009134;
/// Compute DSS enable, bits 31:0 of the Xe2/Xe3 compute mask.
pub const XEHP_GT_COMPUTE_DSS_ENABLE: usize = 0x009144;
/// Compute DSS mask, bits 63:32.
pub const XEHPC_GT_COMPUTE_DSS_ENABLE_EXT: usize = 0x009148;
/// Compute DSS mask, bits 95:64.
pub const XE2_GT_COMPUTE_DSS_2: usize = 0x00914C;

/// GT domain reset control (`GDRST`); a domain bit self-clears when its reset is done.
pub const GDRST: usize = 0x00941C;
/// `GRDOM_GUC`: reset the GuC.
pub const GRDOM_GUC: u32 = 1 << 3;

/// `PMINTRMSK` (`regs/xe_gt_regs.h`).
pub const PMINTRMSK: usize = 0x00A168;
/// `ARAT_EXPIRED_INTRMSK`: masks the always-running-timer interrupt the GuC needs.
pub const ARAT_EXPIRED_INTRMSK: u32 = 1 << 9;

// ── Bits and Masks ───────────────────────────────────────────────────────────

/// Masked 32-bit register write: lower 16 bits value, upper 16 bits mask.
#[inline]
pub const fn masked_write(val: u16, mask: u16) -> u32 {
    (val as u32) | ((mask as u32) << 16)
}

/// Request forcewake on GT domain (set bit 0 with mask bit 0).
pub const FORCEWAKE_REQ: u32 = masked_write(1, 1);
/// Release forcewake on GT domain (clear bit 0 with mask bit 0).
pub const FORCEWAKE_CLEAR: u32 = masked_write(0, 1);
/// Forcewake ACK bit in `FORCEWAKE_ACK_GT`.
pub const FORCEWAKE_ACK_BIT: u32 = 1 << 0;

// ── Register File Trait ──────────────────────────────────────────────────────

/// 32-bit MMIO access to the GT registers (BAR0).
pub trait RegisterFile {
    fn read32(&self, offset: usize) -> ViResult<u32>;
    fn write32(&mut self, offset: usize, value: u32) -> ViResult<()>;

    /// 64-bit write (GGTT entries). The default writes the high dword first, so an entry
    /// whose present bit sits in the low dword becomes valid only with its final value.
    fn write64(&mut self, offset: usize, value: u64) -> ViResult<()> {
        self.write32(offset + 4, (value >> 32) as u32)?;
        self.write32(offset, value as u32)
    }
}

impl RegisterFile for MmioRegion {
    #[inline]
    fn read32(&self, offset: usize) -> ViResult<u32> {
        self.read_u32(offset)
    }

    #[inline]
    fn write32(&mut self, offset: usize, value: u32) -> ViResult<()> {
        self.write_u32(offset, value)
    }

    /// One 64-bit store, as upstream's `writeq` to the GGTT.
    #[inline]
    fn write64(&mut self, offset: usize, value: u64) -> ViResult<()> {
        self.write::<u64>(offset, value)
    }
}

/// Why a [`wait32`] ended without a match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitError {
    /// The register never matched within the timeout; the last value read.
    TimedOut(u32),
    /// The register could not be read.
    Mmio,
}

/// Longest pause between two reads (`__XE_MMIO_WAIT_MAX_BACKOFF_100MS`).
pub const WAIT_MAX_BACKOFF_US: u64 = 100_000;

/// `xe_mmio_wait32`: poll `offset` until `value & mask == expected` for at least
/// `timeout_us`, backing off exponentially from 10 µs, with one last read after the
/// deadline. Returns the matching value.
pub fn wait32(
    regs: &impl RegisterFile,
    clock: &mut impl Clock,
    offset: usize,
    mask: u32,
    expected: u32,
    timeout_us: u64,
) -> Result<u32, WaitError> {
    let end = clock.now_us().saturating_add(timeout_us);
    let mut wait = 10u64;
    loop {
        let read = regs.read32(offset).map_err(|_| WaitError::Mmio)?;
        if read & mask == expected {
            return Ok(read);
        }
        let now = clock.now_us();
        if now >= end {
            break;
        }
        clock.relax(wait.min(end - now));
        wait = (wait * 2).min(WAIT_MAX_BACKOFF_US);
    }
    let read = regs.read32(offset).map_err(|_| WaitError::Mmio)?;
    if read & mask == expected {
        Ok(read)
    } else {
        Err(WaitError::TimedOut(read))
    }
}
