mod support;
use authority_protocol::*;
use support::*;

#[test]
fn promoted_record_retains_v2_receipt_binding_and_rejects_mismatch() {
    let digest = [8; DIGEST_LEN];
    let mut authority = state(0, 0);
    let mut challenges = Challenges(4);
    authority.open_boot(&open(1), &measurement()).unwrap();
    grant_time(
        &mut authority,
        &mut challenges,
        2,
        1,
        TimePurpose::Enrollment,
        1,
        200,
    );
    authority
        .begin_enrollment(&begin(4, 1), &Clock(101))
        .unwrap();
    complete_upload(&mut authority, 5, 1, 1, digest);
    authority
        .consume_receipt(&consume(8, 1, 1, digest))
        .unwrap();
    let prepared = authority.prepare_commit(&commit(9, 1, 1, digest)).unwrap();
    let intent = *prepared.intent();
    let receipt = ProviderCasReceipt {
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
    };
    let verified_receipt = verify_provider_cas_receipt(receipt, &CasPolicy).unwrap();
    authority
        .record_provider_promotion(&prepared, &verified_receipt)
        .unwrap();
    let record = authority.into_store().into_record().unwrap();

    let mut encoded = [0u8; PROTECTED_RECORD_MAX];
    let length = record.encode_canonical(&mut encoded).unwrap();
    const RELAY_TAG_OFFSET: usize = 24;
    const RELAY_INTENT_OFFSET: usize = RELAY_TAG_OFFSET + 1;
    const INTENT_WIRE_LEN: usize = 2 * ID_LEN + 7 * 8 + 4 + 1 + 3 * DIGEST_LEN;
    const RECEIPT_OFFSET: usize = RELAY_INTENT_OFFSET + INTENT_WIRE_LEN;
    const RECEIPT_WIRE_LEN: usize = 2 * ID_LEN + 6 * 8 + 4 + 1 + 2 * DIGEST_LEN;
    const SIGNATURE_OFFSET: usize = RECEIPT_OFFSET + RECEIPT_WIRE_LEN;
    assert_eq!(encoded[RELAY_TAG_OFFSET], 6);
    assert_eq!(
        encoded[SIGNATURE_OFFSET..SIGNATURE_OFFSET + SIGNATURE_LEN],
        [9; SIGNATURE_LEN]
    );
    assert_eq!(
        ProtectedAuthorityRecord::decode_canonical(&encoded[..length]),
        Ok(record)
    );

    encoded[RECEIPT_OFFSET] ^= 1;
    assert_eq!(
        ProtectedAuthorityRecord::decode_canonical(&encoded[..length]),
        Err(WireError::InvalidLength)
    );
}
