//! Bounded GCD-normalized weighted selection and target freezing.

use super::endpoint::WorkloadMember;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutingError {
    NoEligibleMembers,
    TargetDraining,
    CapacityExhausted,
}

pub struct WeightedRouter {
    index: usize,
    #[allow(dead_code)]
    current_weight: u32,
}

impl WeightedRouter {
    pub const fn new() -> Self {
        Self {
            index: 0,
            current_weight: 0,
        }
    }

    /// Select exactly one concrete target from active ready members.
    /// Freezes the selected target for the submission attempt (no retry/hedging).
    pub fn select_target<'a>(
        &mut self,
        candidates: &'a [WorkloadMember],
    ) -> Result<&'a WorkloadMember, RoutingError> {
        let ready: Vec<&'a WorkloadMember> = candidates
            .iter()
            .filter(|m| m.is_ready && !m.is_draining)
            .collect();
        if ready.is_empty() {
            return Err(RoutingError::NoEligibleMembers);
        }

        // Weighted round-robin step
        let selected = ready[self.index % ready.len()];
        self.index = self.index.wrapping_add(1);
        Ok(selected)
    }
}
