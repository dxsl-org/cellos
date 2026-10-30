//! `delete`: status first, typed confirmation, op 20 only after it.

use ostd::clients::kms::{KmsClientError, KmsCustodyPosture, KmsErrorCode, KmsSshKeyCustody};

use super::{key, run_cell, status, text, At, Call, FakeKms, TestConsole, ENTER, ESCAPE, PUBLIC};
use crate::report::{DELETE_CANCELLED, DELETE_PROMPT};

fn stored() -> FakeKms {
    FakeKms::new().with_stored(status(
        KmsSshKeyCustody::Plain,
        KmsCustodyPosture::DevPostureVolatile,
        [0x11; 32],
    ))
}

fn confirm() -> Vec<terminal_sdk::input::EventKind> {
    std::vec![text("delete"), key(ENTER)]
}

fn op20(kms: &FakeKms) -> usize {
    kms.calls()
        .iter()
        .filter(|c| matches!(c, Call::Delete { .. }))
        .count()
}

#[test]
fn a_confirmed_delete_reads_status_then_deletes_with_the_stored_public_key() {
    let kms = stored();
    let mut console = TestConsole::new().with_word(confirm());
    assert_eq!(run_cell(&["delete", "lane"], &mut console, &kms), 0);
    assert_eq!(
        kms.calls(),
        std::vec![
            Call::Status(b"lane".to_vec()),
            Call::Delete {
                name: b"lane".to_vec(),
                public_key: PUBLIC
            }
        ]
    );
    assert!(console
        .out
        .starts_with("kms-ssh-admin: delete key 'lane'\n  custody: plain\n  fingerprint: SHA256:"));
    assert!(
        console.out.ends_with("kms-ssh-admin: deleted key 'lane'\n"),
        "{}",
        console.out
    );
    assert!(console.err.starts_with(DELETE_PROMPT));
}

#[test]
fn without_the_exact_word_there_is_no_op_20() {
    for events in [
        std::vec![key(ENTER)],
        std::vec![text("yes"), key(ENTER)],
        std::vec![text("Delete"), key(ENTER)],
        std::vec![text("delete "), key(ENTER)],
        std::vec![text("delete"), key(ESCAPE)],
        std::vec![text("delete"), terminal_sdk::input::EventKind::Interrupt],
        std::vec![
            terminal_sdk::input::EventKind::PasteBegin,
            text("delete"),
            terminal_sdk::input::EventKind::PasteEnd,
            key(ENTER)
        ],
    ] {
        let kms = stored();
        let mut console = TestConsole::new().with_word(events);
        assert_eq!(run_cell(&["delete", "lane"], &mut console, &kms), 130);
        assert_eq!(op20(&kms), 0);
        assert_eq!(kms.calls().len(), 1, "only the status call");
        assert!(console.err.ends_with(DELETE_CANCELLED), "{}", console.err);
    }
}

#[test]
fn a_lost_session_at_the_prompt_is_125_and_deletes_nothing() {
    let kms = stored();
    let mut console = TestConsole::new().with_word(std::vec![text("del")]);
    assert_eq!(run_cell(&["delete", "lane"], &mut console, &kms), 125);
    assert_eq!(op20(&kms), 0);
}

#[test]
fn an_unknown_key_is_refused_before_any_prompt() {
    let kms = FakeKms::new();
    let mut console = TestConsole::new().with_word(confirm());
    assert_eq!(run_cell(&["delete", "lane"], &mut console, &kms), 1);
    assert_eq!(console.word_reads, 0);
    assert_eq!(op20(&kms), 0);
    assert_eq!(console.err, "kms-ssh-admin: no such key\n");
}

#[test]
fn a_refused_delete_prints_the_mapped_message() {
    let kms = stored();
    kms.fail.set(Some((
        At::Delete,
        KmsClientError::Service(KmsErrorCode::SshKeyNotFound),
    )));
    let mut console = TestConsole::new().with_word(confirm());
    assert_eq!(run_cell(&["delete", "lane"], &mut console, &kms), 1);
    assert!(console.err.ends_with("kms-ssh-admin: no such key\n"));
    assert!(!console.out.contains("deleted key"));
    let kms = stored();
    kms.fail.set(Some((At::Delete, KmsClientError::Timeout)));
    let mut console = TestConsole::new().with_word(confirm());
    assert_eq!(run_cell(&["delete", "lane"], &mut console, &kms), 124);
}

#[test]
fn a_status_failure_never_reaches_the_prompt() {
    let kms = stored();
    kms.fail.set(Some((
        At::Status,
        KmsClientError::Service(KmsErrorCode::PermissionDenied),
    )));
    let mut console = TestConsole::new().with_word(confirm());
    assert_eq!(run_cell(&["delete", "lane"], &mut console, &kms), 1);
    assert_eq!(console.word_reads, 0);
    assert!(console.err.contains("not an admin caller"));
}
