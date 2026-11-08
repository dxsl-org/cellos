//! Intel GT forcewake acquisition and release.
//!
//! Upstream reference: `drivers/gpu/drm/xe/xe_forcewake.c` (MIT).

use crate::regs::{
    RegisterFile, FORCEWAKE_ACK_BIT, FORCEWAKE_ACK_GT, FORCEWAKE_CLEAR, FORCEWAKE_GT, FORCEWAKE_REQ,
};
use types::{ViError, ViResult};

pub const FORCEWAKE_TIMEOUT_TRIES: usize = 50_000;

/// Acquire GT forcewake domain, preventing the GT from entering sleep states.
pub fn forcewake_gt_acquire(regs: &mut impl RegisterFile) -> ViResult<()> {
    regs.write32(FORCEWAKE_GT, FORCEWAKE_REQ)?;
    regs.poll_bit(
        FORCEWAKE_ACK_GT,
        FORCEWAKE_ACK_BIT,
        true,
        FORCEWAKE_TIMEOUT_TRIES,
    )
    .map_err(|_| ViError::IO)
}

/// Release GT forcewake domain, allowing the GT to enter power-saving states.
pub fn forcewake_gt_release(regs: &mut impl RegisterFile) -> ViResult<()> {
    regs.write32(FORCEWAKE_GT, FORCEWAKE_CLEAR)?;
    regs.poll_bit(
        FORCEWAKE_ACK_GT,
        FORCEWAKE_ACK_BIT,
        false,
        FORCEWAKE_TIMEOUT_TRIES,
    )
    .map_err(|_| ViError::IO)
}
