mod adapter_tests;
mod fixture;
mod path_rejection;
mod pending_rejection;
mod policy_rejection;

use std::{vec, vec::Vec};

use super::*;
use fixture::{admitted_snapshot, chain, snapshot, Fixture};

struct Reader {
    first: Vec<u8>,
    second: Option<Vec<u8>>,
    calls: usize,
}
impl PendingPublicReader for Reader {
    fn read_public(
        &mut self,
        _: PendingPublicRequest,
        out: &mut [u8],
    ) -> Result<usize, PublicReadError> {
        let value = if self.calls == 0 {
            &self.first
        } else {
            self.second.as_ref().unwrap_or(&self.first)
        };
        self.calls += 1;
        if value.len() > out.len() {
            return Err(PublicReadError::BufferTooSmall);
        }
        out[..value.len()].copy_from_slice(value);
        Ok(value.len())
    }
}

fn policy<'a>(
    fixture: &'a Fixture,
    nodes: &'a [[u8; 32]],
    serials: &'a [DeniedSerial<'a>],
) -> TrustedPolicy<'a> {
    TrustedPolicy {
        trust_anchor_der: &fixture.root,
        expected_dns_name: b"node.example",
        signed_time_unix: 1_735_689_600,
        expected_slot: 0,
        expected_generation: 1,
        expected_policy_epoch: 3,
        denied_node_ids: nodes,
        denied_serials: serials,
        expected_journal_revision: 0,
    }
}
fn run<'a>(
    fixture: &'a Fixture,
    pending: PendingEnrollmentSnapshot,
    mut policy: TrustedPolicy<'a>,
    second: Option<Vec<u8>>,
) -> Result<ValidatedProfileMetadata<'a>, Error> {
    if policy.expected_journal_revision == 0 {
        policy.expected_journal_revision = pending.journal_revision();
    }
    let mut reader = Reader {
        first: fixture.tpm.clone(),
        second,
        calls: 0,
    };
    validate_profile_core(&fixture.profile, policy, &pending, &mut reader)
}

#[test]
fn direct_and_one_and_two_intermediate_chains_validate() {
    for depth in 0..=2 {
        let fixture = chain(depth);
        let pending = snapshot(&fixture);
        let result = run(&fixture, pending, policy(&fixture, &[], &[]), None).unwrap();
        assert_eq!(result.profile_len(), fixture.profile.len() as u32);
        assert_eq!(result.spki_digest(), &fixture.spki);
        assert_eq!(result.node_id(), &fixture.node);
        assert_eq!(result.serial(), &[1]);
    }
}

#[test]
fn public_api_requires_matching_admission_and_bank_gated_snapshot() {
    let fixture = chain(0);
    let (admitted, pending) = admitted_snapshot(&fixture);
    let mut trusted = policy(&fixture, &[], &[]);
    trusted.expected_journal_revision = pending.journal_revision();
    let mut reader = Reader {
        first: fixture.tpm.clone(),
        second: None,
        calls: 0,
    };
    let result =
        validate_profile(&admitted, &fixture.profile, trusted, &pending, &mut reader).unwrap();
    assert_eq!(result.profile_digest(), pending.profile_digest());

    let other = chain(1);
    let (_, mismatched) = admitted_snapshot(&other);
    trusted.expected_journal_revision = mismatched.journal_revision();
    let mut reader = Reader {
        first: fixture.tpm.clone(),
        second: None,
        calls: 0,
    };
    assert_eq!(
        validate_profile(
            &admitted,
            &fixture.profile,
            trusted,
            &mismatched,
            &mut reader,
        ),
        Err(Error::StaleSnapshot)
    );
}

#[test]
fn generalized_time_before_2050_is_noncanonical() {
    let value = b"20491231235959Z";
    let element = der::Element {
        tag: 0x18,
        full: value,
        value,
    };
    assert_eq!(time::parse(element), Err(Error::InvalidValidity));
}

#[test]
fn cross_domain_revision_and_csr_snapshots_are_rejected() {
    let fixture = chain(0);
    let snapshot = snapshot(&fixture);
    let mut trusted = policy(&fixture, &[], &[]);
    trusted.expected_journal_revision = snapshot.journal_revision();
    let baseline = admission::AdmissionBinding {
        device_id: *snapshot.device_id(),
        authority_id: *snapshot.authority_id(),
        authority_epoch: snapshot.authority_epoch(),
        boot_epoch: snapshot.boot_epoch(),
        csr_handle: snapshot.csr_handle(),
        slot: snapshot.pending_slot(),
        generation: snapshot.generation(),
        policy_epoch: snapshot.policy_epoch(),
        upload_handle: snapshot.upload_handle(),
        profile_len: snapshot.profile_len(),
    };
    assert!(admission::matches(&snapshot, trusted, baseline));
    let mut cases = [baseline; 5];
    cases[0].device_id[0] ^= 1;
    cases[1].authority_id[0] ^= 1;
    cases[2].authority_epoch += 1;
    cases[3].boot_epoch += 1;
    cases[4].csr_handle += 1;
    assert!(cases
        .into_iter()
        .all(|binding| !admission::matches(&snapshot, trusted, binding)));
    let mut stale = trusted;
    stale.expected_journal_revision += 1;
    assert!(!admission::matches(&snapshot, stale, baseline));
}
