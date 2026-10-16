//! Workload endpoints and destination Ready evidence validation.

use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointKind {
    Logical,
    Concrete { generation: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadyEvidence {
    pub destination_node: [u8; 32],
    pub artifact_sha256: [u8; 32],
    pub tier: u8,
    pub reservation_key: [u8; 16],
    pub service_id: u16,
    pub export_id: u16,
    pub generation: u64,
}

impl ReadyEvidence {
    pub fn verifies(&self, expected_artifact: &[u8; 32], expected_service: u16) -> bool {
        &self.artifact_sha256 == expected_artifact && self.service_id == expected_service
    }
}

#[derive(Clone, Debug)]
pub struct WorkloadMember {
    pub member_id: u64,
    pub destination_node: [u8; 32],
    pub generation: u64,
    pub weight: u32,
    pub is_ready: bool,
    pub is_draining: bool,
    pub ready_evidence: Option<ReadyEvidence>,
}

pub struct EndpointRegistry {
    members: Vec<WorkloadMember>,
}

impl EndpointRegistry {
    pub fn new() -> Self {
        Self {
            members: Vec::new(),
        }
    }

    pub fn add_member(&mut self, member: WorkloadMember) {
        self.members.push(member);
    }

    pub fn active_members(&self) -> impl Iterator<Item = &WorkloadMember> {
        self.members.iter().filter(|m| m.is_ready && !m.is_draining)
    }
}
