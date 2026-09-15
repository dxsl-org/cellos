// SPDX-License-Identifier: MPL-2.0
//! Common task-creation admission gate for owner consent and floor matching.

use super::slot_format::ParsedSlot;
use super::{AdmissionDecision, FloorPortOutcome, SlotId};
use crate::sync::Spinlock;
use types::{ViError, ViResult};

/// Global active owner admission state.
#[allow(dead_code)]
pub(crate) struct OwnerAdmissionRegistry {
    pub(crate) floor_port: FloorPortOutcome,
    pub(crate) slots: [Option<ParsedSlot>; 2],
    pub(crate) active_admitted_slot: Option<SlotId>,
    pub(crate) production_enforced: bool,
}

impl OwnerAdmissionRegistry {
    pub const fn new() -> Self {
        Self {
            floor_port: FloorPortOutcome::Missing,
            slots: [None, None],
            active_admitted_slot: None,
            production_enforced: false,
        }
    }

    /// Update the floor outcome and installed A/B slots, recomputing admission decision.
    #[allow(dead_code)]
    pub(crate) fn update(
        &mut self,
        floor: FloorPortOutcome,
        slot_a: Option<ParsedSlot>,
        slot_b: Option<ParsedSlot>,
    ) -> AdmissionDecision {
        self.floor_port = floor;
        let obs_a = slot_a
            .as_ref()
            .map_or(super::SlotObservation::Missing, |s| s.observation());
        let obs_b = slot_b
            .as_ref()
            .map_or(super::SlotObservation::Missing, |s| s.observation());
        self.slots = [slot_a, slot_b];

        let decision = super::decide(&self.floor_port, &[obs_a, obs_b]);
        match decision {
            AdmissionDecision::Admit(slot_id) => {
                self.active_admitted_slot = Some(slot_id);
            }
            _ => {
                self.active_admitted_slot = None;
            }
        }
        decision
    }

    /// Check if the given ELF digest is authorized by the current admitted owner slot.
    pub fn check_admission(&self, elf_sha256: &[u8; 32]) -> ViResult<()> {
        if !self.production_enforced {
            // Dev/permissive posture: permit spawn when production is not strictly enforced
            return Ok(());
        }

        let slot_id = self.active_admitted_slot.ok_or(ViError::PermissionDenied)?;
        let slot = match slot_id {
            SlotId::A => self.slots[0].as_ref(),
            SlotId::B => self.slots[1].as_ref(),
        }
        .ok_or(ViError::PermissionDenied)?;

        if slot.admits_elf(elf_sha256) {
            Ok(())
        } else {
            Err(ViError::PermissionDenied)
        }
    }

    #[allow(dead_code)]
    pub fn set_production_enforced(&mut self, enforced: bool) {
        self.production_enforced = enforced;
    }
}

pub(crate) static ADMISSION_REGISTRY: Spinlock<OwnerAdmissionRegistry> =
    Spinlock::new(OwnerAdmissionRegistry::new());

/// Evaluate owner admission for an ELF binary before task creation.
pub fn evaluate_owner_admission(elf_sha256: &[u8; 32]) -> ViResult<()> {
    ADMISSION_REGISTRY.lock().check_admission(elf_sha256)
}
