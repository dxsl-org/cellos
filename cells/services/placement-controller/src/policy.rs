//! Homogeneous autoscale domain policy configuration.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoscaleDomainPolicy {
    pub domain_id: u32,
    /// Target concurrency per replica (T)
    pub target_t: u32,
    pub min_replicas: usize,
    pub max_replicas: usize,
    pub cooldown_ms: u64,
    pub max_change_per_window: usize,
}

impl AutoscaleDomainPolicy {
    pub fn new(
        domain_id: u32,
        target_t: u32,
        min_replicas: usize,
        max_replicas: usize,
        cooldown_ms: u64,
        max_change_per_window: usize,
    ) -> Self {
        Self {
            domain_id,
            target_t: if target_t == 0 { 1 } else { target_t },
            min_replicas,
            max_replicas: max_replicas.max(min_replicas),
            cooldown_ms,
            max_change_per_window: if max_change_per_window == 0 { 1 } else { max_change_per_window },
        }
    }
}
