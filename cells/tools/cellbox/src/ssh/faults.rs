//! Lane-only key-service fault seam (`kssh-faults`; K8 RT0f scenarios S05a-c).
//!
//! A remote exec command that starts `: kssh-fault MODE;` (MODE one of `session`, `user`,
//! `signature`) makes this client corrupt exactly one thing around its one KMS sign call:
//!
//! - `session`: one byte of the session id handed to the key service is flipped, so the
//!   service signs (and digests) an input for a different session;
//! - `user`: the user name handed to the key service is changed (`tester` -> `testes`, same
//!   length, still in the accepted character class), while the user on the wire stays;
//! - `signature`: one byte of the signature the key service returned is flipped.
//!
//! The client's own verification is not touched: the session compares the returned
//! `message_sha256` with the digest of the input it built (`kms_input_mismatch`) and then
//! verifies the signature (`kms_bad_signature`); that check is what refuses, before anything
//! is sent to the server. Only the sign call is mutated; the unlock/stage call, the
//! preflight and every other connection of the process are not.
//!
//! The command is also what the remote shell runs: `: kssh-fault MODE; true` is a no-op
//! prefix followed by a trivial command, the same shape as the `p09-faults` prefixes.
//!
//! Prefix rules: a lane build that also has `p09-faults` reads its own prefixes only at the
//! very start of the command, so a command carrying both puts the `p09` prefix first
//! (`: p09-sink; : kssh-fault signature; true`). This parser therefore skips leading
//! `: p09-fault ...;` / `: p09-sink;` segments and then reads the kssh prefix; a command
//! that begins with the kssh prefix is read directly. An unknown MODE, a prefix without
//! the closing `;`, or no prefix at all selects no fault: the client behaves exactly as the
//! product client (the same leniency as the `p09` prefix parser). A misspelt fault then
//! shows as a successful login, which the lane judge counts as a failed scenario.
use alloc::string::String;

use ostd::clients::kms::{KmsClientError, SshSignResponsePayload};

use super::KeyService;

/// Announced once per ssh invocation (kernel log), like `rekey=lane`.
pub const ANNOUNCE: &str = "P04 INFO ssh-test-config kssh-faults=on\n";

const KSSH_PREFIX: &str = ": kssh-fault ";
const P09_FAULT_PREFIX: &str = ": p09-fault ";
const P09_SINK_PREFIX: &str = ": p09-sink;";

/// Which single mutation this invocation applies to its sign call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Session,
    User,
    Signature,
}

impl Mode {
    fn named(name: &str) -> Option<Mode> {
        match name.trim() {
            "session" => Some(Mode::Session),
            "user" => Some(Mode::User),
            "signature" => Some(Mode::Signature),
            _ => None,
        }
    }

    /// The fault the exec `command` selects (see the module text for the prefix rules).
    pub fn of_command(command: Option<&str>) -> Option<Mode> {
        let mut rest = command?;
        loop {
            if let Some(spec) = rest.strip_prefix(KSSH_PREFIX) {
                return Mode::named(spec.split_once(';')?.0);
            }
            if rest.starts_with(P09_FAULT_PREFIX) || rest.starts_with(P09_SINK_PREFIX) {
                rest = rest.split_once(';')?.1.trim_start();
                continue;
            }
            return None;
        }
    }
}

/// One byte changed: the lowest bit of byte 0 (the position does not matter to the check).
const FLIP: u8 = 0x01;

/// `session_id` with exactly one byte changed.
pub fn flip_session(session_id: &[u8; 32]) -> [u8; 32] {
    let mut out = *session_id;
    out[0] ^= FLIP;
    out
}

/// `signature` with exactly one byte changed.
pub fn flip_signature(signature: &[u8; 64]) -> [u8; 64] {
    let mut out = *signature;
    out[0] ^= FLIP;
    out
}

/// `username` with its last byte changed to a different one of the accepted class
/// (`[A-Za-z0-9._-]`): the next letter or digit (`tester` -> `testes`), the previous one
/// at the ends of a run (`z`, `Z`, `9`), `x` for punctuation. Same length, always different.
pub fn change_user(username: &str) -> String {
    let mut bytes = username.as_bytes().to_vec();
    match bytes.last_mut() {
        Some(last) => {
            *last = match *last {
                b'z' | b'Z' | b'9' => *last - 1,
                b'a'..=b'y' | b'A'..=b'Y' | b'0'..=b'8' => *last + 1,
                _ => b'x',
            }
        }
        None => bytes.push(b'x'),
    }
    // Only ASCII bytes were written; a multi-byte tail cannot occur for an accepted user.
    String::from_utf8(bytes).unwrap_or_else(|_| String::from("x"))
}

