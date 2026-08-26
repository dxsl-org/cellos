use types::kms::{KmsErrorCode, RELAY_CSR_CHUNK_LEN, RELAY_CSR_MAX_LEN};

mod pending;

pub(crate) use pending::PendingEnrollment;
pub(crate) use pending::{derive_csr_handle, PendingStage};

/// Supervisor identity bound into every pending enrollment fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SupervisorContext {
    pub cell_id: u64,
    pub generation: u64,
    pub tid: usize,
}

impl SupervisorContext {
    pub(crate) fn from_parts(cell_id: u64, generation: u64, sender: usize) -> Self {
        Self {
            cell_id,
            generation,
            tid: sender,
        }
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        self == other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveRelayGeneration {
    pub generation: u64,
    pub policy_epoch: u64,
    pub profile_digest: [u8; 32],
    pub revoked: bool,
}

impl ActiveRelayGeneration {
    /// A revoked generation serves nothing; overlap never resurrects it.
    pub(crate) const fn serving(&self) -> bool {
        !self.revoked
    }
}

/// Authenticated lifecycle facts stored inside the existing sealed journal
/// payload. Pending enrollment material is intentionally never persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProtectedRelayState {
    pub active: Option<ActiveRelayGeneration>,
    pub authenticated_time_floor: u64,
    pub restart_epoch_floor: u64,
}

impl ProtectedRelayState {
    pub(crate) const ENCODED_LEN: usize = 64;

    pub(crate) fn encode(self) -> [u8; Self::ENCODED_LEN] {
        let mut out = [0u8; Self::ENCODED_LEN];
        if let Some(active) = self.active {
            out[..8].copy_from_slice(&active.generation.to_le_bytes());
            out[8..16].copy_from_slice(&active.policy_epoch.to_le_bytes());
            out[16..48].copy_from_slice(&active.profile_digest);
        }
        out[48..56].copy_from_slice(&self.authenticated_time_floor.to_le_bytes());
        out[56..64].copy_from_slice(&self.restart_epoch_floor.to_le_bytes());
        out
    }

    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::ENCODED_LEN {
            return None;
        }
        let generation = u64::from_le_bytes(bytes[..8].try_into().ok()?);
        let policy_epoch = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
        let profile_digest: [u8; 32] = bytes[16..48].try_into().ok()?;
        let authenticated_time_floor = u64::from_le_bytes(bytes[48..56].try_into().ok()?);
        let restart_epoch_floor = u64::from_le_bytes(bytes[56..64].try_into().ok()?);
        if restart_epoch_floor == 0 {
            return None;
        }
        let active = if generation == 0 {
            if policy_epoch != 0
                || profile_digest.iter().any(|byte| *byte != 0)
                || authenticated_time_floor != 0
            {
                return None;
            }
            None
        } else {
            if policy_epoch == 0
                || profile_digest.iter().all(|byte| *byte == 0)
                || authenticated_time_floor == 0
            {
                return None;
            }
            Some(ActiveRelayGeneration {
                generation,
                policy_epoch,
                profile_digest,
                revoked: false,
            })
        };
        Some(Self {
            active,
            authenticated_time_floor,
            restart_epoch_floor,
        })
    }
}

/// Protected active/pending relay lifecycle inside KMS.
///
/// Only committed generation/profile and monotonic floors are recoverable.
/// Missing, torn, or regressed protected state seals both enrollment and
/// serving; volatile entropy alone is never treated as durable state.
pub(crate) struct RelayLifecycle {
    active: Option<ActiveRelayGeneration>,
    pending: Option<PendingEnrollment>,
    restart_epoch: u64,
    begin_counter: u64,
    authenticated_time_floor: u64,
    enrollment_sealed: bool,
    /// Invalid pending slots remain as cleanup tombstones until the provider
    /// confirms deletion or absence.
    pending_poisoned: bool,
    invalidated_read_error: KmsErrorCode,
}

impl RelayLifecycle {
    /// Explicit protected development/test seam for a new empty journal.
    #[cfg(test)]
    pub(crate) fn with_entropy(restart_epoch: u64) -> Self {
        Self::construct(None, restart_epoch, 0, false)
    }

