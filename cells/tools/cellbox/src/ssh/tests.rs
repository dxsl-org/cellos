//! The PTY escape (`~.`, `~~` at line start) and the local status of each ending.
use super::*;
use vissh::args::Host;
use vissh::client::HostKey;
use vissh::session::{Dispatch, Ending, Outcome, Trust};

fn feed(esc: &mut Escape, input: &[u8]) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let quit = esc.feed(input, &mut out);
    (out, quit)
}

#[test]
fn escape_only_at_line_start() {
    let mut esc = Escape::default();
    assert_eq!(feed(&mut esc, b"a~.b"), (b"a~.b".to_vec(), false));
    assert_eq!(feed(&mut esc, b"\r~~x"), (b"\r~x".to_vec(), false));
    assert_eq!(feed(&mut esc, b"\n~x"), (b"\n~x".to_vec(), false));
    // A tilde split across reads is still the escape; `~.` stops before its own bytes.
    assert_eq!(feed(&mut esc, b"\n~"), (b"\n".to_vec(), false));
    assert_eq!(feed(&mut esc, b".rest"), (Vec::new(), true));
    // At the very start of input, and Ctrl-C passes through as a byte.
    let mut esc = Escape::default();
    assert_eq!(feed(&mut esc, b"\x03~."), (b"\x03~.".to_vec(), false));
    let mut esc = Escape::default();
    assert_eq!(feed(&mut esc, b"~"), (Vec::new(), false));
    let mut out = Vec::new();
    esc.flush(&mut out);
    assert_eq!(out, b"~");
}

fn outcome(dispatch: Dispatch, ending: Ending, complete: bool) -> Outcome {
    Outcome { dispatch, ending, stdout_complete: complete, stderr_complete: complete, rekeys: 0, peaks: Default::default() }
}

#[test]
fn status_and_message_per_ending() {
    let key = HostKey { algorithm: "ssh-ed25519", key: [1; 32] };
    let host = Host::Name("h".into());
    let o = |ending| outcome(Dispatch::Accepted, ending, true);
    assert_eq!(report(&o(Ending::RemoteExit(7)), &host, 22), (7, None));
    assert_eq!(report(&o(Ending::RemoteExit(256)), &host, 22).0, 255);
    assert_eq!(report(&o(Ending::RemoteSignal("TERM".into())), &host, 22).0, 255);
    assert_eq!(report(&o(Ending::TimedOut), &host, 22).0, 124);
    let (st, msg) = report(&o(Ending::HostKey(Trust::Mismatch, key.clone())), &host, 2222);
    assert_eq!(st, 255);
    assert!(msg.unwrap().contains(&key.fingerprint()));
    for ending in [Ending::Auth, Ending::Unknown, Ending::Disconnected, Ending::RequestRefused, Ending::TransportLost] {
        assert_eq!(report(&o(ending), &host, 22).0, 255);
    }
    // Transport loss after dispatch never claims the command did not run.
    assert_eq!(report(&o(Ending::TransportLost), &host, 22).1.as_deref(), Some("connection lost; the command may have run"));
}

