#[path = "wire_fixture_values/response.rs"]
mod response;
pub use response::responses;

use authority_protocol::*;

fn context(operation: Operation) -> RequestContext {
    RequestContext {
        device_id: [1; 32],
        authority_id: [2; 32],
        boot_epoch: 3,
        sequence: 4,
        challenge: [5; 32],
        request_id: 6,
        operation,
        payload_digest: [0; 32],
        authenticator: [7; 32],
    }
}

pub fn requests() -> [TypedRequest; 14] {
    use Operation::*;
    let mut values = [
        TypedRequest::OpenBoot(OpenBootRequest {
            context: context(OpenBoot),
            loader_digest: [9; 32],
        }),
        TypedRequest::ReadCommittedRelayState(ReadCommittedRelayStateRequest {
            context: context(ReadCommittedRelayState),
        }),
        TypedRequest::RequestSignedTime(RequestSignedTimeRequest {
            context: context(RequestSignedTime),
            purpose: 1,
        }),
        TypedRequest::AcceptSignedTime(AcceptSignedTimeRequest {
            context: context(AcceptSignedTime),
            time_request_id: [10; 16],
            purpose: 1,
            source_epoch: 11,
            source_sequence: 12,
            unix_seconds: 1000,
            expires_at: 1060,
            nonce: [11; 32],
            source_signature: Bounded::from_slice(&[0x30, 6, 2, 1, 1, 2, 1, 1]).unwrap(),
        }),
        TypedRequest::BeginRelayEnrollment(BeginRelayEnrollmentRequest {
            context: context(BeginRelayEnrollment),
            hostname: Bounded::from_slice(b"relay.example").unwrap(),
        }),
        TypedRequest::ReadRelayCsrChunk(ReadRelayCsrChunkRequest {
            context: context(ReadRelayCsrChunk),
            csr_handle: 13,
            chunk_index: 14,
        }),
        TypedRequest::ValidateAndStageRelayProfile(ValidateAndStageRelayProfileRequest {
            context: context(ValidateAndStageRelayProfile),
            generation: 15,
            policy_epoch: 16,
            pending_slot: 1,
            pending_spki_digest: [12; 32],
            profile_digest: [13; 32],
            tpm_public_digest: [14; 32],
            upload_handle: 18,
            profile_len: 7,
        }),
        TypedRequest::ConsumeStagedRelayProfile(ConsumeStagedRelayProfileRequest {
            context: context(ConsumeStagedRelayProfile),
            generation: 15,
            policy_epoch: 16,
            profile_digest: [13; 32],
        }),
        TypedRequest::CommitRelayGeneration(CommitRelayGenerationRequest {
            context: context(CommitRelayGeneration),
            generation: 15,
            policy_epoch: 16,
            profile_digest: [13; 32],
        }),
        TypedRequest::AbortRelayEnrollment(AbortRelayEnrollmentRequest {
            context: context(AbortRelayEnrollment),
            generation: 15,
        }),
        TypedRequest::GetRelayActivePublicKey(GetRelayActivePublicKeyRequest {
            context: context(GetRelayActivePublicKey),
        }),
        TypedRequest::SignTls13ClientCertificateVerify(SignTls13ClientCertificateVerifyRequest {
            context: context(SignTls13ClientCertificateVerify),
            transcript_hash: [15; 32],
            relay_generation: 15,
            active_profile_digest: [13; 32],
            public_request_id: 17,
        }),
        TypedRequest::BeginRelayProfileUpload(BeginRelayProfileUploadRequest {
            context: context(BeginRelayProfileUpload),
            upload_handle: 18,
            generation: 15,
            policy_epoch: 16,
            pending_slot: 1,
            pending_spki_digest: [12; 32],
            profile_digest: [13; 32],
            tpm_public_digest: [14; 32],
            profile_len: 7,
        }),
        TypedRequest::WriteRelayProfileChunk(WriteRelayProfileChunkRequest {
            context: context(WriteRelayProfileChunk),
            upload_handle: 18,
            chunk_index: 0,
            chunk: Bounded::from_slice(b"profile").unwrap(),
        }),
    ];
    for request in &mut values {
        let digest = request.canonical_body_digest();
        request.context_mut().payload_digest = digest;
    }
    values
}
