use super::{context, validated, CasPolicy, TestState};
use authority_protocol::*;

pub fn begin(sequence: u64, boot: u64) -> ValidatedRequest<BeginRelayEnrollmentRequest> {
    validated(BeginRelayEnrollmentRequest {
        context: context(sequence, boot, Operation::BeginRelayEnrollment),
        hostname: Bounded::from_slice(b"relay.example").unwrap(),
    })
}

pub fn stage(
    sequence: u64,
    boot: u64,
    generation: u64,
    digest: [u8; 32],
) -> ValidateAndStageRelayProfileRequest {
    ValidateAndStageRelayProfileRequest {
        context: context(sequence, boot, Operation::ValidateAndStageRelayProfile),
        generation,
        policy_epoch: 1,
        pending_slot: 0,
        pending_spki_digest: [7; 32],
        profile_digest: digest,
        tpm_public_digest: [6; 32],
        profile: Bounded::from_slice(&digest).unwrap(),
    }
}

pub fn consume(
    sequence: u64,
    boot: u64,
    generation: u64,
    digest: [u8; 32],
) -> ValidatedRequest<ConsumeStagedRelayProfileRequest> {
    validated(ConsumeStagedRelayProfileRequest {
        context: context(sequence, boot, Operation::ConsumeStagedRelayProfile),
        generation,
        policy_epoch: 1,
        profile_digest: digest,
    })
}

pub fn commit(
    sequence: u64,
    boot: u64,
    generation: u64,
    digest: [u8; 32],
) -> ValidatedRequest<CommitRelayGenerationRequest> {
    validated(CommitRelayGenerationRequest {
        context: context(sequence, boot, Operation::CommitRelayGeneration),
        generation,
        policy_epoch: 1,
        profile_digest: digest,
    })
}

pub fn promote(state: &mut TestState, request: &ValidatedRequest<CommitRelayGenerationRequest>) {
    let prepared = state.prepare_commit(request).unwrap();
    let intent = prepared.intent();
    let receipt = ProviderCasReceipt {
        device_id: intent.device_id,
        authority_id: intent.authority_id,
        authority_epoch: intent.authority_epoch,
        generation: intent.generation,
        policy_epoch: intent.policy_epoch,
        pending_slot: intent.pending_slot,
        pending_spki_digest: intent.pending_spki_digest,
        profile_digest: intent.profile_digest,
        boot_epoch: intent.boot_epoch,
        validation_request_id: intent.validation_request_id,
        provider_signature: [9; 64],
    };
    let verified = verify_provider_cas_receipt(receipt, &CasPolicy).unwrap();
    state
        .record_provider_promotion(&prepared, &verified)
        .unwrap();
    state.finalize_commit().unwrap();
}
