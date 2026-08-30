use super::{
    AuthorityState, EnrollmentIntent, PreparedCommitIntent, ProtectedStore, RelayProfileState,
    TimePurpose, TlsSignatureIntent, TrustedClock,
};
use crate::{
    constant_time_eq, AbortRelayEnrollmentRequest, AuthorityFault, BeginRelayEnrollmentRequest,
    CommitRelayGenerationRequest, ConsumeStagedRelayProfileRequest,
    SignTls13ClientCertificateVerifyRequest, ValidatedRequest, VerifiedProviderCasReceipt,
};

impl<S: ProtectedStore> AuthorityState<S> {
    pub fn begin_enrollment(
        &mut self,
        validated: &ValidatedRequest<BeginRelayEnrollmentRequest>,
        clock: &impl TrustedClock,
    ) -> Result<EnrollmentIntent, AuthorityFault> {
        let request = validated.request();
        self.authorize_context(&request.context)?;
        self.consume_live_time(TimePurpose::Enrollment, clock.now_unix_seconds())?;
        self.previous_active = match self.relay {
            RelayProfileState::Active(intent) => Some(intent),
            RelayProfileState::Empty => None,
            _ => return self.seal(AuthorityFault::ProviderSplitBrain),
        };
        self.generation_floor = match self.generation_floor.checked_add(1) {
            Some(value) => value,
            None => return self.seal(AuthorityFault::PersistenceFailure),
        };
        let pending_slot = self
            .previous_active
            .map_or(0, |intent| intent.pending_slot ^ 1);
        self.relay = RelayProfileState::Pending {
            generation: self.generation_floor,
            csr_handle: self.generation_floor,
            pending_slot,
        };
        self.persist_value(EnrollmentIntent {
            generation: self.generation_floor,
            pending_slot,
            hostname: request.hostname,
        })
    }

    pub fn consume_receipt(
        &mut self,
        validated: &ValidatedRequest<ConsumeStagedRelayProfileRequest>,
    ) -> Result<(), AuthorityFault> {
        let request = validated.request();
        self.authorize_context(&request.context)?;
        match self.relay {
            RelayProfileState::Staged(intent)
                if intent.generation == request.generation
                    && intent.policy_epoch == request.policy_epoch
                    && constant_time_eq(&intent.profile_digest, &request.profile_digest) =>
            {
                self.relay = RelayProfileState::ReceiptConsumed(intent);
                self.persist_value(())
            }
            RelayProfileState::ReceiptConsumed(_) => self.seal(AuthorityFault::ReceiptConsumed),
            _ => self.seal(AuthorityFault::ReceiptAbsent),
        }
    }

    pub fn prepare_commit(
        &mut self,
        validated: &ValidatedRequest<CommitRelayGenerationRequest>,
    ) -> Result<PreparedCommitIntent, AuthorityFault> {
        let request = validated.request();
        self.authorize_context(&request.context)?;
        let intent = match self.relay {
            RelayProfileState::ReceiptConsumed(value) | RelayProfileState::Prepared(value) => value,
            _ => return self.seal(AuthorityFault::ProviderSplitBrain),
        };
        if intent.generation != request.generation
            || intent.policy_epoch != request.policy_epoch
            || !constant_time_eq(&intent.profile_digest, &request.profile_digest)
        {
            return self.seal(AuthorityFault::ProviderSplitBrain);
        }
        self.relay = RelayProfileState::Prepared(intent);
        self.persist_value(PreparedCommitIntent(intent))
    }

    pub fn record_provider_promotion(
        &mut self,
        prepared: &PreparedCommitIntent,
        verified: &VerifiedProviderCasReceipt,
    ) -> Result<(), AuthorityFault> {
        let intent = *prepared.intent();
        if self.relay != RelayProfileState::Prepared(intent)
            || !intent.matches_receipt(verified.receipt())
        {
            return self.seal(AuthorityFault::ProviderSplitBrain);
        }
        self.relay = RelayProfileState::Promoted {
            intent,
            provider_signature: verified.receipt().provider_signature,
        };
        self.persist_value(())
    }

    pub fn finalize_commit(&mut self) -> Result<(), AuthorityFault> {
        match self.relay {
            RelayProfileState::Promoted { intent, .. } => {
                self.relay = RelayProfileState::Active(intent);
                self.previous_active = None;
                self.persist_value(())
            }
            _ => self.seal(AuthorityFault::ProviderSplitBrain),
        }
    }

    pub fn abort(
        &mut self,
        validated: &ValidatedRequest<AbortRelayEnrollmentRequest>,
    ) -> Result<u64, AuthorityFault> {
        let request = validated.request();
        self.authorize_context(&request.context)?;
        let matches = match self.relay {
            RelayProfileState::Pending { generation, .. } => generation == request.generation,
            RelayProfileState::Uploading(upload) => upload.generation == request.generation,
            RelayProfileState::Staged(intent) | RelayProfileState::ReceiptConsumed(intent) => {
                intent.generation == request.generation
            }
            _ => false,
        };
        if !matches {
            return self.seal(AuthorityFault::InvalidState);
        }
        self.relay = self
            .previous_active
            .take()
            .map_or(RelayProfileState::Empty, RelayProfileState::Active);
        self.persist_value(request.generation)
    }

    pub fn authorize_tls_signature(
        &mut self,
        validated: &ValidatedRequest<SignTls13ClientCertificateVerifyRequest>,
        clock: &impl TrustedClock,
    ) -> Result<TlsSignatureIntent, AuthorityFault> {
        let request = validated.request();
        self.authorize_context(&request.context)?;
        self.consume_live_time(TimePurpose::TlsCertificateVerify, clock.now_unix_seconds())?;
        match self.relay {
            RelayProfileState::Active(intent)
                if intent.generation == request.relay_generation
                    && constant_time_eq(&intent.profile_digest, &request.active_profile_digest) =>
            {
                self.persist_value(TlsSignatureIntent {
                    relay_generation: intent.generation,
                    transcript_hash: request.transcript_hash,
                    active_profile_digest: request.active_profile_digest,
                    public_request_id: request.public_request_id,
                })
            }
            _ => self.seal(AuthorityFault::InvalidState),
        }
    }
}
