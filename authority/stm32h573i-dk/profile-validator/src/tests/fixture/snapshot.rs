use super::{snapshot_flow::complete_upload, snapshot_support::Auth, Fixture};
use authority_protocol::{AdmittedProfileValidation, Bounded};
use sha2::{Digest, Sha256};
use stm32_authority_journal::*;

pub fn snapshot(fixture: &Fixture) -> PendingEnrollmentSnapshot {
    admitted_snapshot(fixture).1
}

pub fn admitted_snapshot(
    fixture: &Fixture,
) -> (AdmittedProfileValidation, PendingEnrollmentSnapshot) {
    let (prior_protected, protected, storage, admitted) = complete_upload(fixture);
    let revision = protected.revision();
    let pending = ProfileMaterial {
        device_id: [1; 32],
        authority_id: [2; 32],
        authority_epoch: 1,
        boot_epoch: 1,
        slot: 0,
        generation: 1,
        profile_len: 0,
        profile_digest: [0; 32],
        tpm_public_digest: Sha256::digest(&fixture.tpm).into(),
        spki: Bounded::from_slice(&fixture.spki_der).unwrap(),
    };
    let current = FullRecord {
        counter: revision,
        slot_role: SlotRole::A,
        hardware: hardware(),
        protected,
        active: None,
        pending: Some(pending.clone()),
    };
    let prior = FullRecord {
        counter: revision - 1,
        slot_role: SlotRole::B,
        hardware: hardware(),
        protected: prior_protected,
        active: None,
        pending: Some(pending),
    };
    let mut a = [0u8; RECORD_MAX];
    let a_len = encode_record(&current, &Auth, &mut a).unwrap();
    let mut b = [0u8; RECORD_MAX];
    let b_len = encode_record(&prior, &Auth, &mut b).unwrap();
    let expected = ExpectedIdentity {
        device_id: [1; 32],
        authority_id: [2; 32],
        lane_id: [4; 32],
    };
    let token = recover(
        revision,
        [Some(&a[..a_len]), Some(&b[..b_len])],
        &Auth,
        &expected,
    )
    .unwrap();
    let snapshot = ProfileBank::new(storage, Auth)
        .validate_recovered(token)
        .unwrap()
        .pending_enrollment_snapshot()
        .unwrap();
    (admitted, snapshot)
}

fn hardware() -> HardwareBindings {
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
