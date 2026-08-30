mod support;
use authority_protocol::*;
use support::*;

#[derive(Default)]
struct CapturingStore {
    revision: u64,
    record: Option<ProtectedAuthorityRecord>,
    sealed: bool,
}

impl ProtectedStore for CapturingStore {
    fn compare_and_swap(&mut self, expected: u64, next: &ProtectedAuthorityRecord) -> bool {
        if self.revision != expected || next.revision() != expected + 1 {
            return false;
        }
        self.revision = next.revision();
        self.record = Some(*next);
        true
    }

    fn seal_on_conflict(&mut self, _: u64) {
        self.sealed = true;
    }
}

struct RecordPolicy([u8; 32]);
impl ProtectedRecordVerifier for RecordPolicy {
    fn verify(&self, record: &ProtectedAuthorityRecord) -> bool {
        constant_time_eq(&self.0, &record.authentication_digest())
    }
}

#[test]
fn persisted_record_authenticates_and_restores_with_fresh_boot_state() {
    let mut authority = AuthorityState::new(
        CapturingStore::default(),
        AuthorityStateConfig {
            device_id: [1; 32],
            authority_id: [2; 32],
            authority_epoch: 1,
            boot_floor: 0,
            generation_floor: 0,
            state_epoch: 0,
            boot_challenge: [3; 32],
            time_floors: floors(),
        },
    );
    assert_eq!(authority.open_boot(&open(1), &measurement()), Ok(1));
    let stored = authority.into_store();
    let record = stored.record.unwrap();
    let digest = record.authentication_digest();
    let mut encoded = [0u8; PROTECTED_RECORD_MAX];
    let length = record.encode_canonical(&mut encoded).unwrap();
    assert_eq!(
        ProtectedAuthorityRecord::decode_canonical(&encoded[..length]),
        Ok(record)
    );
    assert_eq!(
        ProtectedAuthorityRecord::decode_canonical(&encoded[..length - 1]),
        Err(WireError::Truncated)
    );
    encoded[13] = 0xff;
    assert_eq!(
        ProtectedAuthorityRecord::decode_canonical(&encoded[..length]),
        Err(WireError::UnknownMessageKind)
    );
    assert_ne!(digest, [0; 32]);
    encoded[13] = 1;
    let impossible = ProtectedAuthorityRecord::decode_canonical(&encoded[..length]).unwrap();
    assert_eq!(
        verify_protected_record(
            impossible,
            &RecordPolicy(impossible.authentication_digest()),
        ),
        Err(AuthorityFault::PersistenceFailure)
    );
    assert!(verify_protected_record(record, &RecordPolicy(digest)).is_ok());
    assert_eq!(
        verify_protected_record(record, &RecordPolicy([0; 32])),
        Err(AuthorityFault::PersistenceFailure)
    );

    let verified = verify_protected_record(record, &RecordPolicy(digest)).unwrap();
    let restored = AuthorityState::restore(
        CapturingStore {
            revision: record.revision(),
            ..CapturingStore::default()
        },
        &verified,
        [4; 32],
    );
    assert_eq!(restored.mode(), AuthorityMode::Ready);
    assert_eq!(restored.boot_state(), BootState::Closed);
    assert_eq!(restored.time_state(), TimeState::Unavailable);
}

#[test]
fn cas_conflict_seals_external_counter_domain() {
    let store = CapturingStore {
        revision: 1,
        ..CapturingStore::default()
    };
    let mut authority = AuthorityState::new(
        store,
        AuthorityStateConfig {
            device_id: [1; 32],
            authority_id: [2; 32],
            authority_epoch: 1,
            boot_floor: 0,
            generation_floor: 0,
            state_epoch: 0,
            boot_challenge: [3; 32],
            time_floors: floors(),
        },
    );
    assert_eq!(
        authority.open_boot(&open(1), &measurement()),
        Err(AuthorityFault::PersistenceFailure)
    );
    assert_eq!(authority.mode(), AuthorityMode::Sealed);
    assert!(authority.into_store().sealed);
}

#[test]
fn persisted_sealed_mode_cannot_restore_as_ready() {
    let mut authority = AuthorityState::new(
        CapturingStore::default(),
        AuthorityStateConfig {
            device_id: [1; 32],
            authority_id: [2; 32],
            authority_epoch: 1,
            boot_floor: 0,
            generation_floor: 0,
            state_epoch: 0,
            boot_challenge: [3; 32],
            time_floors: floors(),
        },
    );
    let mut request = OpenBootRequest {
        context: context(1, 0, Operation::OpenBoot),
        loader_digest: [7; 32],
    };
    request.context.device_id = [9; 32];
    let request = validated(request);
    assert_eq!(
        authority.open_boot(&request, &measurement()),
        Err(AuthorityFault::IdentityMismatch)
    );
    let store = authority.into_store();
    let record = store.record.unwrap();
    let verified =
        verify_protected_record(record, &RecordPolicy(record.authentication_digest())).unwrap();
    let restored = AuthorityState::restore(
        CapturingStore {
            revision: record.revision(),
            ..CapturingStore::default()
        },
        &verified,
        [4; 32],
    );
    assert_eq!(restored.mode(), AuthorityMode::Sealed);
}

#[test]
fn protected_v1_records_are_rejected_after_clean_cutover() {
    let mut authority = state(0, 0);
    authority.open_boot(&open(1), &measurement()).unwrap();
    let record = authority.into_store().into_record().unwrap();
    let mut encoded = [0u8; PROTECTED_RECORD_MAX];
    let length = record.encode_canonical(&mut encoded).unwrap();
    encoded[4] = 1;
    assert_eq!(
        ProtectedAuthorityRecord::decode_canonical(&encoded[..length]),
        Err(WireError::UnsupportedVersion)
    );
}
