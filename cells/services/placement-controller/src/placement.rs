//! Node candidate ranking and deterministic placement selection.

use alloc::vec::Vec;
use core::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateNode {
    pub node_id: [u8; 32],
    /// Current number of replicas of this application placed on this node
    pub app_replicas: usize,
    /// Normalized available headroom (larger is better)
    pub normalized_headroom: u64,
    pub is_eligible: bool,
}

/// Rank candidate destination nodes by:
/// 1. Fewer existing application replicas on node
/// 2. Greater normalized capacity headroom
/// 3. Lexicographical NodeId tie-break
pub fn rank_nodes<'a>(candidates: &'a mut [CandidateNode]) -> Vec<&'a CandidateNode> {
    let mut eligible: Vec<&'a CandidateNode> = candidates.iter().filter(|n| n.is_eligible).collect();
    eligible.sort_by(|a, b| {
        match a.app_replicas.cmp(&b.app_replicas) {
            Ordering::Equal => match b.normalized_headroom.cmp(&a.normalized_headroom) {
                Ordering::Equal => a.node_id.cmp(&b.node_id),
                other => other,
            },
            other => other,
        }
    });
    eligible
}