#[test]
fn completion_records_carry_dispatch_uncertainty_and_exact_remote_outcomes() {
    use super::{completion, settle, CANCELLED};
    use api::services::ssh_completion::{Dispatch as D, Mode, Outcome as O, Phase as P};
    let key = HostKey { algorithm: "ssh-ed25519", key: [1; 32] };
    let rec = |dispatch, ending, complete, phase| completion(&outcome(dispatch, ending, complete), Mode::Exec, phase, false);
    // Exact u32 status and bare signal name are kept; the phase is complete.
    let exit = rec(Dispatch::Accepted, Ending::RemoteExit(300), false, P::Running);
    assert_eq!((exit.phase, exit.outcome, exit.remote_status, exit.stdout_complete, exit.error_code), (P::Complete, O::RemoteExit, Some(300), false, None));
    let sig = rec(Dispatch::Accepted, Ending::RemoteSignal("TERM".into()), true, P::Running);
    assert_eq!((sig.outcome, sig.remote_signal.map(|s| String::from(s.as_str())), sig.remote_status), (O::RemoteSignal, Some("TERM".into()), None));
    // Transport loss: a local error before dispatch, unknown once the request may have run.
    for (dispatch, expect, d) in [
        (Dispatch::NotSent, O::LocalError, D::NotSent), (Dispatch::MayHaveRun, O::Unknown, D::MayHaveRun), (Dispatch::Accepted, O::Unknown, D::Accepted),
    ] {
        let r = rec(dispatch, Ending::TransportLost, false, P::Request);
        assert_eq!((r.outcome, r.dispatch, r.phase, r.error_code.map(|c| String::from(c.as_str()))), (expect, d, P::Request, Some("transport_lost".into())));
    }
    // Timeouts keep the phase that ran out; host-key and auth refusals name theirs.
    let t = rec(Dispatch::Accepted, Ending::TimedOut, false, P::Running);
    assert_eq!((t.outcome, t.phase, t.dispatch), (O::Timeout, P::Running, D::Accepted));
    let hk = rec(Dispatch::NotSent, Ending::HostKey(Trust::Mismatch, key), false, P::Trust);
    assert_eq!((hk.outcome, hk.phase, hk.error_code.map(|c| String::from(c.as_str()))), (O::LocalError, P::Trust, Some("host_key_mismatch".into())));
    // Local session loss wins over whatever the transport did.
    let lost = completion(&outcome(Dispatch::Accepted, Ending::Disconnected, false), Mode::Pty, P::Running, true);
    assert_eq!((lost.outcome, lost.phase, lost.mode), (O::SessionLost, P::Running, Mode::Pty));
    // The shell: a cancelled child keeps its last reported phase and dispatch.
    let snapshot = super::progress_record(Mode::Exec, P::Running, Dispatch::Accepted);
    let cancelled = settle(Some(snapshot), CANCELLED, Mode::Exec);
    assert_eq!((cancelled.outcome, cancelled.phase, cancelled.dispatch), (O::Cancelled, P::Running, D::Accepted));
    assert_eq!(settle(Some(exit), 255, Mode::Exec), exit);
    let none = settle(None, 127, Mode::Exec);
    assert_eq!((none.phase, none.outcome, none.dispatch), (P::Admission, O::LocalError, D::NotSent));
}

#[test]
fn first_trust_answers_map_to_session_outcomes_and_messages() {
    use super::{commit_trust, trust_added, NO_FRONTEND};
    use api::ipc::ssh_trust;
    // Only a stored key continues; an undecided wait that ran out is an expiry.
    assert_eq!(commit_trust(ssh_trust::KNOWN_EXACT), Trust::KnownExact);
    for (code, trust) in [
        (ssh_trust::MISMATCH, Trust::Mismatch), (ssh_trust::DENIED, Trust::Denied), (ssh_trust::REFUSED, Trust::Refused),
        (ssh_trust::EXPIRED, Trust::Expired), (ssh_trust::PENDING, Trust::Expired),
        (ssh_trust::UNAVAILABLE, Trust::Unavailable), (ssh_trust::UNKNOWN, Trust::Unavailable),
    ] {
        assert_eq!(commit_trust(code), trust, "code {code}");
    }
    assert_eq!(trust_added("gateway", 2222), "Host 'gateway' (2222) added to the trust store (synced, not crash-proof).");
    let key = HostKey { algorithm: "ssh-ed25519", key: [1; 32] };
    let host = Host::Name("gateway".into());
    let o = |trust| outcome(Dispatch::NotSent, Ending::HostKey(trust, key.clone()), false);
    assert_eq!(report(&o(Trust::NoConfirmation), &host, 22), (255, Some(String::from(NO_FRONTEND))));
    assert_eq!(report(&o(Trust::Expired), &host, 22).1.as_deref(), Some("host key confirmation for gateway:22 timed out; not trusted"));
    let refused = report(&o(Trust::Refused), &host, 22).1.unwrap();
    assert!(refused.contains("gateway:22") && refused.contains(&key.fingerprint()) && refused.ends_with("refused"), "{refused}");
}

// ---- publickey (K6): credential endings, key-service mapping, the unlock/sign sequence ----

use core::cell::RefCell;
use ostd::clients::kms::{KmsClientError, KmsErrorCode, KmsSshKeyCustody, SshSignResponsePayload};
use vissh::session::{Credential, SignRequest};

