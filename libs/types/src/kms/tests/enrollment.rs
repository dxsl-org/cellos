// SPDX-License-Identifier: MPL-2.0
//! Enrollment payload codec tests: bounds, canonical padding, rejection.

use crate::kms::{
    RelayCsrChunkRequestPayload, RelayCsrChunkResponsePayload, RelayEnrollmentAbortRequestPayload,
    RelayEnrollmentBeginRequestPayload, RelayEnrollmentBeginResponsePayload,
    RelayGenerationCommitRequestPayload, RelayGenerationCommitResponsePayload, RELAY_CSR_CHUNK_LEN,
};

#[test]
fn begin_request_codec_rejects_bad_hostnames_and_padding() {
    let mut payload = RelayEnrollmentBeginRequestPayload {
        hostname_len: 22,
        hostname: [0; 64],
    };
    payload.hostname[..22].copy_from_slice(b"relay.example.internal");
    assert_eq!(
        RelayEnrollmentBeginRequestPayload::decode(&payload.encode()),
        Some(payload)
    );

    // Nonzero padding beyond the declared hostname length is noncanonical.
    let mut padded = payload.encode();
    padded[23] = b'x';
    assert!(RelayEnrollmentBeginRequestPayload::decode(&padded).is_none());

    // Declared length beyond the frozen bound.
    let mut long = payload.encode();
    long[0] = 65;
    assert!(RelayEnrollmentBeginRequestPayload::decode(&long).is_none());

    for bad in [
        "",
        ".dot",
        "dot.",
        "double..dot",
        "UPPER.example",
        "hyphen-",
    ] {
        let mut bytes = [0u8; 65];
        bytes[0] = bad.len() as u8;
        bytes[1..1 + bad.len()].copy_from_slice(bad.as_bytes());
        assert!(RelayEnrollmentBeginRequestPayload::decode(&bytes).is_none());
    }
}

#[test]
fn chunk_payloads_round_trip_with_exact_capacity() {
    let request = RelayCsrChunkRequestPayload {
        csr_handle: 0x0102_0304_0506_0708,
        chunk_index: 9,
        reserved: 0,
    };
    assert_eq!(
        RelayCsrChunkRequestPayload::decode(&request.encode()),
        Some(request)
    );
    let mut smuggled = request.encode();
    smuggled[12] = 1;
    assert!(RelayCsrChunkRequestPayload::decode(&smuggled).is_none());

    let mut response = RelayCsrChunkResponsePayload {
        chunk_index: 3,
        chunk_len: RELAY_CSR_CHUNK_LEN as u16,
        reserved: 0,
        chunk: [0xEE; RELAY_CSR_CHUNK_LEN],
    };
    assert_eq!(
        RelayCsrChunkResponsePayload::decode(&response.encode()),
        Some(response)
    );

    // Zero-length and over-capacity chunks are structurally invalid.
    response.chunk_len = 0;
    assert!(RelayCsrChunkResponsePayload::decode(&response.encode()).is_none());
    response.chunk_len = (RELAY_CSR_CHUNK_LEN + 1) as u16;
    assert!(RelayCsrChunkResponsePayload::decode(&response.encode()).is_none());
    let mut reserved = response.encode();
    reserved[6] = 1;
    assert!(RelayCsrChunkResponsePayload::decode(&reserved).is_none());
}

#[test]
fn commit_abort_codecs_round_trip() {
    let commit = RelayGenerationCommitRequestPayload {
        pending_relay_generation: 8,
        expected_policy_epoch: 11,
        profile_digest: [0x44; 32],
    };
    assert_eq!(
        RelayGenerationCommitRequestPayload::decode(&commit.encode()),
        Some(commit)
    );
    let committed = RelayGenerationCommitResponsePayload {
        active_relay_generation: 8,
        policy_epoch: 11,
        active_profile_digest: [0x44; 32],
    };
    assert_eq!(
        RelayGenerationCommitResponsePayload::decode(&committed.encode()),
        Some(committed)
    );
    let abort = RelayEnrollmentAbortRequestPayload {
        pending_relay_generation: 8,
    };
    assert_eq!(
        RelayEnrollmentAbortRequestPayload::decode(&abort.encode()),
        Some(abort)
    );
}

#[test]
fn begin_response_carries_handle_facts_canonically() {
    let begin = RelayEnrollmentBeginResponsePayload {
        pending_relay_generation: 8,
        policy_epoch: 11,
        restart_epoch: 2,
        csr_handle: 0xA1B2_C3D4_1122_3344,
        csr_len: 317,
        reserved: 0,
        csr_sha256: [0x55; 32],
    };
    let encoded = begin.encode();
    assert_eq!(&encoded[..8], &[8, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&encoded[40..72], &[0x55; 32]);
    assert_eq!(
        RelayEnrollmentBeginResponsePayload::decode(&encoded),
        Some(begin)
    );
    let mut noncanonical = encoded;
    noncanonical[36] = 1;
    assert!(RelayEnrollmentBeginResponsePayload::decode(&noncanonical).is_none());
}
