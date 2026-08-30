use crate::*;
use authority_protocol::*;
use sha2::{Digest, Sha256};
use std::vec::Vec;

#[derive(Clone, Copy)]
pub struct TestAuth;
impl RecordAuthenticator for TestAuth {
    fn authenticate(&self, message: &[u8]) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"SOFTWARE_HARNESS journal key");
        hash.update(message);
        hash.finalize().into()
    }
}

#[derive(Default)]
struct CaptureStore(Option<ProtectedAuthorityRecord>);
impl ProtectedStore for CaptureStore {
    fn compare_and_swap(&mut self, expected: u64, next: &ProtectedAuthorityRecord) -> bool {
        if expected != 0 || next.revision() != 1 {
            return false;
        }
        self.0 = Some(*next);
        true
    }
    fn seal_on_conflict(&mut self, _: u64) {}
}

struct AllowRequest;
impl RequestAuthenticator for AllowRequest {
    fn verify(&self, _: &[u8; REQUEST_AUTH_INPUT_LEN], _: &[u8; 32]) -> bool {
        true
    }
}
struct AllowBoot;
impl BootMeasurementVerifier for AllowBoot {
    fn verify_boot_measurement(&self, value: &[u8; 32]) -> bool {
        value == &[7; 32]
    }
}

pub fn full_record(role: SlotRole) -> FullRecord {
    let mut state = AuthorityState::new(
        CaptureStore::default(),
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
    let mut request = OpenBootRequest {
        context: RequestContext {
            device_id: [1; 32],
            authority_id: [2; 32],
            boot_epoch: 0,
            sequence: 1,
            challenge: [3; 32],
            request_id: 10,
            operation: Operation::OpenBoot,
            payload_digest: [0; 32],
            authenticator: [0; 32],
        },
        loader_digest: [7; 32],
    };
    request.context.payload_digest = request.canonical_body_digest();
    let header = FrameHeader {
        class: FrameClass::Request,
        operation: Operation::OpenBoot,
        payload_len: request.canonical_payload_len() as u16,
        request_id: 10,
        authenticator: [0; 16],
    };
    let request = verify_typed_request(request, &header, &AllowRequest).unwrap();
    let measurement = verify_boot_measurement([7; 32], &AllowBoot).unwrap();
    state.open_boot(&request, &measurement).unwrap();
    let protected = state.into_store().0.unwrap();
    FullRecord {
        counter: 1,
        slot_role: role,
        hardware: hardware(),
        protected,
        active: None,
        pending: None,
    }
}

pub fn hardware() -> HardwareBindings {
    HardwareBindings {
        lane_id: [4; 32],
        restart_floor: 1,
        approved_boot_measurement: [5; 32],
        approved_loader_digest: [7; 32],
        manifest_key_digest: [6; 32],
        firmware_floor: 1,
        policy_floor: 1,
        trust_digest: [8; 32],
        verifier_digest: [9; 32],
        denylist_digest: [10; 32],
        qualification_digest: [11; 32],
    }
}

pub fn identity() -> ExpectedIdentity {
    ExpectedIdentity {
        device_id: [1; 32],
        authority_id: [2; 32],
        lane_id: [4; 32],
    }
}

pub fn encoded(role: SlotRole) -> Vec<u8> {
    let mut output = [0u8; RECORD_MAX];
    let length = encode_record(&full_record(role), &TestAuth, &mut output).unwrap();
    output[..length].to_vec()
}

pub fn successor(previous: &FullRecord) -> FullRecord {
    let mut bytes = [0u8; PROTECTED_RECORD_MAX];
    let length = previous.protected.encode_canonical(&mut bytes).unwrap();
    bytes[5..13].copy_from_slice(&2u64.to_le_bytes());
    let protected = ProtectedAuthorityRecord::decode_canonical(&bytes[..length]).unwrap();
    let mut next = previous.clone();
    next.counter = 2;
    next.slot_role = previous.slot_role.other();
    next.protected = protected;
    next
}

pub fn record_at(counter: u64, role: SlotRole) -> FullRecord {
    let base = full_record(role);
    let mut bytes = [0u8; PROTECTED_RECORD_MAX];
    let length = base.protected.encode_canonical(&mut bytes).unwrap();
    bytes[5..13].copy_from_slice(&counter.to_le_bytes());
    let protected = ProtectedAuthorityRecord::decode_canonical(&bytes[..length]).unwrap();
    FullRecord {
        counter,
        protected,
        ..base
    }
}

pub fn encode_full(record: &FullRecord) -> Vec<u8> {
    let mut output = [0u8; RECORD_MAX];
    let length = encode_record(record, &TestAuth, &mut output).unwrap();
    output[..length].to_vec()
}

pub fn pending_record() -> FullRecord {
    let base = full_record(SlotRole::A);
    let mut fixed = [0u8; PROTECTED_RECORD_MAX];
    let length = base.protected.encode_canonical(&mut fixed).unwrap();
    let mut bytes = fixed[..length].to_vec();
    bytes[5..13].copy_from_slice(&2u64.to_le_bytes());
    let mut relay = Vec::new();
    relay.push(1);
    relay.extend_from_slice(&1u64.to_le_bytes());
    relay.extend_from_slice(&1u64.to_le_bytes());
    relay.push(0);
    let shift = relay.len() - 1;
    bytes.splice(24..25, relay);
    bytes[105 + shift..113 + shift].copy_from_slice(&1u64.to_le_bytes());
    let protected = ProtectedAuthorityRecord::decode_canonical(&bytes).unwrap();
    FullRecord {
        counter: 2,
        slot_role: SlotRole::B,
        protected,
        pending: Some(ProfileMaterial {
            device_id: [1; 32],
            authority_id: [2; 32],
            authority_epoch: 1,
            boot_epoch: 1,
            slot: 0,
            generation: 1,
            profile_len: 0,
            profile_digest: [0; 32],
            tpm_public_digest: [13; 32],
            spki: Bounded::from_slice(b"pending-spki").unwrap(),
        }),
        ..base
    }
}