    /// Recover only from authenticated protected facts and a strictly newer
    /// restart epoch. Replayed monotonic state is rejected fail-closed.
    pub(crate) fn recover(
        restart_epoch: u64,
        protected: ProtectedRelayState,
    ) -> Result<Self, KmsErrorCode> {
        if restart_epoch == 0 || restart_epoch <= protected.restart_epoch_floor {
            return Err(KmsErrorCode::PolicyEpochRegressed);
        }
        Ok(Self::construct(
            protected.active,
            restart_epoch,
            protected.authenticated_time_floor,
            false,
        ))
    }

    /// No authenticated protected journal is available: refuse enrollment and
    /// serving rather than silently resetting generation/policy floors.
    pub(crate) fn sealed() -> Self {
        Self::construct(None, 0, 0, true)
    }

    fn construct(
        active: Option<ActiveRelayGeneration>,
        restart_epoch: u64,
        authenticated_time_floor: u64,
        sealed: bool,
    ) -> Self {
        Self {
            active,
            pending: None,
            restart_epoch,
            begin_counter: 0,
            authenticated_time_floor,
            enrollment_sealed: sealed,
            pending_poisoned: false,
            invalidated_read_error: KmsErrorCode::CsrHandleInvalid,
        }
    }

    /// Test seam: start serving a committed generation without touching
    /// the boot-derived restart epoch.
    #[cfg(test)]
    pub(crate) fn activate_for_tests(&mut self, active: ActiveRelayGeneration) {
        self.active = Some(active);
        self.authenticated_time_floor = 1_700_000_000;
    }

    pub(crate) fn restart_epoch(&self) -> u64 {
        self.restart_epoch
    }

    /// The identity allowed to serve mTLS right now, if any.
    pub(crate) fn serving(&self) -> Option<ActiveRelayGeneration> {
        if self.enrollment_sealed {
            None
        } else {
            self.active.filter(|active| active.serving())
        }
    }

    pub(crate) fn pending(&self) -> Option<&PendingEnrollment> {
        self.pending.as_ref()
    }

    pub(crate) fn cleanup_pending(&self) -> Option<&PendingEnrollment> {
        self.pending_poisoned
            .then_some(self.pending.as_ref())
            .flatten()
    }

    pub(crate) fn mark_cleanup_required(&mut self) {
        if self.pending.is_some() {
            self.pending_poisoned = true;
            self.invalidated_read_error = KmsErrorCode::CsrHandleInvalid;
        }
    }

    /// Next generation id: strictly monotonic across renewals and revocation.
    pub(crate) fn next_generation(&self) -> u64 {
        self.active.map_or(1, |active| active.generation + 1)
    }

    /// Next protected policy epoch: never regresses below anything seen.
    pub(crate) fn next_policy_epoch(&self) -> u64 {
        self.active.map_or(1, |active| active.policy_epoch + 1)
    }

    /// Reserve the single pending slot for this live supervisor. Refused
    /// outright when the boot could not protect the restart epoch.
    pub(crate) fn open_pending(
        &mut self,
        supervisor: SupervisorContext,
        begin_request_id: u32,
    ) -> Result<(u64, u64), KmsErrorCode> {
        if self.enrollment_sealed {
            return Err(KmsErrorCode::RelayUnavailable);
        }
        if self.pending.is_some() {
            return Err(KmsErrorCode::EnrollmentPendingExists);
        }
        self.pending_poisoned = false;
        self.invalidated_read_error = KmsErrorCode::CsrHandleInvalid;
        self.begin_counter = match self.begin_counter.checked_add(1) {
            Some(counter) => counter,
            None => {
                self.enrollment_sealed = true;
                return Err(KmsErrorCode::RelayUnavailable);
            }
        };
        let generation = self.next_generation();
        let policy_epoch = self.next_policy_epoch();
        let csr_handle = derive_csr_handle(
            generation,
            policy_epoch,
            begin_request_id as u64,
            self.begin_counter,
            self.restart_epoch,
            &supervisor,
        );
        self.pending = Some(PendingEnrollment::new(
            generation,
            policy_epoch,
            supervisor,
            csr_handle,
        ));
        Ok((generation, policy_epoch))
    }

    /// Install the assembled canonical CSR once provider proof verification
    /// succeeded (`Prepared -> CsrIssued`). Any earlier failure must abort
    /// the pending slot instead.
    pub(crate) fn install_csr(&mut self, csr: &[u8; RELAY_CSR_MAX_LEN], csr_len: usize) {
        if let Some(pending) = &mut self.pending {
            pending.install(csr, csr_len);
        }
    }