const ALL_CREDENTIALS: [Credential; 11] = [
    Credential::Unavailable, Credential::Denied, Credential::UnknownIdentity, Credential::UnlockCancelled,
    Credential::UnlockNeedsTerminal, Credential::UnlockFailed, Credential::UnlockLocked, Credential::Timeout,
    Credential::Protocol, Credential::InputMismatch, Credential::BadSignature,
];

#[test]
fn every_credential_has_a_distinct_short_ascii_code_and_a_secret_free_message() {
    let table: [(Credential, &str, &str); 11] = [
        (Credential::Unavailable, "kms_unavailable", "key service is not available"),
        (Credential::Denied, "kms_denied", "not permitted to use the key service"),
        (Credential::UnknownIdentity, "identity_unknown", "no such identity"),
        (Credential::UnlockCancelled, "unlock_cancelled", "unlock cancelled"),
        (Credential::UnlockNeedsTerminal, "unlock_needs_terminal", "encrypted key needs an interactive terminal"),
        (Credential::UnlockFailed, "unlock_failed", "unlock failed"),
        (Credential::UnlockLocked, "unlock_locked", "too many failed unlocks; key locked"),
        (Credential::Timeout, "kms_timeout", "key service did not answer"),
        (Credential::Protocol, "kms_protocol", "key service protocol error"),
        (Credential::InputMismatch, "kms_input_mismatch", "key service signed a different input"),
        (Credential::BadSignature, "kms_bad_signature", "key service returned an invalid signature"),
    ];
    // The table covers every variant exactly once (the match in `credential_code` is exhaustive).
    assert_eq!(table.map(|row| row.0), ALL_CREDENTIALS);
    let mut seen = Vec::new();
    for (credential, code, message) in table {
        assert_eq!((credential_code(credential), credential_message(credential, None).as_str()), (code, message));
        assert!(code.len() <= 32 && code.is_ascii() && !code.is_empty(), "{code}");
        // The completion record accepts the code as text.
        assert_eq!(sc::Text::new(code).map(|t| String::from(t.as_str())).as_deref(), Some(code));
        assert!(!seen.contains(&code), "{code} is not unique");
        seen.push(code);
    }
    // The identity the user named appears in the preflight message only.
    assert_eq!(credential_message(Credential::UnknownIdentity, Some("work-1")), "no such identity 'work-1'");
}

#[test]
fn a_credential_ending_is_a_local_error_before_dispatch_in_the_auth_phase() {
    use api::services::ssh_completion::{Dispatch as D, Mode, Outcome as O, Phase as P};
    let host = Host::Name("h".into());
    for credential in ALL_CREDENTIALS {
        let out = outcome(Dispatch::NotSent, Ending::Credential(credential), false);
        let record = completion(&out, Mode::Exec, P::Auth, false);
        assert_eq!((record.outcome, record.dispatch, record.phase, record.mode), (O::LocalError, D::NotSent, P::Auth, Mode::Exec), "{credential:?}");
        assert_eq!(record.error_code.map(|c| String::from(c.as_str())).as_deref(), Some(credential_code(credential)));
        assert_eq!((record.remote_status, record.remote_signal, record.stdout_complete), (None, None, false));
        // The local status is 255 and the message is the table's text.
        let (status, message) = report(&out, &host, 22);
        assert_eq!((status, message), (255, Some(credential_message(credential, None))));
    }
}

#[test]
fn key_service_failures_map_to_credentials() {
    use KmsClientError as E;
    use KmsErrorCode as C;
    let table = [
        (E::ServiceNotFound, Credential::Unavailable),
        (E::Service(C::SshKeyUnavailable), Credential::Unavailable),
        (E::Service(C::CallerUnattested), Credential::Denied),
        (E::Service(C::PermissionDenied), Credential::Denied),
        (E::Service(C::SshKeyNotFound), Credential::UnknownIdentity),
        (E::Service(C::SshUnlockFailed), Credential::UnlockFailed),
        (E::Service(C::SshUnlockLocked), Credential::UnlockLocked),
        (E::Service(C::InvalidSignature), Credential::BadSignature),
        (E::Timeout, Credential::Timeout),
        // Everything else is a protocol error, including a missing unlock: the client always
        // stages before it signs, so the service saying otherwise is not a user mistake.
        (E::Service(C::SshUnlockRequired), Credential::Protocol),
        (E::Service(C::InvalidRequest), Credential::Protocol),
        (E::Service(C::SshOrderInvalid), Credential::Protocol),
        (E::Service(C::SshHandleInvalid), Credential::Protocol),
        (E::Service(C::Busy), Credential::Protocol),
        (E::Ipc, Credential::Protocol),
        (E::WrongSender, Credential::Protocol),
        (E::MismatchedResponse, Credential::Protocol),
        (E::InvalidPayload, Credential::Protocol),
        (E::Wire(ostd::clients::kms::KmsWireError::NonZeroReserved), Credential::Protocol),
    ];
    for (error, credential) in table {
        assert_eq!(credential_of(&error), credential, "{error:?}");
    }
}

