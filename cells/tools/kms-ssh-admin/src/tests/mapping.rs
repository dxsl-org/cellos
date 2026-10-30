//! Error mapping table and the command-line grammar.

use ostd::clients::kms::{KmsClientError, KmsErrorCode, KmsWireError};

use super::{run_cell, FakeKms, TestConsole};
use crate::report::{kms_failure, EXIT_REFUSED, EXIT_TIMEOUT};
use crate::{parse, Command, UsageError};

#[test]
fn the_draft_rows_map_to_their_fixed_messages() {
    let rows: [(KmsErrorCode, &str); 12] = [
        (KmsErrorCode::SshKeyFormatRejected, "key format refused (openssh-key-v1 ed25519; none, or bcrypt + aes256-ctr only)"),
        (KmsErrorCode::SshImportIntegrity, "transfer corrupted; try again"),
        (KmsErrorCode::SshImportPending, "an import is already open for this caller; try again"),
        (KmsErrorCode::SshKeyUnavailable, "key service not serving"),
        (KmsErrorCode::PermissionDenied, "not an admin caller (admin flag or policy missing)"),
        (KmsErrorCode::CallerUnattested, "not an admin caller (admin flag or policy missing)"),
        (KmsErrorCode::SshKeyExists, "an identity with this name or public key already exists; delete the existing identity first"),
        (KmsErrorCode::SshStoreFull, "the key store is full: delete the existing identity first, or the key exceeds the store budget"),
        (KmsErrorCode::SshKeyNotFound, "no such key"),
        (KmsErrorCode::SshHandleInvalid, "transfer handle refused by the key service; try again"),
        (KmsErrorCode::SshOrderInvalid, "transfer order refused by the key service; try again"),
        (KmsErrorCode::InvalidRequest, "request refused as invalid"),
    ];
    for (code, message) in rows {
        assert_eq!(
            kms_failure(KmsClientError::Service(code)),
            (EXIT_REFUSED, std::format!("kms-ssh-admin: {message}\n")),
            "{code:?}"
        );
    }
}

#[test]
fn every_service_error_code_has_one_fixed_single_line_message_and_exit_one() {
    let mut seen = 0;
    for raw in 0u16..=200 {
        let Ok(code) = KmsErrorCode::try_from(raw) else {
            continue;
        };
        seen += 1;
        let (exit, message) = kms_failure(KmsClientError::Service(code));
        assert_eq!(exit, EXIT_REFUSED, "{code:?}");
        assert!(
            message.starts_with("kms-ssh-admin: ") && message.ends_with('\n'),
            "{message:?}"
        );
        assert_eq!(message.matches('\n').count(), 1);
        assert!(message.is_ascii());
        // Deterministic: the same code always maps to the same text.
        assert_eq!(kms_failure(KmsClientError::Service(code)), (exit, message));
    }
    assert!(
        seen >= 38,
        "the SSH codes 27-38 are among the known ones: {seen}"
    );
    // A code without its own row prints its own name.
    assert_eq!(
        kms_failure(KmsClientError::Service(KmsErrorCode::SshUnlockFailed)).1,
        "kms-ssh-admin: key service refused: SshUnlockFailed\n"
    );
}

#[test]
fn transport_and_protocol_errors_map_to_fixed_messages() {
    assert_eq!(
        kms_failure(KmsClientError::Timeout),
        (
            EXIT_TIMEOUT,
            "kms-ssh-admin: the key service did not answer in time\n".into()
        )
    );
    assert_eq!(
        kms_failure(KmsClientError::ServiceNotFound),
        (1, "kms-ssh-admin: key service not found\n".into())
    );
    assert_eq!(
        kms_failure(KmsClientError::Ipc),
        (1, "kms-ssh-admin: key service transport error\n".into())
    );
    assert_eq!(
        kms_failure(KmsClientError::WrongSender),
        (
            1,
            "kms-ssh-admin: the reply did not come from the key service\n".into()
        )
    );
    for error in [
        KmsClientError::MismatchedResponse,
        KmsClientError::InvalidPayload,
        KmsClientError::Wire(KmsWireError::NonCanonicalPayload),
        KmsClientError::Wire(KmsWireError::UnknownErrorCode(99)),
    ] {
        assert_eq!(
            kms_failure(error),
            (1, "kms-ssh-admin: key service protocol error\n".into()),
            "{error:?}"
        );
    }
}

#[test]
fn a_failed_connect_is_reported_and_runs_nothing() {
    let mut console = TestConsole::new();
    let code = crate::run(&["status"], &mut console, || {
        Err::<super::Ref<'_, FakeKms>, _>(KmsClientError::ServiceNotFound)
    });
    assert_eq!(code, 1);
    assert_eq!(console.err, "kms-ssh-admin: key service not found\n");
}

#[test]
fn the_grammar_is_exactly_status_import_delete() {
    assert_eq!(parse(&["status"]), Ok(Command::Status(None)));
    assert_eq!(
        parse(&["status", "lane"]),
        Ok(Command::Status(Some("lane")))
    );
    assert_eq!(parse(&["import", "a_b-C9"]), Ok(Command::Import("a_b-C9")));
    assert_eq!(parse(&["delete", "lane"]), Ok(Command::Delete("lane")));
    for args in [
        &[][..],
        &["list"],
        &["export", "lane"],
        &["sign", "lane"],
        &["unlock", "lane"],
        &["import"],
        &["delete"],
        &["import", "lane", "extra"],
        &["status", "a", "b"],
        &["--help"],
        &["IMPORT", "lane"],
    ] {
        assert!(parse(args).is_err(), "{args:?}");
    }
    assert_eq!(parse(&["list"]), Err(UsageError::Grammar));
    let long = "a".repeat(65);
    for name in ["", " ", "a b", "a.b", "a/b", "é", long.as_str(), "a\n"] {
        assert_eq!(parse(&["import", name]), Err(UsageError::Name), "{name:?}");
    }
    let longest = "a".repeat(64);
    assert_eq!(parse(&["import", &longest]), Ok(Command::Import(&longest)));
}

#[test]
fn a_usage_error_prints_usage_exits_two_and_never_connects() {
    let mut console = TestConsole::new();
    let code = crate::run(
        &["nope"],
        &mut console,
        || -> Result<super::Ref<'_, FakeKms>, _> { panic!("connected") },
    );
    assert_eq!(code, 2);
    assert_eq!(
        console.err,
        "usage: kms-ssh-admin status [NAME] | import NAME | delete NAME\n"
    );
    let kms = FakeKms::new();
    let mut console = TestConsole::new();
    assert_eq!(run_cell(&[], &mut console, &kms), 2);
    assert!(kms.calls().is_empty());
}

#[test]
fn the_no_terminal_refusal_text_is_fixed() {
    assert_eq!(
        crate::report::NO_TERMINAL,
        "kms-ssh-admin: an interactive terminal session is required\n"
    );
    assert_eq!(crate::report::EXIT_NO_TERMINAL, 125);
    assert_eq!(
        (
            crate::report::EXIT_OK,
            crate::report::EXIT_USAGE,
            crate::report::EXIT_CANCELLED
        ),
        (0, 2, 130)
    );
}
