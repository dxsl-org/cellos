//! Typed operation payloads; no opaque generic operation exists.

mod response;
pub use response::*;

use crate::{constant_time_eq, AuthorityFault, Bounded, FrameHeader, Operation, FRAME_MAX_PAYLOAD};

pub const ID_LEN: usize = 32;
pub const DIGEST_LEN: usize = 32;
pub const AUTHENTICATOR_LEN: usize = 32;
pub const REQUEST_AUTH_INPUT_LEN: usize = 153;
pub const SIGNATURE_LEN: usize = 64;
pub const RESPONSE_AUTH_INPUT_LEN: usize = 113;
pub const RESPONSE_BINDING_WIRE_LEN: usize = 177;
pub const HOSTNAME_MAX: usize = 64;
pub const PROFILE_MAX: usize = 768;
pub const CSR_CHUNK_MAX: usize = 104;
pub const TLS_SIGNATURE_MAX: usize = 72;

/// Untrusted request fields. Call [`verify_request_context`] before any state
/// transition; [`ValidatedRequestContext`] cannot be constructed by callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestContext {
    pub device_id: [u8; ID_LEN],
    pub authority_id: [u8; ID_LEN],
    pub boot_epoch: u64,
    pub sequence: u64,
    pub challenge: [u8; DIGEST_LEN],
    pub request_id: u64,
    pub operation: Operation,
    pub payload_digest: [u8; DIGEST_LEN],
    pub authenticator: [u8; AUTHENTICATOR_LEN],
}

impl RequestContext {
    /// Return the canonical bytes that a protected adapter must authenticate.
    pub fn authentication_input(&self) -> [u8; REQUEST_AUTH_INPUT_LEN] {
        let mut out = [0u8; REQUEST_AUTH_INPUT_LEN];
        out[..32].copy_from_slice(&self.device_id);
        out[32..64].copy_from_slice(&self.authority_id);
        out[64..72].copy_from_slice(&self.boot_epoch.to_le_bytes());
        out[72..80].copy_from_slice(&self.sequence.to_le_bytes());
        out[80..112].copy_from_slice(&self.challenge);
        out[112..120].copy_from_slice(&self.request_id.to_le_bytes());
        out[120] = self.operation as u8;
        out[121..153].copy_from_slice(&self.payload_digest);
        out
    }
}

/// Injected MAC/signature verifier owned by the protected authority adapter.
pub trait RequestAuthenticator {
    /// Verify the canonical authentication input and its fixed authenticator.
    fn verify(
        &self,
        input: &[u8; REQUEST_AUTH_INPUT_LEN],
        authenticator: &[u8; AUTHENTICATOR_LEN],
    ) -> bool;
}

/// Closed typed request contract used by the authenticated verifier.
pub trait AuthorityRequest: Copy {
    const OPERATION: Operation;
    fn canonical_body(&self) -> ([u8; DIGEST_LEN], usize);
    fn canonical_body_digest(&self) -> [u8; DIGEST_LEN] {
        self.canonical_body().0
    }
    fn canonical_payload_len(&self) -> usize {
        self.canonical_body().1
    }
    fn context(&self) -> &RequestContext;
    fn context_mut(&mut self) -> &mut RequestContext;
}

/// A typed request whose header, operation, payload digest, and authenticator
/// were verified together. Its inner request is unavailable to carriers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedRequest<T: AuthorityRequest>(T);

impl<T: AuthorityRequest> ValidatedRequest<T> {
    pub(crate) const fn request(&self) -> &T {
        &self.0
    }
}

/// Verify a decoded typed request before it can reach state transitions.
pub fn verify_typed_request<T: AuthorityRequest, P: RequestAuthenticator>(
    request: T,
    header: &FrameHeader,
    policy: &P,
) -> Result<ValidatedRequest<T>, AuthorityFault> {
    let context = request.context();
    if header.class != crate::FrameClass::Request
        || T::OPERATION != header.operation
        || context.operation != T::OPERATION
        || context.request_id != header.request_id
    {
        return Err(AuthorityFault::ChallengeMismatch);
    }
    let (body_digest, payload_len) = request.canonical_body();
    if header.payload_len as usize != payload_len
        || !constant_time_eq(&context.payload_digest, &body_digest)
        || !constant_time_eq(&context.authenticator[..16], &header.authenticator)
        || !policy.verify(&context.authentication_input(), &context.authenticator)
    {
        return Err(AuthorityFault::ChallengeMismatch);
    }
    Ok(ValidatedRequest(request))
}

