//! Exact output lines, and the property that no key byte is ever printed.

use std::string::String;

use ostd::clients::kms::{KmsClientError, KmsCustodyPosture, KmsErrorCode, KmsSshKeyCustody};

use super::{
    armor_text, filler, pasted, plain_blob, run_cell, status, wrapped_blob, At, FakeKms,
    TestConsole,
};
use crate::report::{self, digest_prefix, fingerprint};

/// Independent standard base64 without padding.
fn b64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

fn import_with(
    custody: KmsSshKeyCustody,
    posture: KmsCustodyPosture,
    blob: &[u8],
) -> (i32, TestConsole) {
    let mut kms = FakeKms::new();
    kms.commit_status = status(custody, posture, [0xAB; 32]);
    let mut console = TestConsole::new().with_block(pasted(&armor_text(blob)));
    let code = run_cell(&["import", "lane"], &mut console, &kms);
    (code, console)
}

#[test]
fn fingerprint_and_digest_prefix_follow_ssh_keygen_and_fips() {
    assert_eq!(
        fingerprint(&[0xAB; 32]),
        std::format!("SHA256:{}", b64(&[0xAB; 32]))
    );
    assert_eq!(
        fingerprint(&[0; 32]),
        std::format!("SHA256:{}", "A".repeat(43))
    );
    // FIPS 180-4 "abc".
    let abc = ostd::session::sha256(b"abc");
    assert_eq!(digest_prefix(&abc), "ba7816bf8f01cfea");
}

#[test]
fn a_plain_t0_import_prints_exactly_these_lines() {
    let (code, console) = import_with(
        KmsSshKeyCustody::Plain,
        KmsCustodyPosture::DevPostureVolatile,
        b"abc",
    );
    assert_eq!(code, 0);
    assert_eq!(
        console.out,
        std::format!(
            "kms-ssh-admin: imported key 'lane'\n  custody: plain\n  fingerprint: SHA256:{}\n  posture: DEV_POSTURE volatile: lost when the key service restarts\n  blob: 3 bytes, sha256 prefix ba7816bf8f01cfea\n  WARNING: unencrypted key held by the key service (development posture)\n",
            b64(&[0xAB; 32])
        )
    );
    assert_eq!(console.err, std::format!("{}\n", report::IMPORT_PROMPT));
}

#[test]
fn a_wrapped_t1_import_has_no_plain_warning() {
    let (code, console) = import_with(
        KmsSshKeyCustody::PassphraseWrapped,
        KmsCustodyPosture::DevPosturePersistent,
        b"abc",
    );
    assert_eq!(code, 0);
    assert_eq!(
        console.out,
        std::format!(
            "kms-ssh-admin: imported key 'lane'\n  custody: passphrase-wrapped\n  fingerprint: SHA256:{}\n  posture: DEV_POSTURE persistent: no rollback or crash-durability claim\n  blob: 3 bytes, sha256 prefix ba7816bf8f01cfea\n",
            b64(&[0xAB; 32])
        )
    );
}

#[test]
fn plain_wrapped_t0_t1_cover_all_four_combinations_and_the_warning_is_plain_only() {
    for custody in [KmsSshKeyCustody::Plain, KmsSshKeyCustody::PassphraseWrapped] {
        for posture in [
            KmsCustodyPosture::DevPostureVolatile,
            KmsCustodyPosture::DevPosturePersistent,
            KmsCustodyPosture::Unassessed,
        ] {
            let (code, console) = import_with(custody, posture, &plain_blob(3));
            assert_eq!(code, 0);
            assert_eq!(
                console.out.contains("WARNING"),
                custody == KmsSshKeyCustody::Plain
            );
            assert!(console.out.contains(report::posture_text(posture)));
            assert!(console.out.contains(report::custody_name(custody)));
        }
    }
}

