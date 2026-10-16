//! Demand evaluation and ceil(demand / T) scaling computation.

use super::policy::AutoscaleDomainPolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleDecision {
    NoChange,
    ScaleUp { desired: usize },
    ScaleDown { desired: usize },
    FrozenIncompleteTelemetry,
}

pub struct ScalingEngine {
    last_decision_ms: u64,
}

impl ScalingEngine {
    pub const fn new() -> Self {
        Self {
            last_decision_ms: 0,
        }
    }

    /// Calculate the desired replica count: ceil(demand / T), clamped to [min_replicas, max_replicas]
    /// and respects cooldown and per-window rate limits.
    pub fn evaluate(
        &mut self,
        policy: &AutoscaleDomainPolicy,
        current_replicas: usize,
        eligible_demand: u32,
        is_telemetry_complete: bool,
        now_ms: u64,
    ) -> ScaleDecision {
        if !is_telemetry_complete {
            return ScaleDecision::FrozenIncompleteTelemetry;
        }

        // Check cooldown
        if now_ms < self.last_decision_ms.saturating_add(policy.cooldown_ms) {
            return ScaleDecision::NoChange;
        }

        // ceil(demand / T)
        let t = policy.target_t as u64;
        let demand = eligible_demand as u64;
        let raw_desired = ((demand + t - 1) / t) as usize;

        // Clamp to [min_replicas, max_replicas]
        let clamped = raw_desired.clamp(policy.min_replicas, policy.max_replicas);

        if clamped > current_replicas {
            let step = (clamped - current_replicas).min(policy.max_change_per_window);
            self.last_decision_ms = now_ms;
            ScaleDecision::ScaleUp {
                desired: current_replicas + step,
            }
        } else if clamped < current_replicas {
            let step = (current_replicas - clamped).min(policy.max_change_per_window);
            self.last_decision_ms = now_ms;
            ScaleDecision::ScaleDown {
                desired: current_replicas - step,
            }
        } else {
            ScaleDecision::NoChange
        }
    }
}