    /// Drop a valid pending slot after an explicit abort.
    pub(crate) fn drop_pending(&mut self) -> Option<PendingEnrollment> {
        self.pending_poisoned = false;
        self.invalidated_read_error = KmsErrorCode::CsrHandleInvalid;
        self.pending.take()
    }

    /// Remove a provider-confirmed cleanup tombstone while retaining the
    /// invalidating read result until a fresh Begin replaces it.
    pub(crate) fn confirm_cleanup(&mut self) -> Option<PendingEnrollment> {
        self.pending.take()
    }

    /// One-shot ordered chunk reader bound to the exact live supervisor.
    ///
    /// Any mismatch of handle, supervisor identity/generation/TID, or read
    /// order invalidates the whole pending enrollment before erroring.
    pub(crate) fn read_chunk(
        &mut self,
        handle: u64,
        chunk_index: u32,
        supervisor: &SupervisorContext,
    ) -> Result<([u8; RELAY_CSR_CHUNK_LEN], usize), KmsErrorCode> {
        if self.pending_poisoned {
            return Err(self.invalidated_read_error);
        }
        let Some(pending) = &mut self.pending else {
            return Err(KmsErrorCode::CsrHandleInvalid);
        };
        // Which supervisor is asking is checked before any handle facts:
        // a foreign identity must learn nothing and poisons the cleanup tombstone.
        if !pending.supervisor.matches(supervisor) {
            self.invalidated_read_error = KmsErrorCode::PermissionDenied;
            self.pending_poisoned = true;
            return Err(KmsErrorCode::PermissionDenied);
        }
        if pending.csr_handle != handle || pending.stage != PendingStage::CsrIssued {
            self.invalidated_read_error = KmsErrorCode::CsrHandleInvalid;
            self.pending_poisoned = true;
            return Err(KmsErrorCode::CsrHandleInvalid);
        }
        match pending.chunk(chunk_index) {
            Ok(output) => Ok(output),
            Err(code) => {
                self.invalidated_read_error = KmsErrorCode::CsrHandleInvalid;
                self.pending_poisoned = true;
                Err(code)
            }
        }
    }

    /// Authenticated service-net staging (`CsrIssued -> Staged`): bind the
    /// validated profile digest to this pending generation. Only valid once
    /// every CSR chunk has been consumed in order.
    pub(crate) fn stage_pending(
        &mut self,
        pending_generation: u64,
        expected_policy_epoch: u64,
        profile_digest: [u8; 32],
    ) -> Result<(), KmsErrorCode> {
        let pending = self
            .pending
            .as_mut()
            .ok_or(KmsErrorCode::CsrHandleInvalid)?;
        if pending.generation != pending_generation {
            return Err(KmsErrorCode::RelayGenerationMismatch);
        }
        if expected_policy_epoch != pending.policy_epoch {
            return Err(KmsErrorCode::InvalidRequest);
        }
        pending.mark_staged(profile_digest)
    }

