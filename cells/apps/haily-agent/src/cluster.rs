//! Multi-Node C2C Cluster Coordination, Task Placement, and Fault Reconciliation for Haily.
//!
//! Implements Stage H2 requirements: node enrollment, role routing, session affinity,
//! parallel task placement, and disconnected-node reconciliation.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// 64-bit Node Identifier in C2C cluster.
pub type NodeId = u64;
/// 64-bit Distributed Task Identifier.
pub type TaskId = u64;

/// Specific functional capability role a node offers to the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    /// Primary user interaction and coordination node.
    Coordinator,
    /// General-purpose analysis and execution worker.
    Worker,
    /// Resident local LLM inference host (Spec 24).
    ModelInference,
    /// Isolated sandboxed code compilation/execution host.
    SandboxExecutor,
}

/// Dynamic health and lifecycle state of a cluster node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeState {
    /// Fully operational and accepting tasks.
    Active,
    /// Draining active workloads prior to planned retirement.
    Draining,
    /// Disconnected or heartbeat timed out.
    Disconnected,
}

/// Metadata and status record for an enrolled C2C node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HailyNode {
    pub id: NodeId,
    pub name: String,
    pub role: NodeRole,
    pub state: NodeState,
    pub capacity_weight: u32,
    pub active_tasks: u32,
}

/// Functional category of a distributed Haily workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskKind {
    CodeAnalysis,
    ResearchSynthesis,
    ModelInference,
}

/// Distributed task lifecycle outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskOutcome {
    Success(String),
    Failed(String),
    Indeterminate,
}

/// Workload unit dispatched across the C2C cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedTask {
    pub task_id: TaskId,
    pub session_id: String,
    pub kind: TaskKind,
    pub payload: String,
    pub assigned_node: Option<NodeId>,
    pub outcome: Option<TaskOutcome>,
}

/// Result of a task dispatch attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchStatus {
    Routed { task_id: TaskId, target_node: NodeId },
    RefusedNoCapacity,
    RefusedNoEligibleNode,
}

/// Coordinator managing node topology, placement, and failure handling.
#[derive(Debug)]
#[allow(dead_code)]
pub struct HailyClusterCoordinator {
    pub local_node_id: NodeId,
    pub nodes: Vec<HailyNode>,
    /// Session affinity: session_id -> preferred NodeId
    pub session_affinity: BTreeMap<String, NodeId>,
    pub next_task_id: TaskId,
}

impl HailyClusterCoordinator {
    /// Initialize cluster coordinator with local node identity.
    pub fn new(local_node_id: NodeId) -> Self {
        let local_node = HailyNode {
            id: local_node_id,
            name: String::from("Node-01-Local"),
            role: NodeRole::Coordinator,
            state: NodeState::Active,
            capacity_weight: 10,
            active_tasks: 0,
        };

        Self {
            local_node_id,
            nodes: alloc::vec![local_node],
            session_affinity: BTreeMap::new(),
            next_task_id: 1000,
        }
    }

    /// Enroll a newly authenticated C2C peer node into the cluster.
    pub fn enroll_node(&mut self, node: HailyNode) -> Result<(), ()> {
        if self.nodes.iter().any(|n| n.id == node.id) {
            return Err(()); // Duplicate node ID
        }
        self.nodes.push(node);
        Ok(())
    }

    /// Update the lifecycle state of a node (e.g. Active -> Draining / Disconnected).
    pub fn update_node_state(&mut self, node_id: NodeId, state: NodeState) -> Result<(), ()> {
        let node = self.nodes.iter_mut().find(|n| n.id == node_id).ok_or(())?;
        node.state = state;
        Ok(())
    }