#[test]
fn status_lines_are_exact() {
    let stored = status(
        KmsSshKeyCustody::Plain,
        KmsCustodyPosture::DevPostureVolatile,
        [0x11; 32],
    );
    let kms = FakeKms::new().with_stored(stored);
    let mut console = TestConsole::new();
    assert_eq!(run_cell(&["status"], &mut console, &kms), 0);
    assert_eq!(
        console.out,
        "kms-ssh-admin: key service ssh leaf\n  readiness: Ready\n  keys: 1\n  posture: DEV_POSTURE volatile: lost when the key service restarts\n"
    );
    let mut console = TestConsole::new();
    assert_eq!(run_cell(&["status", "lane"], &mut console, &kms), 0);
    assert_eq!(
        console.out,
        std::format!(
            "kms-ssh-admin: key 'lane'\n  custody: plain\n  fingerprint: SHA256:{}\n  posture: DEV_POSTURE volatile: lost when the key service restarts\n",
            b64(&[0x11; 32])
        )
    );
    assert!(console.err.is_empty());
}

#[test]
fn no_key_byte_appears_in_any_output() {
    // Over many pseudo-random blobs and every ending (success, KMS refusal, timeout, bad
    // armor, cancel), neither stream carries a base64 body line, a window of the raw blob as
    // hex, or a window of it as base64.
    for round in 0..24usize {
        let blob = filler(1 + round * 41);
        let armored = armor_text(&blob);
        let body_lines: Vec<&str> = armored
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .collect();
        let hex: String = blob.iter().map(|b| std::format!("{b:02x}")).collect();
        let sha = digest_prefix(&ostd::session::sha256(&blob));
        let scenarios: Vec<(Option<(At, KmsClientError)>, bool)> = std::vec![
            (None, true),
            (
                Some((
                    At::Begin,
                    KmsClientError::Service(KmsErrorCode::SshKeyFormatRejected)
                )),
                true
            ),
            (
                Some((
                    At::Commit,
                    KmsClientError::Service(KmsErrorCode::SshImportIntegrity)
                )),
                true
            ),
            (Some((At::Chunk(0), KmsClientError::Timeout)), true),
            (None, false),
        ];
        for (fail, valid) in scenarios {
            let kms = FakeKms::new();
            kms.fail.set(fail);
            let text = if valid {
                armored.clone()
            } else {
                armored.replacen("QQ", "Q ", 1).replacen('A', " ", 1)
            };
            let mut console = TestConsole::new().with_block(pasted(&text));
            let _ = run_cell(&["import", "lane"], &mut console, &kms);
            for output in [&console.out, &console.err] {
                for line in &body_lines {
                    assert!(
                        !output.contains(&line[..line.len().min(12)]),
                        "round {round}: body line in output"
                    );
                }
                for window in hex.as_bytes().windows(16).step_by(7) {
                    let window = std::str::from_utf8(window).unwrap();
                    // The one allowed hex run is the SHA-256 prefix the cell prints on purpose.
                    if window != sha {
                        assert!(
                            !output.contains(window),
                            "round {round}: raw blob bytes in output"
                        );
                    }
                }
                for window in blob.windows(9).step_by(5) {
                    assert!(
                        !output.contains(&b64(window)),
                        "round {round}: base64 of blob bytes in output"
                    );
                }
            }
        }
    }
}

#[test]
fn the_key_name_is_the_only_input_text_printed() {
    let (_, console) = import_with(
        KmsSshKeyCustody::Plain,
        KmsCustodyPosture::DevPostureVolatile,
        &wrapped_blob(),
    );
    assert!(console.out.contains("'lane'"));
    // A name KMS would refuse is refused locally with a fixed message that does not echo it.
    let kms = FakeKms::new();
    let mut console = TestConsole::new();
    assert_eq!(run_cell(&["import", "bad name!"], &mut console, &kms), 2);
    assert!(!console.err.contains("bad name"));
    assert!(kms.calls().is_empty());
}
