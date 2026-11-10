//! Intel GT forcewake acquisition and release.
//!
//! Upstream reference: `drivers/gpu/drm/xe/xe_force_wake.c` (MIT).

use crate::regs::{
    RegisterFile, FORCEWAKE_ACK_BIT, FORCEWAKE_ACK_GT, FORCEWAKE_CLEAR, FORCEWAKE_GT, FORCEWAKE_REQ,
};
use types::{ViError, ViResult};

pub const FORCEWAKE_TIMEOUT_TRIES: usize = 50_000;

/// Acquire GT forcewake domain, preventing the GT from entering sleep states.
pub fn forcewake_gt_acquire(regs: &mut impl RegisterFile) -> ViResult<()> {
    regs.write32(FORCEWAKE_GT, FORCEWAKE_REQ)?;
    wait_ack(regs, true)
}

/// Release GT forcewake domain, allowing the GT to enter power-saving states.
pub fn forcewake_gt_release(regs: &mut impl RegisterFile) -> ViResult<()> {
    regs.write32(FORCEWAKE_GT, FORCEWAKE_CLEAR)?;
    wait_ack(regs, false)
}

fn wait_ack(regs: &impl RegisterFile, awake: bool) -> ViResult<()> {
    regs.poll_bit(
        FORCEWAKE_ACK_GT,
        FORCEWAKE_ACK_BIT,
        awake,
        FORCEWAKE_TIMEOUT_TRIES,
    )?;
    // An unpowered or unmapped BAR reads all-ones, which would otherwise pass as an ack.
    if regs.read32(FORCEWAKE_ACK_GT)? == u32::MAX {
        return Err(ViError::IO);
    }
    Ok(())
}
