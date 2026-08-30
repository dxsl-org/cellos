use crate::test_support::*;
use crate::*;

#[test]
fn complete_bundle_verifies_and_component_tampering_fails() {
    let (manifest, expected, limits) = fixture();
    let (padded, public_key) = padded_bundle(&manifest);
    let mut scratch = [0u8; MAX_SIG_STRUCTURE_LEN];
    let verified = verify_bundle(&padded, &public_key, &expected, &limits, &mut scratch).unwrap();
    assert_eq!(verified.manifest, manifest);
    assert_eq!(verified.component_region, REGION);
    let mut tampered = padded.clone();
    let start = verified.logical_len - REGION.len();
    tampered[start] ^= 1;
    assert_eq!(
        verify_bundle(&tampered, &public_key, &expected, &limits, &mut scratch),
        Err(Error::DigestMismatch)
    );
}

#[test]
fn bundle_rejects_identity_freshness_length_padding_and_extra_block() {
    let (manifest, expected, limits) = fixture();
    let (padded, public_key) = padded_bundle(&manifest);
    let mut scratch = [0u8; MAX_SIG_STRUCTURE_LEN];
    let mut wrong = expected;
    wrong.device_id[0] ^= 1;
    assert_eq!(
        verify_bundle(&padded, &public_key, &wrong, &limits, &mut scratch),
        Err(Error::WrongIdentity)
    );
    let mut wrong = expected;
    wrong.request_id += 1;
    assert_eq!(
        verify_bundle(&padded, &public_key, &wrong, &limits, &mut scratch),
        Err(Error::WrongFreshness)
    );
    let mut bad_length = padded.clone();
    bad_length[..4].fill(0);
    assert_eq!(
        verify_bundle(&bad_length, &public_key, &expected, &limits, &mut scratch),
        Err(Error::LimitExceeded)
    );
    let mut bad_padding = padded.clone();
    let last = bad_padding.len() - 1;
    bad_padding[last] = 0;
    assert_eq!(
        verify_bundle(&bad_padding, &public_key, &expected, &limits, &mut scratch),
        Err(Error::InvalidPadding)
    );
    let mut extra = padded.clone();
    extra.extend_from_slice(&[XMODEM_PADDING; XMODEM_BLOCK_LEN]);
    assert_eq!(
        verify_bundle(&extra, &public_key, &expected, &limits, &mut scratch),
        Err(Error::TrailingData)
    );
}

#[test]
fn xmodem_transcript_is_deterministic_and_roundtrips() {
    let logical = b"deterministic transcript";
    let length = xmodem_encoded_len(logical.len(), 2).unwrap();
    let mut first = std::vec![0u8; length];
    let mut second = std::vec![0u8; length];
    assert_eq!(encode_xmodem(logical, &mut first, 2), Ok(length));
    assert_eq!(encode_xmodem(logical, &mut second, 2), Ok(length));
    assert_eq!(first, second);
    assert_eq!(first[length - 1], XMODEM_EOT);
    let mut decoded = [0u8; XMODEM_BLOCK_LEN];
    assert_eq!(decode_xmodem(&first, &mut decoded, 2), Ok(XMODEM_BLOCK_LEN));
    assert_eq!(&decoded[..logical.len()], logical);
    assert!(decoded[logical.len()..]
        .iter()
        .all(|byte| *byte == XMODEM_PADDING));
}

#[test]
fn xmodem_rejects_block_complement_crc_missing_eot_and_trailing_bytes() {
    let logical = b"frame";
    let length = xmodem_encoded_len(logical.len(), 1).unwrap();
    let mut transcript = std::vec![0u8; length];
    encode_xmodem(logical, &mut transcript, 1).unwrap();
    let mut output = [0u8; XMODEM_BLOCK_LEN];
    let mut bad = transcript.clone();
    bad[2] ^= 1;
    assert_eq!(
        decode_xmodem(&bad, &mut output, 1),
        Err(Error::InvalidBlock)
    );
    let mut bad = transcript.clone();
    bad[3] ^= 1;
    assert_eq!(decode_xmodem(&bad, &mut output, 1), Err(Error::InvalidCrc));
    let mut bad = transcript.clone();
    bad.pop();
    assert_eq!(decode_xmodem(&bad, &mut output, 1), Err(Error::MissingEot));
    let mut bad = transcript.clone();
    bad.push(0);
    assert_eq!(
        decode_xmodem(&bad, &mut output, 1),
        Err(Error::TrailingData)
    );
}

#[test]
fn transcript_padding_and_additional_data_block_fail_bundle_admission() {
    let (manifest, expected, limits) = fixture();
    let (padded, public_key) = padded_bundle(&manifest);
    let mut transcript = std::vec![0u8; xmodem_encoded_len(padded.len(), 2).unwrap()];
    encode_xmodem(&padded, &mut transcript, 2).unwrap();
    transcript[3 + padded.len() - 1] = 0;
    let crc = crc16_xmodem(&transcript[3..3 + XMODEM_BLOCK_LEN]).to_be_bytes();
    transcript[3 + XMODEM_BLOCK_LEN..3 + XMODEM_BLOCK_LEN + 2].copy_from_slice(&crc);
    let mut decoded = [0u8; XMODEM_BLOCK_LEN * 2];
    let decoded_len = decode_xmodem(&transcript, &mut decoded, 2).unwrap();
    let mut scratch = [0u8; MAX_SIG_STRUCTURE_LEN];
    assert_eq!(
        verify_bundle(
            &decoded[..decoded_len],
            &public_key,
            &expected,
            &limits,
            &mut scratch
        ),
        Err(Error::InvalidPadding)
    );
    let mut enlarged = padded.clone();
    enlarged.extend_from_slice(&[XMODEM_PADDING; XMODEM_BLOCK_LEN]);
    let mut transcript = std::vec![0u8; xmodem_encoded_len(enlarged.len(), 2).unwrap()];
    encode_xmodem(&enlarged, &mut transcript, 2).unwrap();
    let decoded_len = decode_xmodem(&transcript, &mut decoded, 2).unwrap();
    assert_eq!(
        verify_bundle(
            &decoded[..decoded_len],
            &public_key,
            &expected,
            &limits,
            &mut scratch
        ),
        Err(Error::TrailingData)
    );
}
