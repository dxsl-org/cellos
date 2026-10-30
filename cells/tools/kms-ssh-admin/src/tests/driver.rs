//! The chunk driver and the import command over a scripted key service.

use ostd::clients::kms::{KmsClientError, KmsErrorCode, KmsWireError};

use super::{
    armor_text, filler, plain_blob, run_cell, At, Call, FakeKms, Tamper, TestConsole, HANDLE,
};
use crate::import::drive;

/// K2 section 4.2: (total, chunk count, last chunk length).
const VECTORS: [(usize, u32, usize); 7] = [
    (1, 1, 1),
    (96, 1, 96),
    (97, 2, 1),
    (250, 3, 58),
    (290, 4, 2),
    (65_535, 683, 63),
    (65_536, 683, 64),
];

#[test]
fn the_driver_sends_begin_canonical_chunks_and_commit_in_order() {
    for (total, count, last) in VECTORS {
        let blob = filler(total);
        let kms = FakeKms::new();
        let done = drive(&kms, b"lane", &blob).unwrap();
        let sha = ostd::session::sha256(&blob);
        assert_eq!(done.blob_len, total);
        assert_eq!(done.digest, sha);
        let calls = kms.calls();
        assert_eq!(calls.len(), 2 + count as usize, "{total}");
        assert_eq!(
            calls[0],
            Call::Begin {
                name: b"lane".to_vec(),
                total: total as u32,
                sha
            }
        );
        let mut rebuilt = Vec::new();
        for (i, call) in calls[1..calls.len() - 1].iter().enumerate() {
            let Call::Chunk {
                handle,
                index,
                data,
            } = call
            else {
                panic!("{call:?}")
            };
            assert_eq!((*handle, *index as usize), (HANDLE, i));
            let want = if i + 1 < count as usize { 96 } else { last };
            assert_eq!(data.len(), want, "{total} chunk {i}");
            rebuilt.extend_from_slice(data);
        }
        assert_eq!(rebuilt, blob, "{total}");
        assert_eq!(
            calls[calls.len() - 1],
            Call::Commit {
                handle: HANDLE,
                count,
                sha
            }
        );
    }
}

#[test]
fn the_reference_sizes_take_five_and_six_round_trips() {
    // 250 bytes: Begin + 3 chunks + Commit; 290 bytes: Begin + 4 chunks + Commit.
    let kms = FakeKms::new();
    drive(&kms, b"k", &filler(250)).unwrap();
    assert_eq!(kms.calls().len(), 5);
    let kms = FakeKms::new();
    drive(&kms, b"k", &filler(290)).unwrap();
    assert_eq!(kms.calls().len(), 6);
    let kms = FakeKms::new();
    drive(&kms, b"k", &filler(65_536)).unwrap();
    assert_eq!(kms.calls().len(), 685);
}

#[test]
fn a_run_of_random_sizes_reassembles_and_rechunks_identically() {
    let mut size = 1usize;
    for round in 0..40 {
        size = (size * 5 + 17 + round) % 65_536 + 1;
        let blob = filler(size);
        let first = FakeKms::new();
        drive(&first, b"k", &blob).unwrap();
        let second = FakeKms::new();
        drive(&second, b"k", &blob).unwrap();
        assert_eq!(first.calls(), second.calls(), "{size}");
        let joined: Vec<u8> = first
            .calls()
            .iter()
            .filter_map(|c| {
                if let Call::Chunk { data, .. } = c {
                    Some(data.clone())
                } else {
                    None
                }
            })
            .flatten()
            .collect();
        assert_eq!(joined, blob);
    }
}

#[test]
fn local_refusals_send_nothing() {
    let kms = FakeKms::new();
    assert_eq!(
        drive(&kms, b"k", &[]).err(),
        Some(KmsClientError::InvalidPayload)
    );
    assert_eq!(
        drive(&kms, b"k", &filler(65_537)).err(),
        Some(KmsClientError::InvalidPayload)
    );
    assert!(kms.calls().is_empty());
}

#[test]
fn every_failure_after_begin_aborts_the_pending_import() {
    let blob = filler(250);
    let error = KmsClientError::Service(KmsErrorCode::SshOrderInvalid);
    for at in [At::Chunk(0), At::Chunk(1), At::Chunk(2), At::Commit] {
        let kms = FakeKms::failing(at, error);
        assert_eq!(drive(&kms, b"k", &blob).err(), Some(error), "{at:?}");
        assert_eq!(kms.calls().last(), Some(&Call::Abort(HANDLE)), "{at:?}");
        let aborts = kms
            .calls()
            .iter()
            .filter(|c| matches!(c, Call::Abort(_)))
            .count();
        assert_eq!(aborts, 1);
        // Nothing is sent after the abort and no chunk after the failing one.
        if let At::Chunk(n) = at {
            let chunks = kms
                .calls()
                .iter()
                .filter(|c| matches!(c, Call::Chunk { .. }))
                .count();
            assert_eq!(chunks, n as usize + 1);
            assert!(!kms.calls().iter().any(|c| matches!(c, Call::Commit { .. })));
        }
    }
    // Begin itself failing has nothing to abort.
    let kms = FakeKms::failing(At::Begin, error);
    assert_eq!(drive(&kms, b"k", &blob).err(), Some(error));
    assert_eq!(kms.calls().len(), 1);
}

