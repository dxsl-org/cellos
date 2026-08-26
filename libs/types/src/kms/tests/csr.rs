// SPDX-License-Identifier: MPL-2.0
//! Frozen CSR/DER assembly tests for the relay profile.

use crate::kms::{
    assemble_relay_csr, canonical_relay_cri, der_ecdsa_signature, p256_spki_der, validate_hostname,
    CRI_MAX_LEN, NODE_ID_OID_CONTENT, RELAY_CSR_CHUNK_LEN, RELAY_CSR_MAX_LEN, RELAY_HOSTNAME_MAX,
};

const HOSTNAME: &[u8] = b"relay.example.internal";
const POINT: [u8; 65] = {
    let mut point = [0u8; 65];
    point[0] = 0x04;
    point[1] = 0xAB;
    point
};

#[test]
fn hostname_validation_enforces_the_frozen_dns_profile() {
    assert!(validate_hostname(HOSTNAME));
    assert!(validate_hostname(b"a.b"));
    assert!(validate_hostname(b"0-9.a-z"));
    for bad in [
        &b""[..],
        b".relay.example",
        b"relay.example.",
        b"-relay.example",
        b"relay-.example",
        b"relay..example",
        b"Relay.example",
        b"relay_ex.ample",
        b"relay.ex ample",
        &[b'a'; RELAY_HOSTNAME_MAX + 1][..],
    ] {
        assert!(!validate_hostname(bad), "expected rejection: {bad:?}");
    }
    // DNS labels cap at 63 even though the wire bound is 64 total.
    let longest = [b'a'; RELAY_HOSTNAME_MAX - 1];
    assert!(validate_hostname(&longest));
}
#[test]
fn spki_der_is_fixed_length_and_rejects_compressed_points() {
    let (spki, len) = p256_spki_der(&POINT).unwrap();
    assert_eq!(len, 91);
    assert_eq!(&spki[..2], &[0x30, 0x59]);
    assert_eq!(&spki[2..4], &[0x30, 0x13]);
    assert_eq!(
        &spki[4..13],
        &[0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01]
    );
    assert_eq!(
        &spki[13..23],
        &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07]
    );
    assert_eq!(&spki[23..25], &[0x03, 0x42]);
    assert_eq!(spki[25], 0x00);
    assert_eq!(&spki[26..28], &[0x04, 0xAB]);
    assert!(spki[len..].iter().all(|byte| *byte == 0));
    let mut compressed = POINT;
    compressed[0] = 0x02;
    assert!(p256_spki_der(&compressed).is_none());
}

#[test]
fn node_id_oid_content_encodes_the_exact_extension_arc() {
    let expected: [u64; 9] = [1, 3, 6, 1, 4, 1, 55555, 1, 1];
    // The first content byte packs the first two arcs as 40*1+3.
    assert_eq!(
        u64::from(NODE_ID_OID_CONTENT[0]),
        40 * expected[0] + expected[1]
    );
    let mut cursor = 1usize;
    let mut index = 2usize;
    while index < expected.len() {
        let mut decoded: u64 = 0;
        loop {
            let byte = NODE_ID_OID_CONTENT[cursor];
            cursor += 1;
            decoded = (decoded << 7) | (byte & 0x7F) as u64;
            if byte & 0x80 == 0 {
                break;
            }
        }
        assert_eq!(decoded, expected[index], "arc {index}");
        index += 1;
    }
    assert_eq!(cursor, NODE_ID_OID_CONTENT.len());
}

#[test]
fn canonical_cri_is_deterministic_bounded_and_input_sensitive() {
    let (cri, len) = canonical_relay_cri(HOSTNAME, &POINT).unwrap();
    assert!(len <= CRI_MAX_LEN);
    assert_eq!(cri[0], 0x30);
    assert!(cri[len..].iter().all(|byte| *byte == 0));
    let (again, again_len) = canonical_relay_cri(HOSTNAME, &POINT).unwrap();
    assert_eq!((cri, len), (again, again_len));
    // Layout: SEQ | INTEGER 0 | Name SEQ { SET { SEQ { OID cn, UTF8 } } }.
    // Canonical minimal long-form length: 0x81 plus the 131-byte content.
    assert_eq!(&cri[1..3], &[0x81, 0x83]);
    assert_eq!(&cri[3..6], &[0x02, 0x01, 0x00]);
    assert_eq!(cri[6], 0x30);
    assert_eq!(cri[8], 0x31);
    assert_eq!(&cri[12..17], &[0x06, 0x03, 0x55, 0x04, 0x03]);
    assert_eq!(cri[18] as usize, HOSTNAME.len());
    assert_eq!(&cri[19..19 + HOSTNAME.len()], HOSTNAME);
    // The SPKI follows verbatim, then the mandatory empty attributes.
    let spki_at = 19 + HOSTNAME.len();
    assert_eq!(&cri[spki_at..spki_at + 2], &[0x30, 0x59]);
    let attributes_at = spki_at + 91;
    assert_eq!(&cri[attributes_at..len], &[0xA0, 0x00]);
    let (other, _) = canonical_relay_cri(b"other.example.internal", &POINT).unwrap();
    assert_ne!(&cri[..len], &other[..other.len()]);
}