#[test]
fn secret_input_is_bounded_and_refuses_nul() {
    assert!(secret_accepts(0, "a"));
    assert!(secret_accepts(MAX_SECRET - 1, "a"));
    assert!(secret_accepts(0, &"x".repeat(MAX_SECRET)));
    assert!(!secret_accepts(MAX_SECRET, "a"));
    assert!(!secret_accepts(1, &"x".repeat(MAX_SECRET)));
    assert!(!secret_accepts(0, "a\0b"));
    assert_eq!(MAX_SECRET, ostd::clients::kms::KMS_SSH_SECRET_MAX);
}

/// A key service that records what it is asked.
struct Fake {
    calls: RefCell<Vec<&'static str>>,
    staged: RefCell<Vec<Vec<u8>>>,
    stage: Result<(), KmsClientError>,
    sign: Result<SshSignResponsePayload, KmsClientError>,
}

impl Fake {
    fn ok() -> Self {
        Fake {
            calls: RefCell::default(),
            staged: RefCell::default(),
            stage: Ok(()),
            sign: Ok(SshSignResponsePayload { request_id: 1, signature: [5; 64], message_sha256: [6; 32] }),
        }
    }
}

impl KeyService for Fake {
    fn stage_secret(&self, _public_key: &[u8; 32], secret: &[u8]) -> Result<(), KmsClientError> {
        self.calls.borrow_mut().push("stage");
        self.staged.borrow_mut().push(secret.to_vec());
        self.stage
    }
    fn sign(&self, _public_key: &[u8; 32], _session_id: &[u8; 32], _username: &str) -> Result<SshSignResponsePayload, KmsClientError> {
        self.calls.borrow_mut().push("sign");
        self.sign
    }
}

fn request() -> SignRequest<'static> {
    SignRequest { session_id: [1; 32], username: "tester", message_sha256: [6; 32] }
}

#[test]
fn a_plain_identity_signs_without_a_passphrase() {
    let kms = Fake::ok();
    let signed = kms_sign(&kms, &[9; 32], KmsSshKeyCustody::Plain, &request(), || panic!("a plain key is never unlocked")).unwrap();
    assert_eq!((signed.signature, signed.message_sha256), ([5; 64], [6; 32]));
    assert_eq!(*kms.calls.borrow(), ["sign"]);
}

#[test]
fn a_wrapped_identity_asks_once_stages_then_signs() {
    let kms = Fake::ok();
    let asked = RefCell::new(0u32);
    let signed = kms_sign(&kms, &[9; 32], KmsSshKeyCustody::PassphraseWrapped, &request(), || {
        *asked.borrow_mut() += 1;
        Ok(String::from("correct horse"))
    })
    .unwrap();
    assert_eq!(signed.signature, [5; 64]);
    assert_eq!((*asked.borrow(), kms.calls.borrow().clone()), (1, vec!["stage", "sign"]));
    assert_eq!(*kms.staged.borrow(), [b"correct horse".to_vec()]);
}

#[test]
fn the_unlock_asks_once_and_a_refusal_never_reaches_the_service() {
    // Escape/EOF/timeout at the prompt: no key-service call at all.
    let kms = Fake::ok();
    let cancelled = kms_sign(&kms, &[9; 32], KmsSshKeyCustody::PassphraseWrapped, &request(), || Err(Credential::UnlockCancelled));
    assert_eq!(cancelled, Err(Credential::UnlockCancelled));
    // No terminal for the prompt (a pipeline stage).
    let needs = kms_sign(&kms, &[9; 32], KmsSshKeyCustody::PassphraseWrapped, &request(), || Err(Credential::UnlockNeedsTerminal));
    assert_eq!(needs, Err(Credential::UnlockNeedsTerminal));
    // An empty passphrase is not representable on the wire: unlock_failed, locally.
    let empty = kms_sign(&kms, &[9; 32], KmsSshKeyCustody::PassphraseWrapped, &request(), || Ok(String::new()));
    assert_eq!(empty, Err(Credential::UnlockFailed));
    assert!(kms.calls.borrow().is_empty());
}

