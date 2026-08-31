use super::snapshot_support::*;
use super::Fixture;
use authority_protocol::*;
use core::cell::Cell;
use sha2::{Digest, Sha256};
use stm32_authority_journal::*;

pub(super) fn complete_upload(
    fixture: &Fixture,
) -> (
    ProtectedAuthorityRecord,
    ProtectedAuthorityRecord,
    MemoryBank,
    AdmittedProfileValidation,
) {
    let revision = Cell::new(0);
    let record = Cell::new(None);
    let store = Store {
        revision: &revision,
        record: &record,
    };
    let mut state = pending_state(store);

    let profile_digest = Sha256::digest(&fixture.profile).into();
    let tpm_digest = Sha256::digest(&fixture.tpm).into();
    let request = validated(BeginRelayProfileUploadRequest {
        context: context(5, 1, Operation::BeginRelayProfileUpload),
        upload_handle: 11,
        generation: 1,
        policy_epoch: 3,
        pending_slot: 0,
        pending_spki_digest: fixture.spki,
        profile_digest,
        tpm_public_digest: tpm_digest,
        profile_len: fixture.profile.len() as u32,
    });
    let metadata = ProfileBankMetadata {
        slot: 0,
        device_id: [1; 32],
        authority_id: [2; 32],
        authority_epoch: 1,
        boot_epoch: 1,
        generation: 1,
        policy_epoch: 3,
        upload_handle: 11,
        profile_len: fixture.profile.len() as u32,
        profile_digest,
        pending_spki_digest: fixture.spki,
        spki: Bounded::from_slice(&fixture.spki_der).unwrap(),
        tpm_public_digest: tpm_digest,
    };
    let mut bank = ProfileBank::new(MemoryBank::default(), Auth);
    let mut upload = begin_profile_upload(&mut state, &mut bank, &request, &metadata).unwrap();
    for (index, bytes) in fixture.profile.chunks(PROFILE_CHUNK_SIZE).enumerate() {
        let write = validated(WriteRelayProfileChunkRequest {
            context: context(6 + index as u64, 1, Operation::WriteRelayProfileChunk),
            upload_handle: 11,
            chunk_index: index as u8,
            chunk: Bounded::from_slice(bytes).unwrap(),
        });
        upload = write_profile_chunk(&mut state, &mut bank, &write, &metadata).unwrap();
    }
    let validation = validated(ValidateAndStageRelayProfileRequest {
        context: context(
            6 + upload.chunk_count() as u64,
            1,
            Operation::ValidateAndStageRelayProfile,
        ),
        generation: 1,
        policy_epoch: 3,
        pending_slot: 0,
        pending_spki_digest: fixture.spki,
        profile_digest,
        tpm_public_digest: tpm_digest,
        upload_handle: 11,
        profile_len: fixture.profile.len() as u32,
    });
    let prior = record.get().unwrap();
    let admitted = state.admit_profile_validation(&validation).unwrap();
    let current = record.get().unwrap();
    let (storage, _) = bank.into_parts();
    (prior, current, storage, admitted)
}

pub(super) fn pending_state<S: ProtectedStore>(store: S) -> AuthorityState<S> {
    let mut state = AuthorityState::new(
        store,
        AuthorityStateConfig {
            device_id: [1; 32],
            authority_id: [2; 32],
            authority_epoch: 1,
            boot_floor: 0,
            generation_floor: 0,
            state_epoch: 0,
            boot_challenge: [3; 32],
            time_floors: ProtectedTimeFloors {
                source_epoch: 0,
                source_sequence: 0,
                unix_seconds: 0,
            },
        },
    );
    let open = validated(OpenBootRequest {
        context: context(1, 0, Operation::OpenBoot),
        loader_digest: [7; 32],
    });
    state
        .open_boot(&open, &verify_boot_measurement([7; 32], &Boot).unwrap())
        .unwrap();
    let ask = validated(RequestSignedTimeRequest {
        context: context(2, 1, Operation::RequestSignedTime),
        purpose: TimePurpose::Enrollment as u8,
    });
    let challenge = state.request_signed_time(&ask, &mut Challenges).unwrap();
    let mut fact = AcceptSignedTimeRequest {
        context: context(3, 1, Operation::AcceptSignedTime),
        time_request_id: challenge.time_request_id,
        purpose: TimePurpose::Enrollment as u8,
        source_epoch: 1,
        source_sequence: 1,
        unix_seconds: 100,
        expires_at: 200,
        nonce: challenge.nonce,
        source_signature: Bounded::from_slice(&[0x30, 6, 2, 1, 1, 2, 1, 1]).unwrap(),
    };
    fact.context.payload_digest = fact.canonical_body_digest();
    let fact_header = header(&fact);
    let fact = verify_signed_time(fact, &fact_header, &Requests, &SignedTime).unwrap();
    state.accept_time(&fact, &Clock).unwrap();
    let begin = validated(BeginRelayEnrollmentRequest {
        context: context(4, 1, Operation::BeginRelayEnrollment),
        hostname: Bounded::from_slice(b"node.example").unwrap(),
    });
    state.begin_enrollment(&begin, &Clock).unwrap();
    state
}

pub(super) fn context(sequence: u64, boot: u64, operation: Operation) -> RequestContext {
    RequestContext {
        device_id: [1; 32],
        authority_id: [2; 32],
        boot_epoch: boot,
        sequence,
        challenge: [3; 32],
        request_id: sequence + 100,
        operation,
        payload_digest: [0; 32],
        authenticator: [0; 32],
    }
}
pub(super) fn validated<T: AuthorityRequest>(mut request: T) -> ValidatedRequest<T> {
    request.context_mut().payload_digest = request.canonical_body_digest();
    let request_header = header(&request);
    verify_typed_request(request, &request_header, &Requests).unwrap()
}
fn header<T: AuthorityRequest>(request: &T) -> FrameHeader {
    FrameHeader {
        class: FrameClass::Request,
        operation: T::OPERATION,
        payload_len: request.canonical_payload_len() as u16,
        request_id: request.context().request_id,
        authenticator: [0; 16],
    }
}