#[test]
fn a_timeout_ends_the_run_without_sending_more() {
    let blob = filler(250);
    for at in [At::Chunk(1), At::Commit] {
        let kms = FakeKms::failing(at, KmsClientError::Timeout);
        assert_eq!(
            drive(&kms, b"k", &blob).err(),
            Some(KmsClientError::Timeout)
        );
        assert!(
            !kms.calls().iter().any(|c| matches!(c, Call::Abort(_))),
            "{at:?}: the poisoned client cannot send"
        );
    }
}

#[test]
fn a_reply_that_deviates_is_a_mismatch_and_aborts() {
    let blob = filler(250);
    for tamper in [
        Tamper::BeginLength,
        Tamper::BeginCount,
        Tamper::BeginCapacity,
        Tamper::AckHandle,
        Tamper::AckNext,
        Tamper::AckReceived,
        Tamper::CommitNoKey,
    ] {
        let kms = FakeKms::tampered(tamper);
        assert_eq!(
            drive(&kms, b"k", &blob).err(),
            Some(KmsClientError::MismatchedResponse),
            "{tamper:?}"
        );
        assert_eq!(kms.calls().last(), Some(&Call::Abort(HANDLE)), "{tamper:?}");
    }
    // A zero handle cannot be aborted: it is refused and the run ends after Begin.
    let kms = FakeKms::tampered(Tamper::ZeroHandle);
    assert_eq!(
        drive(&kms, b"k", &blob).err(),
        Some(KmsClientError::MismatchedResponse)
    );
    assert_eq!(kms.calls().len(), 1);
    // A deviation in the very first chunk ack sends no second chunk.
    let kms = FakeKms::tampered(Tamper::AckNext);
    let _ = drive(&kms, b"k", &blob);
    assert_eq!(
        kms.calls()
            .iter()
            .filter(|c| matches!(c, Call::Chunk { .. }))
            .count(),
        1
    );
}

#[test]
fn the_import_command_runs_the_sequence_and_prints_the_report() {
    let blob = plain_blob(7);
    let kms = FakeKms::new();
    let mut console = TestConsole::new().with_block(super::typed_lines(&armor_text(&blob)));
    assert_eq!(run_cell(&["import", "lane"], &mut console, &kms), 0);
    let calls = kms.calls();
    assert!(matches!(calls[0], Call::Begin { total, .. } if total as usize == blob.len()));
    assert!(matches!(calls.last(), Some(Call::Commit { .. })));
    assert_eq!(console.block_reads, 1);
    assert!(
        console
            .out
            .starts_with("kms-ssh-admin: imported key 'lane'\n"),
        "{}",
        console.out
    );
    assert_eq!(console.err, crate::report::IMPORT_PROMPT.to_string() + "\n");
}

#[test]
fn a_cancelled_or_refused_input_sends_no_frame() {
    // Escape.
    let kms = FakeKms::new();
    let mut console = TestConsole::new().with_block(std::vec![super::key(super::ESCAPE)]);
    assert_eq!(run_cell(&["import", "lane"], &mut console, &kms), 130);
    assert!(kms.calls().is_empty());
    assert!(console.err.contains("cancelled; nothing imported"));
    // Not an armored key.
    let kms = FakeKms::new();
    let mut console = TestConsole::new().with_block(super::pasted(
        "-----BEGIN OPENSSH PRIVATE KEY-----\nAA A\n-----END OPENSSH PRIVATE KEY-----\n",
    ));
    assert_eq!(run_cell(&["import", "lane"], &mut console, &kms), 2);
    assert!(kms.calls().is_empty());
    assert!(console.err.contains("base64"), "{}", console.err);
    // Session lost while reading.
    let kms = FakeKms::new();
    let mut console = TestConsole::new().with_block(std::vec![super::text("abc")]);
    assert_eq!(run_cell(&["import", "lane"], &mut console, &kms), 125);
    assert!(kms.calls().is_empty());
    // Over the text cap.
    let kms = FakeKms::new();
    let big: Vec<_> = (0..120).map(|_| super::text(&"A".repeat(1000))).collect();
    let mut console = TestConsole::new().with_block(big);
    assert_eq!(run_cell(&["import", "lane"], &mut console, &kms), 2);
    assert!(kms.calls().is_empty());
    assert!(console.err.contains("longer than 98304"));
}

#[test]
fn a_refusal_at_commit_aborts_and_prints_nothing_to_stdout() {
    let blob = plain_blob(7);
    let kms = FakeKms::failing(
        At::Commit,
        KmsClientError::Service(KmsErrorCode::SshKeyFormatRejected),
    );
    let mut console = TestConsole::new().with_block(super::pasted(&armor_text(&blob)));
    assert_eq!(run_cell(&["import", "lane"], &mut console, &kms), 1);
    assert!(console.err.contains("key format refused"));
    assert_eq!(kms.calls().last(), Some(&Call::Abort(HANDLE)));
    assert!(console.out.is_empty());
}

#[test]
fn a_protocol_error_is_exit_one() {
    let kms = FakeKms::failing(
        At::Begin,
        KmsClientError::Wire(KmsWireError::NonCanonicalPayload),
    );
    let mut console = TestConsole::new().with_block(super::pasted(&armor_text(&plain_blob(1))));
    assert_eq!(run_cell(&["import", "lane"], &mut console, &kms), 1);
    assert!(console.err.ends_with("key service protocol error\n"));
}