#[test]
fn a_failed_unlock_or_sign_ends_with_the_mapped_credential_and_no_retry() {
    for (stage, expect) in [
        (KmsClientError::Service(KmsErrorCode::SshUnlockLocked), Credential::UnlockLocked),
        (KmsClientError::Service(KmsErrorCode::SshKeyUnavailable), Credential::Unavailable),
        (KmsClientError::Timeout, Credential::Timeout),
    ] {
        let kms = Fake { stage: Err(stage), ..Fake::ok() };
        let result = kms_sign(&kms, &[9; 32], KmsSshKeyCustody::PassphraseWrapped, &request(), || Ok(String::from("pw")));
        // The sign is not attempted after a failed stage, and the passphrase is not offered twice.
        assert_eq!((result, kms.calls.borrow().clone()), (Err(expect), vec!["stage"]), "{stage:?}");
    }
    // The service's one answer for a wrong passphrase, a corrupt blob and a key mismatch.
    let kms = Fake { sign: Err(KmsClientError::Service(KmsErrorCode::SshUnlockFailed)), ..Fake::ok() };
    let result = kms_sign(&kms, &[9; 32], KmsSshKeyCustody::PassphraseWrapped, &request(), || Ok(String::from("wrong")));
    assert_eq!((result, kms.calls.borrow().clone()), (Err(Credential::UnlockFailed), vec!["stage", "sign"]));
    // A signing failure of a plain key, and a self-verify failure inside the service.
    for (error, expect) in [
        (KmsClientError::Service(KmsErrorCode::InvalidSignature), Credential::BadSignature),
        (KmsClientError::Service(KmsErrorCode::PermissionDenied), Credential::Denied),
        (KmsClientError::MismatchedResponse, Credential::Protocol),
    ] {
        let kms = Fake { sign: Err(error), ..Fake::ok() };
        assert_eq!(kms_sign(&kms, &[9; 32], KmsSshKeyCustody::Plain, &request(), || unreachable!()), Err(expect));
    }
}

/// Every `.rs` file under `dir`, but the test files themselves.
fn sources(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("ssh/tests.rs") {
            out.push((path.display().to_string(), std::fs::read_to_string(&path).unwrap()));
        }
    }
}

#[test]
fn cellbox_never_calls_a_key_management_operation() {
    // The admin calls (import, delete) are the admin cell's; the service answers a login
    // Cell `PermissionDenied` anyway, but cellbox must not even name them, and it has no
    // key-listing or key-import command.
    let forbidden = [concat!("ssh_", "import"), concat!("ssh_", "delete"), concat!("Begin", "SshKeyImport"), concat!("Delete", "SshKey")];
    let mut files = Vec::new();
    sources(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")), &mut files);
    assert!(files.len() > 20, "found {} sources", files.len());
    for (path, text) in files {
        for word in forbidden {
            assert!(!text.contains(word), "{path} names {word}");
        }
    }
}

#[test]
fn an_ssh_stage_is_launched_from_the_ssh_path_with_ssh_as_its_first_argument() {
    let words: Vec<String> = ["ssh", "--user", "u", "--auth", "publickey", "--identity", "k", "h", "--command", "cat > f"]
        .iter()
        .map(|w| String::from(*w))
        .collect();
    let argv = stage_argv(&words);
    assert_eq!(argv[0], SSH_ENTRY);
    assert_eq!(&argv[1..], &words[1..].iter().map(String::as_str).collect::<Vec<_>>()[..]);
    assert_eq!(argv.len(), words.len());
    assert_eq!(SSH_PATH, "/bin/cellbox-ssh");
    // The child entry takes everything after argv[0] as the ssh arguments.
    assert!(vissh::args::parse(argv.iter().skip(1).copied()).is_ok());
}
