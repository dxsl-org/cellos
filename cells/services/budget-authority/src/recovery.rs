//! Crash recovery and fail-closed ledger initialization.

use super::fence::EnforcementFence;
use super::ledger::{ApplicationLedger, ResourceVector};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryStatus {
    Recovered,
    UninitializedStorage,
    CorruptedMedia,
    FenceRegression,
}

pub fn recover_ledger(
    has_valid_genesis: bool,
    is_corrupt: bool,
    stored_fence: u64,
    boot_floor: u64,
    ledger_id: [u8; 32],
    genesis_id: [u8; 32],
    max_vector: ResourceVector,
    ceiling_vector: ResourceVector,
) -> Result<(ApplicationLedger, EnforcementFence), RecoveryStatus> {
    if !has_valid_genesis {
        return Err(RecoveryStatus::UninitializedStorage);
    }
    if is_corrupt {
        return Err(RecoveryStatus::CorruptedMedia);
    }
    if stored_fence < boot_floor {
        return Err(RecoveryStatus::FenceRegression);
    }
    let ledger = ApplicationLedger::new(ledger_id, genesis_id, max_vector, ceiling_vector, stored_fence)
        .map_err(|_| RecoveryStatus::CorruptedMedia)?;
    let fence = EnforcementFence::new(stored_fence);
    Ok((ledger, fence))
}