    /// Validate a commit request without mutating anything. The caller must
    /// promote the provider key only after this succeeds and before
    /// [`RelayLifecycle::apply_commit`], so a failed promotion leaves no
    /// mixed state.
    pub(crate) fn prepare_commit(
        &self,
        pending_generation: u64,
        expected_policy_epoch: u64,
        profile_digest: [u8; 32],
        supervisor: &SupervisorContext,
    ) -> Result<(), KmsErrorCode> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(KmsErrorCode::CsrHandleInvalid)?;
        if pending.generation != pending_generation {
            return Err(KmsErrorCode::RelayGenerationMismatch);
        }
        if !pending.supervisor.matches(supervisor) {
            return Err(KmsErrorCode::PermissionDenied);
        }
        if !pending.staged_digest_matches(profile_digest)
            || expected_policy_epoch != pending.policy_epoch
        {
            return Err(KmsErrorCode::InvalidRequest);
        }
        if let Some(active) = self.active {
            if expected_policy_epoch < active.policy_epoch {
                return Err(KmsErrorCode::PolicyEpochRegressed);
            }
        }
        Ok(())
    }

    /// Atomically activate the pending generation. Only a fully staged slot
    /// commits, and only with the exact digest service-net attested; the
    /// previous active keeps serving until this call succeeds. Call only
    /// after [`RelayLifecycle::prepare_commit`] and provider promotion.
    pub(crate) fn apply_commit(
        &mut self,
        pending_generation: u64,
        expected_policy_epoch: u64,
        profile_digest: [u8; 32],
        supervisor: &SupervisorContext,
    ) -> Result<ActiveRelayGeneration, KmsErrorCode> {
        self.prepare_commit(
            pending_generation,
            expected_policy_epoch,
            profile_digest,
            supervisor,
        )?;
        let pending = self.pending.as_ref().expect("checked above");
        let committed = ActiveRelayGeneration {
            generation: pending.generation,
            policy_epoch: pending.policy_epoch,
            profile_digest,
            revoked: false,
        };
        self.pending = None;
        self.pending_poisoned = false;
        self.active = Some(committed);
        Ok(committed)
    }

    pub(crate) fn observe_authenticated_time(
        &mut self,
        authenticated_time_floor: u64,
    ) -> Result<(), KmsErrorCode> {
        if authenticated_time_floor == 0 || authenticated_time_floor < self.authenticated_time_floor
        {
            self.enrollment_sealed = true;
            return Err(KmsErrorCode::TimeUntrusted);
        }
        self.authenticated_time_floor = authenticated_time_floor;
        Ok(())
    }

    pub(crate) fn protected_state(&self) -> Result<ProtectedRelayState, KmsErrorCode> {
        if self.enrollment_sealed || self.restart_epoch == 0 {
            return Err(KmsErrorCode::RelayUnavailable);
        }
        Ok(ProtectedRelayState {
            active: self.active,
            authenticated_time_floor: self.authenticated_time_floor,
            restart_epoch_floor: self.restart_epoch,
        })
    }

    pub(crate) fn seal(&mut self) {
        self.enrollment_sealed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::kms::KmsErrorCode;

    fn supervisor() -> SupervisorContext {
        SupervisorContext {
            cell_id: 91,
            generation: 3,
            tid: 8,
        }
    }

    #[test]
    fn regressed_protected_restart_floor_is_refused() {
        let protected = ProtectedRelayState {
            active: None,
            authenticated_time_floor: 0,
            restart_epoch_floor: 4,
        };
        assert!(RelayLifecycle::recover(5, protected).is_ok());
        // Equal and lower epochs are rollback: fail-closed, typed error.
        for epoch in [4, 3] {
            assert!(matches!(
                RelayLifecycle::recover(epoch, protected),
                Err(KmsErrorCode::PolicyEpochRegressed)
            ));
        }
    }

    #[test]
    fn sealed_boot_refuses_every_enrollment_open() {
        let mut sealed = RelayLifecycle::sealed();
        assert_eq!(sealed.restart_epoch(), 0);
        for request_id in [1u32, 2, 99] {
            assert_eq!(
                sealed.open_pending(supervisor(), request_id),
                Err(KmsErrorCode::RelayUnavailable)
            );
        }
    }

    #[test]
    fn foreign_touch_poisons_slot_until_fresh_begin() {
        let mut lifecycle = RelayLifecycle::with_entropy(7);
        lifecycle.open_pending(supervisor(), 1).unwrap();
        let owner = supervisor();
        let (handle, _) = {
            let pending = lifecycle.pending().unwrap();
            (pending.csr_handle, pending.stage)
        };
        let foreign = SupervisorContext {
            cell_id: 99,
            ..supervisor()
        };
        assert_eq!(
            lifecycle.read_chunk(handle, 0, &foreign).err(),
            Some(KmsErrorCode::PermissionDenied)
        );
        // The legitimate supervisor is denied too: the slot is poisoned.
        assert_eq!(
            lifecycle.read_chunk(handle, 0, &owner).err(),
            Some(KmsErrorCode::PermissionDenied)
        );
        // Provider-confirmed cleanup removes the tombstone; a fresh Begin
        // clears the retained denial and uses a new handle nonce.
        lifecycle.confirm_cleanup();
        lifecycle.open_pending(supervisor(), 2).unwrap();
        assert!(lifecycle.pending().is_some());
    }

    #[test]
    fn repeated_begin_request_id_still_produces_a_unique_handle() {
        let mut lifecycle = RelayLifecycle::with_entropy(7);
        lifecycle.open_pending(supervisor(), 1).unwrap();
        let first = lifecycle.pending().unwrap().csr_handle;
        lifecycle.mark_cleanup_required();
        lifecycle.confirm_cleanup();
        lifecycle.open_pending(supervisor(), 1).unwrap();
        let second = lifecycle.pending().unwrap().csr_handle;
        assert_ne!(first, second);
    }
}