/// Authority-signed response binding. The signature covers every preceding
/// field plus the operation-specific response payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthenticatedBinding {
    pub device_id: [u8; ID_LEN],
    pub authority_id: [u8; ID_LEN],
    pub boot_epoch: u64,
    pub request_id: u64,
    pub operation: Operation,
    pub payload_digest: [u8; DIGEST_LEN],
    pub authority_signature: [u8; SIGNATURE_LEN],
}
impl AuthenticatedBinding {
    pub fn authentication_input(&self) -> [u8; RESPONSE_AUTH_INPUT_LEN] {
        let mut output = [0u8; RESPONSE_AUTH_INPUT_LEN];
        output[..32].copy_from_slice(&self.device_id);
        output[32..64].copy_from_slice(&self.authority_id);
        output[64..72].copy_from_slice(&self.boot_epoch.to_le_bytes());
        output[72..80].copy_from_slice(&self.request_id.to_le_bytes());
        output[80] = self.operation as u8;
        output[81..113].copy_from_slice(&self.payload_digest);
        output
    }
}

pub trait ResponseAuthenticator {
    fn verify(
        &self,
        input: &[u8; RESPONSE_AUTH_INPUT_LEN],
        signature: &[u8; SIGNATURE_LEN],
    ) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedResponseBinding {
    pub device_id: [u8; ID_LEN],
    pub authority_id: [u8; ID_LEN],
    pub boot_epoch: u64,
    pub request_id: u64,
    pub operation: Operation,
}

macro_rules! request_types {
    ($( $name:ident => $operation:ident { $( $field:ident : $ty:ty ),* $(,)? } ),+ $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name {
            pub context: RequestContext,
            $(pub $field: $ty,)*
        }
        impl AuthorityRequest for $name {
            const OPERATION: Operation = Operation::$operation;
            fn context(&self) -> &RequestContext { &self.context }
            fn context_mut(&mut self) -> &mut RequestContext { &mut self.context }
            fn canonical_body(&self) -> ([u8; DIGEST_LEN], usize) {
                <Self as crate::wire::CanonicalBody>::canonical_body(self)
            }
        }
    )+};
}

request_types! {
    OpenBootRequest => OpenBoot { loader_digest: [u8; DIGEST_LEN] },
    ReadCommittedRelayStateRequest => ReadCommittedRelayState {},
    RequestSignedTimeRequest => RequestSignedTime { purpose: u8 },
    AcceptSignedTimeRequest => AcceptSignedTime { time_request_id: [u8; 16], purpose: u8, source_epoch: u64, source_sequence: u64, unix_seconds: i64, expires_at: u64, nonce: [u8; DIGEST_LEN], source_signature: Bounded<TLS_SIGNATURE_MAX> },
    BeginRelayEnrollmentRequest => BeginRelayEnrollment { hostname: Bounded<HOSTNAME_MAX> },
    ReadRelayCsrChunkRequest => ReadRelayCsrChunk { csr_handle: u64, chunk_index: u32 },
    ValidateAndStageRelayProfileRequest => ValidateAndStageRelayProfile { generation: u64, policy_epoch: u64, pending_slot: u8, pending_spki_digest: [u8; DIGEST_LEN], profile_digest: [u8; DIGEST_LEN], tpm_public_digest: [u8; DIGEST_LEN], profile: Bounded<PROFILE_MAX> },
    ConsumeStagedRelayProfileRequest => ConsumeStagedRelayProfile { generation: u64, policy_epoch: u64, profile_digest: [u8; DIGEST_LEN] },
    CommitRelayGenerationRequest => CommitRelayGeneration { generation: u64, policy_epoch: u64, profile_digest: [u8; DIGEST_LEN] },
    AbortRelayEnrollmentRequest => AbortRelayEnrollment { generation: u64 },
    GetRelayActivePublicKeyRequest => GetRelayActivePublicKey {},
    SignTls13ClientCertificateVerifyRequest => SignTls13ClientCertificateVerify { transcript_hash: [u8; DIGEST_LEN], relay_generation: u64, active_profile_digest: [u8; DIGEST_LEN], public_request_id: u64 },
}

pub const REQUEST_CONTEXT_WIRE_LEN: usize = 185;

/// Maximum canonical request payload for an operation.
pub const fn max_payload_len(operation: Operation) -> usize {
    match operation {
        Operation::ValidateAndStageRelayProfile => {
            REQUEST_CONTEXT_WIRE_LEN + 8 + 8 + 1 + (DIGEST_LEN * 3) + 2 + PROFILE_MAX
        }
        Operation::AcceptSignedTime => {
            REQUEST_CONTEXT_WIRE_LEN + 16 + 1 + 32 + DIGEST_LEN + 2 + TLS_SIGNATURE_MAX
        }
        Operation::BeginRelayEnrollment => REQUEST_CONTEXT_WIRE_LEN + 2 + HOSTNAME_MAX,
        _ => REQUEST_CONTEXT_WIRE_LEN + 160,
    }
}

const _: () =
    assert!(max_payload_len(Operation::ValidateAndStageRelayProfile) <= FRAME_MAX_PAYLOAD);
