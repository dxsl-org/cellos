#![allow(dead_code)]
//! MMIO register map and abstraction for Intel Xe2/Xe3 integrated GPUs.
//!
//! Upstream reference: `drivers/gpu/drm/xe/regs/xe_gt_regs.h`, `xe_regs.h` (MIT).

use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

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

/// Fuse status for L3 and DSS.
pub const MIRROR_FUSE3: usize = 0x009118;
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

/// GuC status register.
pub const GUC_STATUS: usize = 0x00C000;
/// GuC scratch registers.
pub const SOFT_SCRATCH_0: usize = 0x00C180;

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

pub trait RegisterFile {
    fn read32(&self, offset: usize) -> ViResult<u32>;
    fn write32(&mut self, offset: usize, value: u32) -> ViResult<()>;

    /// Poll for a bit to reach the expected state (with bounded spin).
    fn poll_bit(&self, offset: usize, mask: u32, expected: bool, max_tries: usize) -> ViResult<()> {
        for _ in 0..max_tries {
            let val = self.read32(offset)?;
            if (val & mask != 0) == expected {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(ViError::IO)
    }

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