    /// Dispatch and route a distributed task across the enrolled nodes.
    pub fn dispatch_task(&mut self, session_id: &str, kind: TaskKind, payload: &str) -> (DistributedTask, DispatchStatus) {
        self.next_task_id += 1;
        let task_id = self.next_task_id;

        let mut task = DistributedTask {
            task_id,
            session_id: String::from(session_id),
            kind,
            payload: String::from(payload),
            assigned_node: None,
            outcome: None,
        };

        // 1. Check Session Affinity
        if let Some(&preferred_id) = self.session_affinity.get(session_id) {
            if let Some(node) = self.nodes.iter_mut().find(|n| n.id == preferred_id && n.state == NodeState::Active) {
                if node.active_tasks < node.capacity_weight * 2 {
                    node.active_tasks += 1;
                    task.assigned_node = Some(node.id);
                    return (task, DispatchStatus::Routed { task_id, target_node: node.id });
                }
            }
        }

        // 2. Filter eligible nodes by role and active state
        let eligible: Vec<usize> = self.nodes.iter().enumerate()
            .filter(|(_, n)| n.state == NodeState::Active && match kind {
                TaskKind::CodeAnalysis => matches!(n.role, NodeRole::Worker | NodeRole::SandboxExecutor | NodeRole::Coordinator),
                TaskKind::ResearchSynthesis => matches!(n.role, NodeRole::Worker | NodeRole::Coordinator),
                TaskKind::ModelInference => matches!(n.role, NodeRole::ModelInference | NodeRole::Worker | NodeRole::Coordinator),
            })
            .map(|(idx, _)| idx)
            .collect();

        if eligible.is_empty() {
            return (task, DispatchStatus::RefusedNoEligibleNode);
        }

        // 3. Lowest-load selection based on active_tasks / capacity_weight ratio
        let mut best_idx = eligible[0];
        let mut min_load = (self.nodes[best_idx].active_tasks as f32) / (self.nodes[best_idx].capacity_weight as f32).max(1.0);

        for &idx in &eligible[1..] {
            let load = (self.nodes[idx].active_tasks as f32) / (self.nodes[idx].capacity_weight as f32).max(1.0);
            if load < min_load || (load == min_load && self.nodes[idx].capacity_weight > self.nodes[best_idx].capacity_weight) {
                min_load = load;
                best_idx = idx;
            }
        }

        let chosen_node = &mut self.nodes[best_idx];
        if chosen_node.active_tasks >= chosen_node.capacity_weight * 4 {
            return (task, DispatchStatus::RefusedNoCapacity);
        }

        chosen_node.active_tasks += 1;
        let target_node = chosen_node.id;
        task.assigned_node = Some(target_node);

        // Bind session affinity for future turns
        self.session_affinity.insert(String::from(session_id), target_node);

        (task, DispatchStatus::Routed { task_id, target_node })
    }

    /// Reconcile active tasks assigned to a node that unexpectedly disconnected.
    ///
    /// Preserves Spec 20 §2.6 invariants: unacknowledged tasks become `Indeterminate`
    /// rather than silently re-executing non-idempotent operations.
    pub fn reconcile_disconnection(&mut self, disconnected_node_id: NodeId, active_tasks: &mut [DistributedTask]) -> usize {
        let _ = self.update_node_state(disconnected_node_id, NodeState::Disconnected);
        let mut reconciled = 0;

        for task in active_tasks.iter_mut() {
            if task.assigned_node == Some(disconnected_node_id) && task.outcome.is_none() {
                task.outcome = Some(TaskOutcome::Indeterminate);
                reconciled += 1;
            }
        }

        // Drop session affinity pointing to disconnected node
        self.session_affinity.retain(|_, &mut node_id| node_id != disconnected_node_id);

        reconciled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_enrollment_and_duplicate_refusal() {
        let mut coord = HailyClusterCoordinator::new(1001);
        assert_eq!(coord.nodes.len(), 1);

        let peer = HailyNode {
            id: 2002,
            name: String::from("Node-02"),
            role: NodeRole::Worker,
            state: NodeState::Active,
            capacity_weight: 10,
            active_tasks: 0,
        };
        assert!(coord.enroll_node(peer.clone()).is_ok());
        assert_eq!(coord.nodes.len(), 2);

        // Duplicate enrollment must fail
        assert!(coord.enroll_node(peer).is_err());
    }

    #[test]
    fn test_task_dispatch_and_affinity() {
        let mut coord = HailyClusterCoordinator::new(1001);
        coord.enroll_node(HailyNode {
            id: 2002,
            name: String::from("Node-02"),
            role: NodeRole::Worker,
            state: NodeState::Active,
            capacity_weight: 50,
            active_tasks: 0,
        }).unwrap();

        // First turn: routes to Node-02 (weight 50 > weight 10)
        let (task1, status1) = coord.dispatch_task("sess-01", TaskKind::ResearchSynthesis, "payload");
        assert_eq!(task1.assigned_node, Some(2002));
        assert_eq!(status1, DispatchStatus::Routed { task_id: task1.task_id, target_node: 2002 });

        // Second turn with same session: preserves affinity to Node-02
        let (task2, status2) = coord.dispatch_task("sess-01", TaskKind::CodeAnalysis, "payload 2");
        assert_eq!(task2.assigned_node, Some(2002));
        assert_eq!(status2, DispatchStatus::Routed { task_id: task2.task_id, target_node: 2002 });
    }

    #[test]
    fn test_node_disconnection_reconciliation() {
        let mut coord = HailyClusterCoordinator::new(1001);
        coord.enroll_node(HailyNode {
            id: 2002,
            name: String::from("Node-02"),
            role: NodeRole::Worker,
            state: NodeState::Active,
            capacity_weight: 20,
            active_tasks: 1,
        }).unwrap();

        let mut tasks = alloc::vec![
            DistributedTask {
                task_id: 1001,
                session_id: String::from("sess-01"),
                kind: TaskKind::ResearchSynthesis,
                payload: String::from("work"),
                assigned_node: Some(2002),
                outcome: None,
            }
        ];

        // Reconcile node 2002 disconnect
        let count = coord.reconcile_disconnection(2002, &mut tasks);
        assert_eq!(count, 1);
        assert_eq!(tasks[0].outcome, Some(TaskOutcome::Indeterminate));
        assert_eq!(coord.nodes.iter().find(|n| n.id == 2002).unwrap().state, NodeState::Disconnected);
    }
}
