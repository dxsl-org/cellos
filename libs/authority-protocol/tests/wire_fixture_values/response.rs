use authority_protocol::*;

fn binding(operation: Operation) -> AuthenticatedBinding {
    AuthenticatedBinding {
        device_id: [1; 32],
        authority_id: [2; 32],
        boot_epoch: 3,
        request_id: 6,
        operation,
        payload_digest: [0; 32],
        authority_signature: [8; 64],
    }
}

pub fn responses() -> [TypedResponse; 14] {
    use Operation::*;
    let mut values = [
        TypedResponse::OpenBoot(OpenBootResponse {
            binding: binding(OpenBoot),
            boot_epoch: 3,
            state_epoch: 4,
            approved_loader_digest: [9; 32],
        }),
        TypedResponse::ReadCommittedRelayState(ReadCommittedRelayStateResponse {
            binding: binding(ReadCommittedRelayState),
            generation: 15,
            policy_epoch: 16,
            profile_digest: [13; 32],
        }),
        TypedResponse::RequestSignedTime(RequestSignedTimeResponse {
            binding: binding(RequestSignedTime),
            time_request_id: [10; 16],
            purpose: 1,
            nonce: [11; 32],
        }),
        TypedResponse::AcceptSignedTime(AcceptSignedTimeResponse {
            binding: binding(AcceptSignedTime),
            time_request_id: [10; 16],
            purpose: 1,
            source_epoch: 11,
            source_sequence: 12,
            expires_at: 1060,
        }),
        TypedResponse::BeginRelayEnrollment(BeginRelayEnrollmentResponse {
            binding: binding(BeginRelayEnrollment),
            generation: 15,
            policy_epoch: 16,
            pending_slot: 1,
            csr_handle: 13,
            csr_len: 100,
            csr_digest: [15; 32],
        }),
        TypedResponse::ReadRelayCsrChunk(ReadRelayCsrChunkResponse {
            binding: binding(ReadRelayCsrChunk),
            chunk_index: 14,
            chunk: Bounded::from_slice(b"csr").unwrap(),
        }),
        TypedResponse::ValidateAndStageRelayProfile(ValidateAndStageRelayProfileResponse {
            binding: binding(ValidateAndStageRelayProfile),
            receipt: StagedProfileReceipt {
                device_id: [1; 32],
                authority_id: [2; 32],
                authority_epoch: 4,
                generation: 15,
                policy_epoch: 16,
                pending_slot: 1,
                pending_spki_digest: [12; 32],
                profile_digest: [13; 32],
                boot_epoch: 3,
                validation_request_id: 6,
                upload_handle: 18,
                profile_len: 7,
            },
        }),
        TypedResponse::ConsumeStagedRelayProfile(ConsumeStagedRelayProfileResponse {
            binding: binding(ConsumeStagedRelayProfile),
            generation: 15,
        }),
        TypedResponse::CommitRelayGeneration(CommitRelayGenerationResponse {
            binding: binding(CommitRelayGeneration),
            generation: 15,
            policy_epoch: 16,
            profile_digest: [13; 32],
        }),
        TypedResponse::AbortRelayEnrollment(AbortRelayEnrollmentResponse {
            binding: binding(AbortRelayEnrollment),
            generation: 15,
        }),
        TypedResponse::GetRelayActivePublicKey(GetRelayActivePublicKeyResponse {
            binding: binding(GetRelayActivePublicKey),
            generation: 15,
            public_key: {
                let mut key = [17; 65];
                key[0] = 4;
                key
            },
            public_key_digest: [12; 32],
        }),
        TypedResponse::SignTls13ClientCertificateVerify(SignTls13ClientCertificateVerifyResponse {
            binding: binding(SignTls13ClientCertificateVerify),
            signature: [18; 64],
        }),
        TypedResponse::BeginRelayProfileUpload(BeginRelayProfileUploadResponse {
            binding: binding(BeginRelayProfileUpload),
            upload_handle: 18,
            profile_len: 7,
            chunk_size: PROFILE_CHUNK_MAX as u16,
            next_index: 0,
        }),
        TypedResponse::WriteRelayProfileChunk(WriteRelayProfileChunkResponse {
            binding: binding(WriteRelayProfileChunk),
            upload_handle: 18,
            next_index: 1,
            complete: 1,
        }),
    ];
    for response in &mut values {
        let digest = response.canonical_body_digest();
        response.binding_mut().payload_digest = digest;
    }
    values
}
