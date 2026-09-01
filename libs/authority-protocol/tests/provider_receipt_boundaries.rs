mod support;

use authority_protocol::*;
use support::*;

const DIGEST: [u8; 32] = [9; 32];

type Mutation = fn(&mut ProviderCasReceipt);

fn prepared_state() -> (TestState, PreparedCommitIntent) {
    let mut state = state(0, 0);
    let mut challenges = Challenges(4);
    state.open_boot(&open(1), &measurement()).unwrap();
    grant_time(
        &mut state,
        &mut challenges,
        2,
        1,
        TimePurpose::Enrollment,
        1,
        200,
    );
    state.begin_enrollment(&begin(4, 1), &Clock(101)).unwrap();
    complete_upload(&mut state, 5, 1, 1, DIGEST);
    state.consume_receipt(&consume(8, 1, 1, DIGEST)).unwrap();
    let prepared = state.prepare_commit(&commit(9, 1, 1, DIGEST)).unwrap();
    (state, prepared)
}

fn receipt(prepared: &PreparedCommitIntent) -> ProviderCasReceipt {
    let intent = prepared.intent();
    ProviderCasReceipt {
        device_id: intent.device_id,
        authority_id: intent.authority_id,
        authority_epoch: intent.authority_epoch,
        generation: intent.generation,
        policy_epoch: intent.policy_epoch,
        pending_slot: intent.pending_slot,
        pending_spki_digest: intent.pending_spki_digest,
        profile_digest: intent.profile_digest,
        boot_epoch: intent.boot_epoch,
        validation_request_id: intent.validation_request_id,
        upload_handle: intent.upload_handle,
        profile_len: intent.profile_len,
        provider_signature: [9; SIGNATURE_LEN],
    }
}

#[test]
fn provider_promotion_requires_every_exact_receipt_field() {
    let mutations: [Mutation; 12] = [
        |value| value.device_id[0] ^= 1,
        |value| value.authority_id[0] ^= 1,
        |value| value.authority_epoch += 1,
        |value| value.generation += 1,
        |value| value.policy_epoch += 1,
        |value| value.pending_slot ^= 1,
        |value| value.pending_spki_digest[0] ^= 1,
        |value| value.profile_digest[0] ^= 1,
        |value| value.boot_epoch += 1,
        |value| value.validation_request_id += 1,
        |value| value.upload_handle += 1,
        |value| value.profile_len += 1,
    ];

    for (index, mutate) in mutations.into_iter().enumerate() {
        let (mut state, prepared) = prepared_state();
        let mut candidate = receipt(&prepared);
        mutate(&mut candidate);
        let verified = verify_provider_cas_receipt(candidate, &CasPolicy).unwrap();
        assert_eq!(
            state.record_provider_promotion(&prepared, &verified),
            Err(AuthorityFault::ProviderSplitBrain),
            "receipt field {index} was not bound"
        );
        assert_eq!(state.mode(), AuthorityMode::Sealed);
    }
}

#[test]
fn provider_signature_must_verify_before_promotion() {
    let (_, prepared) = prepared_state();
    let mut candidate = receipt(&prepared);
    candidate.provider_signature = [8; SIGNATURE_LEN];
    assert_eq!(
        verify_provider_cas_receipt(candidate, &CasPolicy),
        Err(AuthorityFault::ProviderSplitBrain)
    );
}
