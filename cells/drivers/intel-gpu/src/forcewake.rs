//! Intel GT forcewake acquisition and release.
//!
//! Upstream reference: `drivers/gpu/drm/xe/xe_force_wake.c` (MIT): `__domain_wait` waits
//! for the ack with `xe_mmio_wait32` for `XE_FORCE_WAKE_ACK_TIMEOUT_MS` (50 ms).

use crate::clock::Clock;
use crate::regs::{
    wait32, RegisterFile, FORCEWAKE_ACK_BIT, FORCEWAKE_ACK_GT, FORCEWAKE_CLEAR, FORCEWAKE_GT,
    FORCEWAKE_REQ,
};
use types::{ViError, ViResult};

/// `XE_FORCE_WAKE_ACK_TIMEOUT_MS` in microseconds.
pub const FORCEWAKE_ACK_TIMEOUT_US: u64 = 50_000;

/// Acquire GT forcewake domain, preventing the GT from entering sleep states.
pub fn forcewake_gt_acquire(regs: &mut impl RegisterFile, clock: &mut impl Clock) -> ViResult<()> {
    regs.write32(FORCEWAKE_GT, FORCEWAKE_REQ)?;
    wait_ack(regs, clock, true)
}

/// Release GT forcewake domain, allowing the GT to enter power-saving states.
pub fn forcewake_gt_release(regs: &mut impl RegisterFile, clock: &mut impl Clock) -> ViResult<()> {
    regs.write32(FORCEWAKE_GT, FORCEWAKE_CLEAR)?;
    wait_ack(regs, clock, false)
}

fn wait_ack(regs: &impl RegisterFile, clock: &mut impl Clock, awake: bool) -> ViResult<()> {
    let expected = if awake { FORCEWAKE_ACK_BIT } else { 0 };
    let ack = wait32(
        regs,
        clock,
        FORCEWAKE_ACK_GT,
        FORCEWAKE_ACK_BIT,
        expected,
        FORCEWAKE_ACK_TIMEOUT_US,
    )
    .map_err(|_| ViError::IO)?;
    // An unpowered or unmapped BAR reads all-ones, which would otherwise pass as an ack.
    if ack == u32::MAX {
        return Err(ViError::IO);
    }
    Ok(())
}
