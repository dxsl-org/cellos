use crate::cbor_write::Writer;
use crate::test_support::*;
use crate::*;
use ed25519_compact::{KeyPair, Seed};

#[test]
fn payload_roundtrip_is_canonical_and_deterministic() {
    let (manifest, _, _) = fixture();
    let mut first = [0u8; MAX_PAYLOAD_LEN];
    let mut second = [0u8; MAX_PAYLOAD_LEN];
    let n = encode_payload(&manifest, &mut first).unwrap();
    let m = encode_payload(&manifest, &mut second).unwrap();
    assert_eq!(&first[..n], &second[..m]);
    assert_eq!(decode_payload(&first[..n]), Ok(manifest));
}

#[test]
fn cose_signing_is_byte_deterministic() {
    let (manifest, _, _) = fixture();
    let (first, first_key) = signed_cose(&manifest);
    let (second, second_key) = signed_cose(&manifest);
    assert_eq!(first_key, second_key);
    assert_eq!(first, second);
}

#[test]
fn payload_rejects_noncanonical_schema_and_trailing_forms() {
    let (manifest, _, _) = fixture();
    let mut bytes = [0u8; MAX_PAYLOAD_LEN];
    let n = encode_payload(&manifest, &mut bytes).unwrap();
    let mut noncanonical = bytes[..n].to_vec();
    let epoch = noncanonical.windows(2).position(|w| w == [5, 11]).unwrap();
    noncanonical.splice(epoch + 1..epoch + 2, [0x18, 11]);
    assert_eq!(decode_payload(&noncanonical), Err(Error::NonCanonical));
    let mut duplicate = bytes[..n].to_vec();
    let key3 = duplicate.windows(2).position(|w| w == [3, 0x58]).unwrap();
    duplicate[key3] = 4;
    assert_eq!(decode_payload(&duplicate), Err(Error::UnknownKey));
    let mut trailing = bytes[..n].to_vec();
    trailing.push(0);
    assert_eq!(decode_payload(&trailing), Err(Error::TrailingData));
    let lane = bytes[..n]
        .windows(LANE.len())
        .position(|w| w == LANE.as_bytes())
        .unwrap();
    let mut wrong_lane = bytes[..n].to_vec();
    wrong_lane[lane] ^= 1;
    assert_eq!(decode_payload(&wrong_lane), Err(Error::WrongLane));
}

#[test]
fn cose_rejects_tag_algorithm_key_signature_and_public_key_changes() {
    let (manifest, _, _) = fixture();
    let (cose, public_key) = signed_cose(&manifest);
    let mut scratch = [0u8; MAX_SIG_STRUCTURE_LEN];
    assert!(verify_cose(&cose, &public_key, &mut scratch).is_ok());
    let mut bad = cose.clone();
    bad[0] = 0x84;
    assert!(verify_cose(&bad, &public_key, &mut scratch).is_err());
    let mut bad = cose.clone();
    bad[6] = 0x26;
    assert_eq!(
        verify_cose(&bad, &public_key, &mut scratch),
        Err(Error::WrongAlgorithm)
    );
    let mut bad = cose.clone();
    bad[10] ^= 1;
    assert_eq!(
        verify_cose(&bad, &public_key, &mut scratch),
        Err(Error::WrongKeyId)
    );
    let mut bad = cose.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert_eq!(
        verify_cose(&bad, &public_key, &mut scratch),
        Err(Error::Signature)
    );
    let other_key = public_key_from_seed(&[8; 32]).unwrap();
    assert_eq!(
        verify_cose(&cose, &other_key, &mut scratch),
        Err(Error::WrongKeyId)
    );
}

#[test]
fn cose_rejects_unprotected_detached_and_trailing_forms() {
    let (manifest, _, _) = fixture();
    let (cose, public_key) = signed_cose(&manifest);
    let mut scratch = [0u8; MAX_SIG_STRUCTURE_LEN];
    let mut bad = cose.clone();
    bad[42] = 0xa1;
    assert_eq!(
        verify_cose(&bad, &public_key, &mut scratch),
        Err(Error::InvalidCose)
    );
    let mut bad = cose.clone();
    bad[43] = 0xf6;
    assert_eq!(
        verify_cose(&bad, &public_key, &mut scratch),
        Err(Error::WrongType)
    );
    let mut bad = cose.clone();
    bad.push(0);
    assert_eq!(
        verify_cose(&bad, &public_key, &mut scratch),
        Err(Error::TrailingData)
    );
}

#[test]
fn fixed_external_aad_is_part_of_signature_preimage() {
    let (manifest, _, _) = fixture();
    let (mut cose, public_key) = signed_cose(&manifest);
    let protected = &cose[4..42];
    let payload_head = 43;
    let (header, payload_len) = if cose[payload_head] == 0x58 {
        (2, cose[payload_head + 1] as usize)
    } else {
        (
            3,
            u16::from_be_bytes([cose[payload_head + 1], cose[payload_head + 2]]) as usize,
        )
    };
    let payload_start = payload_head + header;
    let payload = &cose[payload_start..payload_start + payload_len];
    let mut preimage = [0u8; MAX_SIG_STRUCTURE_LEN];
    let mut writer = Writer::new(&mut preimage);
    writer.array(4).unwrap();
    writer.tstr("Signature1").unwrap();
    writer.bstr(protected).unwrap();
    writer.bstr(b"wrong-aad").unwrap();
    writer.bstr(payload).unwrap();
    let preimage_len = writer.len();
    drop(writer);
    let pair = KeyPair::from_seed(Seed::from(SEED));
    let signature = pair.sk.sign(&preimage[..preimage_len], None);
    let signature_start = cose.len() - 64;
    cose[signature_start..].copy_from_slice(signature.as_ref());
    let mut scratch = [0u8; MAX_SIG_STRUCTURE_LEN];
    assert_eq!(
        verify_cose(&cose, &public_key, &mut scratch),
        Err(Error::Signature)
    );
}

#[test]
fn signing_rejects_zero_seed_and_non_manifest_payload() {
    assert_eq!(public_key_from_seed(&[0; 32]), Err(Error::InvalidSeed));
    let mut out = [0u8; MAX_COSE_LEN];
    let mut scratch = [0u8; MAX_SIG_STRUCTURE_LEN];
    assert_eq!(
        sign_cose(b"not-cbor", &[7; 32], &mut out, &mut scratch),
        Err(Error::WrongType)
    );
}
