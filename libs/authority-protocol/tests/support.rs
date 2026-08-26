#![allow(dead_code)]
#[path = "support/relay.rs"]
mod relay;
use authority_protocol::*;
#[allow(unused_imports)]
pub use relay::*;

#[derive(Default)]
pub struct MemoryStore {
    revision: u64,
    record: Option<ProtectedAuthorityRecord>,
}
impl ProtectedStore for MemoryStore {
    fn compare_and_swap(
        &mut self,
        expected_revision: u64,
        next: &ProtectedAuthorityRecord,
    ) -> bool {
        if self.revision != expected_revision || next.revision() != expected_revision + 1 {
            return false;
        }
        self.revision = next.revision();
        self.record = Some(*next);
        true
    }

    fn seal_on_conflict(&mut self, _: u64) {
        self.record = None;
    }
}
pub type TestState = AuthorityState<MemoryStore>;

pub struct RequestPolicy;
impl RequestAuthenticator for RequestPolicy {
    fn verify(&self, input: &[u8; REQUEST_AUTH_INPUT_LEN], authenticator: &[u8; 32]) -> bool {
        authenticator == &test_mac(input)
    }
}

pub struct TimePolicy;
impl SignedTimeVerifier for TimePolicy {
    fn verify_signed_time(&self, request: &AcceptSignedTimeRequest) -> bool {
        request.source_signature == time_signature()
    }
}

pub struct ProfilePolicy;
impl RootProfileVerifier for ProfilePolicy {
    fn verify_root_profile(&self, request: &ValidateAndStageRelayProfileRequest) -> bool {
        request.profile.as_slice() == request.profile_digest
            && request.tpm_public_digest == [6; 32]
            && request.pending_spki_digest == [7; 32]
    }
}

pub struct BootPolicy;
impl BootMeasurementVerifier for BootPolicy {
    fn verify_boot_measurement(&self, loader_digest: &[u8; 32]) -> bool {
        loader_digest == &[7; 32]
    }
}

pub fn measurement() -> VerifiedBootMeasurement {
    verify_boot_measurement([7; 32], &BootPolicy).unwrap()
}

pub struct CasPolicy;
impl ProviderCasVerifier for CasPolicy {
    fn verify_provider_cas(&self, receipt: &ProviderCasReceipt) -> bool {
        receipt.provider_signature == [9; 64]
    }
}

pub struct Clock(pub u64);
impl TrustedClock for Clock {
    fn now_unix_seconds(&self) -> u64 {
        self.0
    }
}

pub struct Challenges(pub u8);
impl TimeChallengeSource for Challenges {
    fn generate_challenge(&mut self) -> Result<([u8; 16], [u8; 32]), AuthorityFault> {
        let id = [self.0; 16];
        let nonce = [self.0.wrapping_add(1); 32];
        self.0 = self.0.wrapping_add(2);
        Ok((id, nonce))
    }
}

pub fn floors() -> ProtectedTimeFloors {
    ProtectedTimeFloors {
        source_epoch: 0,
        source_sequence: 0,
        unix_seconds: 0,
    }
}

pub fn state(boot: u64, generation: u64) -> TestState {
    AuthorityState::new(
        MemoryStore::default(),
        [1; 32],
        [2; 32],
        1,
        boot,
        generation,
        0,
        [3; 32],
        floors(),
    )
}

pub fn test_mac(input: &[u8; REQUEST_AUTH_INPUT_LEN]) -> [u8; 32] {
    let mut output = [0x5au8; 32];
    for (index, byte) in input.iter().enumerate() {
        output[index % 32] = output[index % 32].rotate_left(1) ^ byte;
    }
    output
}

pub fn time_signature() -> Bounded<72> {
    Bounded::from_slice(&[0x30, 6, 2, 1, 1, 2, 1, 1]).unwrap()
}

pub fn context(sequence: u64, boot_epoch: u64, operation: Operation) -> RequestContext {
    let mut value = RequestContext {
        device_id: [1; 32],
        authority_id: [2; 32],
        boot_epoch,
        sequence,
        challenge: [3; 32],
        request_id: sequence + 100,
        operation,
        payload_digest: [operation as u8; 32],
        authenticator: [0; 32],
    };
    value.authenticator = test_mac(&value.authentication_input());
    value
}

pub fn header<T: AuthorityRequest>(request: &T) -> FrameHeader {
    let context = request.context();
    FrameHeader {
        class: FrameClass::Request,
        operation: T::OPERATION,
        payload_len: request.canonical_payload_len() as u16,
        request_id: context.request_id,
        authenticator: context.authenticator[..16].try_into().unwrap(),
    }
}

pub fn authenticate<T: AuthorityRequest>(request: &mut T) {
    let digest = request.canonical_body_digest();
    let context = request.context_mut();
    context.payload_digest = digest;
    context.authenticator = test_mac(&context.authentication_input());
}

pub fn validated<T: AuthorityRequest>(mut request: T) -> ValidatedRequest<T> {
    authenticate(&mut request);
    verify_typed_request(request, &header(&request), &RequestPolicy).unwrap()
}

pub fn open(sequence: u64) -> ValidatedRequest<OpenBootRequest> {
    validated(OpenBootRequest {
        context: context(sequence, 0, Operation::OpenBoot),
        loader_digest: [7; 32],
    })
}

pub fn grant_time(
    state: &mut TestState,
    challenges: &mut Challenges,
    sequence: u64,
    boot: u64,
    purpose: TimePurpose,
    source_sequence: u64,
    expires_at: u64,
) {
    let request = validated(RequestSignedTimeRequest {
        context: context(sequence, boot, Operation::RequestSignedTime),
        purpose: purpose as u8,
    });
    let challenge = state.request_signed_time(&request, challenges).unwrap();
    let mut fact = AcceptSignedTimeRequest {
        context: context(sequence + 1, boot, Operation::AcceptSignedTime),
        time_request_id: challenge.time_request_id,
        purpose: purpose as u8,
        source_epoch: 1,
        source_sequence,
        unix_seconds: 100 + source_sequence as i64,
        expires_at,
        nonce: challenge.nonce,
        source_signature: time_signature(),
    };
    authenticate(&mut fact);
    let verified = verify_signed_time(fact, &header(&fact), &RequestPolicy, &TimePolicy).unwrap();
    state.accept_time(&verified, &Clock(100)).unwrap();
}
