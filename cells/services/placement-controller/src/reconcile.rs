//! Downscale reconciliation, draining status, and key release.

use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberLifecycleState {
    Active,
    Draining,
    Quiesced,
    Gone,
}

pub struct ReconcileTarget {
    pub key: [u8; 16],
    pub node_id: [u8; 32],
    pub state: MemberLifecycleState,
}

pub struct ReconciliationTracker {
    targets: Vec<ReconcileTarget>,
}

impl ReconciliationTracker {
    pub fn new() -> Self {
        Self {
            targets: Vec::new(),
        }
    }

    /// Mark a target for downscale (enters Draining state).
    pub fn begin_drain(&mut self, key: [u8; 16], node_id: [u8; 32]) {
        self.targets.push(ReconcileTarget {
            key,
            node_id,
            state: MemberLifecycleState::Draining,
        });
    }

    /// Acknowledge that the draining target has quiesced all in-flight requests.
    pub fn observe_quiescence(&mut self, key: &[u8; 16]) -> Result<(), ()> {
        let target = self.targets.iter_mut().find(|t| &t.key == key).ok_or(())?;
        if target.state != MemberLifecycleState::Draining {
            return Err(());
        }
        target.state = MemberLifecycleState::Quiesced;
        Ok(())
    }

    /// Complete closure once the destination confirms absence/Gone.
    pub fn observe_gone(&mut self, key: &[u8; 16]) -> Result<(), ()> {
        let target = self.targets.iter_mut().find(|t| &t.key == key).ok_or(())?;
        if target.state != MemberLifecycleState::Quiesced {
            return Err(());
        }
        target.state = MemberLifecycleState::Gone;
        Ok(())
    }
}
