use super::{AuthenticatedBinding, CSR_CHUNK_MAX, DIGEST_LEN, SIGNATURE_LEN};
use crate::Bounded;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StagedProfileReceipt {
    pub device_id: [u8; super::ID_LEN],
    pub authority_id: [u8; super::ID_LEN],
    pub authority_epoch: u64,
    pub generation: u64,
    pub policy_epoch: u64,
    pub pending_slot: u8,
    pub pending_spki_digest: [u8; DIGEST_LEN],
    pub profile_digest: [u8; DIGEST_LEN],
    pub boot_epoch: u64,
    pub upload_handle: u64,
    pub profile_len: u32,
    pub validation_request_id: u64,
}

macro_rules! response_types {
    ($( $name:ident { $( $field:ident : $ty:ty ),* $(,)? } ),+ $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name {
            pub binding: AuthenticatedBinding,
            $(pub $field: $ty,)*
        }
    )+};
}

response_types! {
    OpenBootResponse { boot_epoch: u64, state_epoch: u64, approved_loader_digest: [u8; DIGEST_LEN] },
    ReadCommittedRelayStateResponse { generation: u64, policy_epoch: u64, profile_digest: [u8; DIGEST_LEN] },
    RequestSignedTimeResponse { time_request_id: [u8; 16], purpose: u8, nonce: [u8; DIGEST_LEN] },
    AcceptSignedTimeResponse { time_request_id: [u8; 16], purpose: u8, source_epoch: u64, source_sequence: u64, expires_at: u64 },
    BeginRelayEnrollmentResponse { generation: u64, policy_epoch: u64, pending_slot: u8, csr_handle: u64, csr_len: u32, csr_digest: [u8; DIGEST_LEN] },
    ReadRelayCsrChunkResponse { chunk_index: u32, chunk: Bounded<CSR_CHUNK_MAX> },
    ValidateAndStageRelayProfileResponse { receipt: StagedProfileReceipt },
    ConsumeStagedRelayProfileResponse { generation: u64 },
    CommitRelayGenerationResponse { generation: u64, policy_epoch: u64, profile_digest: [u8; DIGEST_LEN] },
    AbortRelayEnrollmentResponse { generation: u64 },
    GetRelayActivePublicKeyResponse { generation: u64, public_key: [u8; 65], public_key_digest: [u8; DIGEST_LEN] },
    SignTls13ClientCertificateVerifyResponse { signature: [u8; SIGNATURE_LEN] },
    BeginRelayProfileUploadResponse { upload_handle: u64, profile_len: u32, chunk_size: u16, next_index: u8 },
    WriteRelayProfileChunkResponse { upload_handle: u64, next_index: u8, complete: u8 },
}