/// The key service as the login sees it with a fault selected: every call passes through
/// except the sign call, which is mutated as `mode` says.
pub struct Faulted<'a, K: KeyService> {
    inner: &'a K,
    mode: Option<Mode>,
}

impl<'a, K: KeyService> Faulted<'a, K> {
    pub fn new(inner: &'a K, mode: Option<Mode>) -> Self {
        Faulted { inner, mode }
    }
}

impl<K: KeyService> KeyService for Faulted<'_, K> {
    fn stage_secret(&self, public_key: &[u8; 32], secret: &[u8]) -> Result<(), KmsClientError> {
        self.inner.stage_secret(public_key, secret)
    }

    fn sign(&self, public_key: &[u8; 32], session_id: &[u8; 32], username: &str) -> Result<SshSignResponsePayload, KmsClientError> {
        match self.mode {
            None => self.inner.sign(public_key, session_id, username),
            Some(Mode::Session) => self.inner.sign(public_key, &flip_session(session_id), username),
            Some(Mode::User) => self.inner.sign(public_key, session_id, &change_user(username)),
            Some(Mode::Signature) => {
                let mut signed = self.inner.sign(public_key, session_id, username)?;
                signed.signature = flip_signature(&signed.signature);
                Ok(signed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::kms_sign;
    use alloc::vec::Vec;
    use core::cell::RefCell;
    use ostd::clients::kms::KmsSshKeyCustody;
    use vissh::session::{Credential, SignRequest, Signed};
    use vissh::sunset::ed25519_dalek::{Signer, SigningKey};
    use vissh::userauth;

    fn mode(command: &str) -> Option<Mode> {
        Mode::of_command(Some(command))
    }

    #[test]
    fn each_mode_is_selected_by_its_prefix_and_nothing_else() {
        assert_eq!(mode(": kssh-fault session; true"), Some(Mode::Session));
        assert_eq!(mode(": kssh-fault user; true"), Some(Mode::User));
        assert_eq!(mode(": kssh-fault signature; true"), Some(Mode::Signature));
        // The prefix alone, and a command that follows without a space.
        assert_eq!(mode(": kssh-fault user;"), Some(Mode::User));
        assert_eq!(mode(": kssh-fault signature;true"), Some(Mode::Signature));
    }

    #[test]
    fn no_prefix_an_unknown_mode_or_a_missing_semicolon_selects_no_fault() {
        assert_eq!(Mode::of_command(None), None);
        for command in [
            "",
            "true",
            "echo : kssh-fault session; true",
            " : kssh-fault session; true",
            ":kssh-fault session; true",
            ": kssh-fault session true",
            ": kssh-fault session",
            ": kssh-fault ;",
            ": kssh-fault bogus; true",
            ": kssh-fault Session; true",
            ": kssh-fault session user; true",
            ": kssh-fault ",
            ": kssh-fault",
        ] {
            assert_eq!(mode(command), None, "{command:?}");
        }
    }

    #[test]
    fn the_p09_lane_prefixes_do_not_hide_or_get_hidden_by_the_kssh_prefix() {
        // The p09 prefix stays first (its parser reads the start of the command only).
        assert_eq!(mode(": p09-sink; : kssh-fault signature; true"), Some(Mode::Signature));
        assert_eq!(mode(": p09-fault accepted hold; : kssh-fault user; true"), Some(Mode::User));
        assert_eq!(mode(": p09-sink; : p09-fault accepted hold; : kssh-fault session;true"), Some(Mode::Session));
        // Only a p09 prefix, or a p09 prefix that is not closed: no kssh fault.
        assert_eq!(mode(": p09-sink; true"), None);
        assert_eq!(mode(": p09-fault accepted hold; true"), None);
        assert_eq!(mode(": p09-sink"), None);
        // The prefix elsewhere in the command is plain text.
        assert_eq!(mode(": p09-sink; echo : kssh-fault user;"), None);
    }

    #[test]
    fn the_session_flip_changes_exactly_one_byte_of_the_session_id() {
        let id: [u8; 32] = core::array::from_fn(|i| (i as u8).wrapping_mul(7));
        let flipped = flip_session(&id);
        assert_eq!(id.iter().zip(&flipped).filter(|(a, b)| a != b).count(), 1);
        assert_ne!(id, flipped);
        // Flipping is its own inverse, so the mutation never depends on the value.
        assert_eq!(flip_session(&flipped), id);
        assert_ne!(flip_session(&[0; 32]), [0; 32]);
        assert_ne!(flip_session(&[0xff; 32]), [0xff; 32]);
    }

    #[test]
    fn the_signature_flip_changes_exactly_one_byte_of_the_signature() {
        let signature: [u8; 64] = core::array::from_fn(|i| (i as u8) ^ 0x5a);
        let flipped = flip_signature(&signature);
        assert_eq!(signature.iter().zip(&flipped).filter(|(a, b)| a != b).count(), 1);
        assert_eq!(flip_signature(&flipped), signature);
    }

    #[test]
    fn the_user_change_keeps_length_and_class_and_changes_the_name() {
        assert_eq!(change_user("tester"), "testes");
        for name in ["tester", "a", "z", "Z", "9", "x9", "a.b", "a-", "a_", "az", "ZZ", "0", "user.name-1_x"] {
            let changed = change_user(name);
            assert_eq!(changed.len(), name.len(), "{name}");
            assert_ne!(changed, name, "{name}");
            assert!(changed.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')), "{changed}");
            // Only the last byte moves.
            assert_eq!(changed[..name.len() - 1], name[..name.len() - 1]);
            assert!(ostd::clients::kms::validate_ssh_username(changed.as_bytes()), "{changed}");
        }
    }

    /// The key service as the real one behaves: it builds the signing input from the
    /// session id and user name it is *given*, signs it with the real key and answers with
    /// the digest of what it signed.
    struct Service {
        key: SigningKey,
        public: [u8; 32],
        asked: RefCell<Vec<([u8; 32], String)>>,
        staged: RefCell<u32>,
    }

    impl Service {
        fn new() -> Self {
            let key = SigningKey::from_bytes(&[0x42; 32]);
            let public = key.verifying_key().to_bytes();
            Service { key, public, asked: RefCell::default(), staged: RefCell::default() }
        }
    }

    impl KeyService for Service {
        fn stage_secret(&self, _public_key: &[u8; 32], _secret: &[u8]) -> Result<(), KmsClientError> {
            *self.staged.borrow_mut() += 1;
            Ok(())
        }
        fn sign(&self, public_key: &[u8; 32], session_id: &[u8; 32], username: &str) -> Result<SshSignResponsePayload, KmsClientError> {
            assert_eq!(public_key, &self.public);
            self.asked.borrow_mut().push((*session_id, String::from(username)));
            let input = userauth::signing_input(session_id, username, public_key).expect("an accepted user name");
            Ok(SshSignResponsePayload {
                request_id: 1,
                signature: self.key.sign(&input).to_bytes(),
                message_sha256: userauth::digest(&input),
            })
        }
    }

    const SESSION: [u8; 32] = [0x11; 32];
    const USER: &str = "tester";

    /// What the client built for the server: the input the key service has to sign.
    fn request(public: &[u8; 32]) -> (SignRequest<'static>, Vec<u8>) {
        let input = userauth::signing_input(&SESSION, USER, public).unwrap();
        let request = SignRequest { session_id: SESSION, username: USER, message_sha256: userauth::digest(&input) };
        (request, input.to_vec())
    }

    /// The two checks of the session on a key-service reply (`vissh` `answer_sign`):
    /// the digest must be that of the expected input, then the signature must verify.
    fn client_verdict(public: &[u8; 32], request: &SignRequest<'_>, input: &[u8], signed: &Signed) -> Option<Credential> {
        if signed.message_sha256 != request.message_sha256 {
            return Some(Credential::InputMismatch);
        }
        if !userauth::verify(public, input, &signed.signature) {
            return Some(Credential::BadSignature);
        }
        None
    }

    fn login(fault: Option<Mode>, custody: KmsSshKeyCustody) -> (Option<Credential>, Service) {
        let service = Service::new();
        let (request, input) = request(&service.public);
        let faulted = Faulted::new(&service, fault);
        let signed = kms_sign(&faulted, &service.public, custody, &request, || Ok(String::from("correct horse"))).unwrap();
        let verdict = client_verdict(&service.public, &request, &input, &signed);
        (verdict, service)
    }

    #[test]
    fn without_a_fault_the_wrapper_is_the_product_client() {
        for custody in [KmsSshKeyCustody::Plain, KmsSshKeyCustody::PassphraseWrapped] {
            let (verdict, service) = login(None, custody);
            assert_eq!(verdict, None);
            assert_eq!(*service.asked.borrow(), [(SESSION, String::from(USER))]);
        }
    }

    #[test]
    fn a_session_fault_is_refused_by_the_digest_compare() {
        let (verdict, service) = login(Some(Mode::Session), KmsSshKeyCustody::Plain);
        assert_eq!(verdict, Some(Credential::InputMismatch));
        // The service saw one changed byte of the session id and the real user.
        let asked = service.asked.borrow();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0], (flip_session(&SESSION), String::from(USER)));
    }

    #[test]
    fn a_user_fault_is_refused_by_the_digest_compare() {
        let (verdict, service) = login(Some(Mode::User), KmsSshKeyCustody::Plain);
        assert_eq!(verdict, Some(Credential::InputMismatch));
        // The service saw the real session id and the changed user; the request the
        // client holds (the wire user) is untouched.
        assert_eq!(*service.asked.borrow(), [(SESSION, String::from("testes"))]);
        assert_eq!(USER, "tester");
    }

    #[test]
    fn a_signature_fault_passes_the_digest_compare_and_is_refused_by_the_verify() {
        let (verdict, service) = login(Some(Mode::Signature), KmsSshKeyCustody::Plain);
        assert_eq!(verdict, Some(Credential::BadSignature));
        // The service itself was asked with the real values, and it answered correctly:
        // only the returned signature was changed.
        assert_eq!(*service.asked.borrow(), [(SESSION, String::from(USER))]);
        let (request, input) = request(&service.public);
        let honest = service.sign(&service.public, &SESSION, USER).unwrap();
        assert_eq!(client_verdict(&service.public, &request, &input, &Signed { signature: honest.signature, message_sha256: honest.message_sha256 }), None);
        let faulted = Faulted::new(&service, Some(Mode::Signature)).sign(&service.public, &SESSION, USER).unwrap();
        assert_eq!(honest.signature.iter().zip(&faulted.signature).filter(|(a, b)| a != b).count(), 1);
        assert_eq!((honest.message_sha256, honest.request_id), (faulted.message_sha256, faulted.request_id));
    }

    #[test]
    fn the_fault_applies_to_the_sign_call_only_and_the_unlock_runs_as_usual() {
        for fault in [Mode::Session, Mode::User, Mode::Signature] {
            let (verdict, service) = login(Some(fault), KmsSshKeyCustody::PassphraseWrapped);
            assert!(verdict.is_some(), "{fault:?}");
            // The passphrase was staged once, unmodified in count, before the single sign.
            assert_eq!(*service.staged.borrow(), 1, "{fault:?}");
            assert_eq!(service.asked.borrow().len(), 1, "{fault:?}");
        }
    }

    #[test]
    fn a_failed_sign_stays_the_service_error_in_every_mode() {
        struct Failing;
        impl KeyService for Failing {
            fn stage_secret(&self, _: &[u8; 32], _: &[u8]) -> Result<(), KmsClientError> {
                Ok(())
            }
            fn sign(&self, _: &[u8; 32], _: &[u8; 32], _: &str) -> Result<SshSignResponsePayload, KmsClientError> {
                Err(KmsClientError::Timeout)
            }
        }
        for fault in [None, Some(Mode::Session), Some(Mode::User), Some(Mode::Signature)] {
            let result = Faulted::new(&Failing, fault).sign(&[0; 32], &[0; 32], USER);
            assert_eq!(result.err(), Some(KmsClientError::Timeout), "{fault:?}");
        }
    }

    #[test]
    fn the_announcement_is_the_line_the_driver_looks_for() {
        assert_eq!(ANNOUNCE, "P04 INFO ssh-test-config kssh-faults=on\n");
    }
}
