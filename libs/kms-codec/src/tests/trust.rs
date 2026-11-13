// SPDX-License-Identifier: MPL-2.0
//! SSH host-trust store ABI (opcodes 25-29, errors 39-40): pinned values and strict
//! payload codecs.

use crate::{
    KmsErrorCode, KmsOpcode, KmsRequestV1, KmsResponseV1, KmsTrustQueryKind,
    TrustChallengeCommitRequestPayload, TrustOutcomePayload, TrustQueryBeginRequestPayload,
    KMS_TRUST_QUERY_MAX,
};

#[test]
fn trust_opcodes_errors_and_kinds_are_pinned() {
    for (opcode, value) in [
        (KmsOpcode::BeginTrustQuery, 25u8),
        (KmsOpcode::TrustQueryChunk, 26),
        (KmsOpcode::CommitTrustQuery, 27),
        (KmsOpcode::CommitTrustChallenge, 28),
        (KmsOpcode::AbortTrustQuery, 29),
    ] {
        assert_eq!(opcode as u8, value);
        assert_eq!(KmsOpcode::try_from(value), Ok(opcode));
        let frame = KmsRequestV1::new(opcode, 9, &[]).unwrap().to_bytes();
        assert_eq!(frame[1], value);
    }
    assert_eq!(KmsOpcode::try_from(30), Err(30));
    for (code, value) in [
        (KmsErrorCode::TrustHandleInvalid, 39u16),
        (KmsErrorCode::TrustOrderInvalid, 40),
    ] {
        assert_eq!(code as u16, value);
        assert_eq!(KmsErrorCode::try_from(value), Ok(code));
        let frame = KmsResponseV1::error(KmsOpcode::TrustQueryChunk, 9, code).to_bytes();
        assert_eq!(&frame[8..10], &value.to_le_bytes());
    }
    assert_eq!(KmsErrorCode::try_from(41), Err(41));
    assert_eq!(KmsTrustQueryKind::Lookup as u8, 1);
    assert_eq!(KmsTrustQueryKind::Challenge as u8, 2);
    assert_eq!(KmsTrustQueryKind::try_from(0), Err(0));
    assert_eq!(KmsTrustQueryKind::try_from(3), Err(3));
    assert_eq!(KMS_TRUST_QUERY_MAX, 253 + 64 + 4096);
}

fn lookup() -> TrustQueryBeginRequestPayload {
    TrustQueryBeginRequestPayload {
        kind: KmsTrustQueryKind::Lookup,
        algorithm_len: 11,
        port: 2222,
        host_len: 8,
        key_len: 51,
        epoch: 0,
        session: (0, 0),
        query_sha256: [0x5a; 32],
    }
}

#[test]
fn begin_request_has_fixed_bytes_and_strict_decode() {
    let begin = TrustQueryBeginRequestPayload {
        kind: KmsTrustQueryKind::Challenge,
        epoch: 4,
        session: (41, 3),
        ..lookup()
    };
    let bytes = begin.encode();
    let mut expected = [0u8; 64];
    expected[..8].copy_from_slice(&[2, 11, 0xae, 0x08, 8, 0, 51, 0]);
    expected[8] = 4;
    expected[16] = 41;
    expected[24] = 3;
    expected[32..].copy_from_slice(&[0x5a; 32]);
    assert_eq!(bytes, expected);
    assert_eq!(TrustQueryBeginRequestPayload::decode(&bytes), Some(begin));
    assert_eq!(begin.total_len(), 8 + 11 + 51);
    assert_eq!(
        TrustQueryBeginRequestPayload::decode(&lookup().encode()),
        Some(lookup())
    );

    let refused: [fn(&mut TrustQueryBeginRequestPayload); 9] = [
        |p| p.port = 0,
        |p| p.host_len = 0,
        |p| p.host_len = 254,
        |p| p.algorithm_len = 0,
        |p| p.algorithm_len = 65,
        |p| p.key_len = 0,
        |p| p.key_len = 4097,
        |p| p.query_sha256 = [0; 32],
        // A lookup names no session or epoch.
        |p| p.session = (1, 0),
    ];
    for (i, change) in refused.iter().enumerate() {
        let mut p = lookup();
        change(&mut p);
        assert_eq!(
            TrustQueryBeginRequestPayload::decode(&p.encode()),
            None,
            "case {i}"
        );
    }
    let mut epoch = lookup().encode();
    epoch[8] = 1;
    assert_eq!(TrustQueryBeginRequestPayload::decode(&epoch), None);
    // A challenge needs a session; an unknown kind and a wrong length are refused.
    let mut no_session = begin.encode();
    no_session[16] = 0;
    no_session[24] = 0;
    assert_eq!(TrustQueryBeginRequestPayload::decode(&no_session), None);
    let mut kind = begin.encode();
    kind[0] = 3;
    assert_eq!(TrustQueryBeginRequestPayload::decode(&kind), None);
    assert_eq!(TrustQueryBeginRequestPayload::decode(&bytes[..63]), None);
}

#[test]
fn outcome_payload_has_exactly_two_shapes() {
    let plain = TrustOutcomePayload::outcome(2);
    let bytes = plain.encode();
    assert_eq!(bytes[0], 2);
    assert!(bytes[1..].iter().all(|b| *b == 0));
    assert_eq!(TrustOutcomePayload::decode(&bytes), Some(plain));
    for code in 0..=7 {
        let p = TrustOutcomePayload::outcome(code);
        assert_eq!(TrustOutcomePayload::decode(&p.encode()), Some(p));
    }
    assert_eq!(
        TrustOutcomePayload::decode(&TrustOutcomePayload::outcome(8).encode()),
        None
    );

    let issued = TrustOutcomePayload::challenged((500, 7), (1, 1));
    let bytes = issued.encode();
    let mut expected = [0u8; 40];
    expected[0] = 1;
    expected[1] = 1;
    expected[8..10].copy_from_slice(&500u16.to_le_bytes());
    expected[16] = 7;
    expected[24] = 1;
    expected[32] = 1;
    assert_eq!(bytes, expected);
    assert_eq!(TrustOutcomePayload::decode(&bytes), Some(issued));

    // Refs without the flag, the flag without refs or with another outcome, reserved
    // bytes: all refused.
    let mut stray = TrustOutcomePayload::outcome(0).encode();
    stray[8] = 1;
    assert_eq!(TrustOutcomePayload::decode(&stray), None);
    let mut no_refs = expected;
    no_refs[24] = 0;
    no_refs[32] = 0;
    assert_eq!(TrustOutcomePayload::decode(&no_refs), None);
    let mut other = expected;
    other[0] = 7;
    assert_eq!(TrustOutcomePayload::decode(&other), None);
    let mut reserved = expected;
    reserved[5] = 1;
    assert_eq!(TrustOutcomePayload::decode(&reserved), None);
    let mut flag = expected;
    flag[1] = 2;
    assert_eq!(TrustOutcomePayload::decode(&flag), None);
}

#[test]
fn challenge_commit_request_names_a_nonzero_ref() {
    let p = TrustChallengeCommitRequestPayload { challenge: (3, 1) };
    let bytes = p.encode();
    assert_eq!(bytes, [3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(TrustChallengeCommitRequestPayload::decode(&bytes), Some(p));
    assert_eq!(TrustChallengeCommitRequestPayload::decode(&[0; 16]), None);
    assert_eq!(
        TrustChallengeCommitRequestPayload::decode(&bytes[..15]),
        None
    );
}