#[test]
fn signature_der_encoding_handles_zero_stripping_and_high_bit_pad() {
    let mut raw = [0u8; 64];
    raw[30] = 0x01;
    raw[31] = 0xFF;
    raw[32] = 0x80;
    let (sig, len) = der_ecdsa_signature(&raw);
    // r strips to two significant bytes; s keeps its full 32-byte body with
    // a high-bit pad octet, so the SEQUENCE content is 39 bytes.
    assert_eq!(&sig[..2], &[0x30, 0x27]);
    assert_eq!(&sig[2..6], &[0x02, 0x02, 0x01, 0xFF]); // zeros stripped
    assert_eq!(&sig[6..10], &[0x02, 0x21, 0x00, 0x80]); // high bit padded
    assert_eq!(len, 41);
}

#[test]
fn csr_assembly_is_bounded_and_zero_tailed() {
    let (cri, cri_len) = canonical_relay_cri(HOSTNAME, &POINT).unwrap();
    let (sig, sig_len) = der_ecdsa_signature(&[0x22; 64]);
    let (csr, csr_len) = assemble_relay_csr(&cri, cri_len, &sig, sig_len).unwrap();
    // Outer SEQUENCE uses the canonical minimal one-byte long-form length.
    assert_eq!(csr[1], 0x81);
    assert_eq!(csr[2] as usize, csr_len - 3);
    // The CRI is embedded verbatim right behind the outer header.
    assert_eq!(&csr[3..3 + cri_len], &cri[..cri_len]);
}

#[test]
fn csr_assembly_rejects_unbounded_inputs() {
    let (cri, cri_len) = canonical_relay_cri(HOSTNAME, &POINT).unwrap();
    let (sig, sig_len) = der_ecdsa_signature(&[0x11; 64]);
    assert!(assemble_relay_csr(&cri, 0, &sig, sig_len).is_none());
    assert!(assemble_relay_csr(&cri, cri.len() + 1, &sig, sig_len).is_none());
    assert!(assemble_relay_csr(&cri, cri_len, &sig, 0).is_none());
    assert!(assemble_relay_csr(&cri, cri_len, &sig, sig.len() + 1).is_none());
    assert!(RELAY_CSR_CHUNK_LEN < RELAY_CSR_MAX_LEN);
}

#[test]
fn signature_algorithm_sequence_wraps_the_full_oid_tlv() {
    let (cri, cri_len) = canonical_relay_cri(HOSTNAME, &POINT).unwrap();
    let (sig, sig_len) = der_ecdsa_signature(&[0x22; 64]);
    let (csr, csr_len) = assemble_relay_csr(&cri, cri_len, &sig, sig_len).unwrap();
    // Behind the CRI: AlgorithmIdentifier SEQ(0A){ OID(08) } — the header
    // counts the whole embedded OID TLV (10 bytes), not just its content.
    let alg_at = 3 + cri_len;
    assert_eq!(
        &csr[alg_at..alg_at + 12],
        &[0x30, 0x0A, 0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02]
    );
    // BIT STRING header plus unused-bits octet, then the DER signature.
    assert_eq!(&csr[alg_at + 12..alg_at + 15], &[0x03, 0x47, 0x00]);
    assert_eq!(csr_len, alg_at + 15 + sig_len);
    assert!(csr[csr_len..].iter().all(|byte| *byte == 0));
}

#[test]
fn every_der_element_declares_its_true_content_length() {
    let (cri, cri_len) = canonical_relay_cri(HOSTNAME, &POINT).unwrap();
    let (sig, sig_len) = der_ecdsa_signature(&[0x22; 64]);
    let (csr, csr_len) = assemble_relay_csr(&cri, cri_len, &sig, sig_len).unwrap();
    parse_minimal_der(&csr[..csr_len], 0);

    /// Recursively walks DER elements, failing unless every header declares
    /// a minimal length and the children tile the parent content exactly.
    fn parse_minimal_der(buf: &[u8], depth: usize) {
        assert!(depth < 8, "DER nesting too deep");
        let mut at = 0usize;
        while at < buf.len() {
            assert!(at + 2 <= buf.len(), "truncated header");
            let tag = buf[at];
            let first = buf[at + 1] as usize;
            let (head_len, content_len) = if first < 0x80 {
                (2, first)
            } else {
                let count = first & 0x7F;
                assert!(count <= 2 && at + 2 + count <= buf.len());
                assert!(buf[at + 2] != 0, "non-minimal long-form length");
                let mut len = 0usize;
                for byte in &buf[at + 2..at + 2 + count] {
                    len = (len << 8) | usize::from(*byte);
                }
                assert!(len >= 0x80, "long form used for a short length");
                (2 + count, len)
            };
            let end = at + head_len + content_len;
            assert!(end <= buf.len(), "declared length overruns parent");
            if tag == 0x30 || tag == 0x31 {
                parse_minimal_der(&buf[at + head_len..end], depth + 1);
            }
            at = end;
        }
    }
}
