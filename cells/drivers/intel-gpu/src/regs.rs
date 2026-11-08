#![allow(dead_code)]
//! MMIO register map and abstraction for Intel Xe2/Xe3 integrated GPUs.
//!
//! Upstream reference: `drivers/gpu/drm/xe/regs/xe_gt_regs.h`, `xe_regs.h` (MIT).

use ostd::mmio::MmioRegion;
use types::{ViError, ViResult};

// ── Register Offsets ──────────────────────────────────────────────────────────

/// Forcewake control for GT core.
pub const FORCEWAKE_GT: usize = 0x00A188;
/// Forcewake acknowledgement for GT core.
pub const FORCEWAKE_ACK_GT: usize = 0x00A18C;

/// Graphics, Media, Display ID register (GMD_ID).
pub const GMD_ID: usize = 0x000D8C;
/// Timestamp and reference clock configuration.
pub const RPM_CONFIG0: usize = 0x000D00;

/// Fuse status for L3 and DSS.
pub const MIRROR_FUSE3: usize = 0x009118;
/// Xe2/Xe3 DSS Geometry mask word 1.
pub const XE2_GT_GEOMETRY_DSS_1: usize = 0x009138;
/// Xe2/Xe3 DSS Geometry mask word 2.
pub const XE2_GT_GEOMETRY_DSS_2: usize = 0x00913C;
/// EU enable mask.
pub const XELP_EU_ENABLE: usize = 0x009134;

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
}
