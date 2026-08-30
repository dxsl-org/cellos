#![allow(dead_code)]
#[path = "support/relay.rs"]
mod relay;
#[path = "support/time.rs"]
mod time;
use authority_protocol::*;
#[allow(unused_imports)]
pub use relay::*;
#[allow(unused_imports)]
pub use time::*;

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

impl MemoryStore {
    pub fn into_record(self) -> Option<ProtectedAuthorityRecord> {
        self.record
    }
    pub fn from_record(record: ProtectedAuthorityRecord) -> Self {
        Self {
            revision: record.revision(),
            record: Some(record),
        }
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
    fn verify_root_profile(&self, admitted: &AdmittedProfileValidation) -> bool {
        let request = admitted.request();
        request.profile_len == 32
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

pub fn state(boot: u64, generation: u64) -> TestState {
    AuthorityState::new(
        MemoryStore::default(),
        AuthorityStateConfig {
            device_id: [1; 32],
            authority_id: [2; 32],
            authority_epoch: 1,
            boot_floor: boot,
            generation_floor: generation,
            state_epoch: 0,
            boot_challenge: [3; 32],
            time_floors: floors(),
        },
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
