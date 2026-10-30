//! `cellbox ssh` (Phase 09, command-contracts §4).
//!
//! The shell builtin validates argv and re-launches this same ELF from its second
//! install path `/bin/cellbox-ssh` with argv `["ssh", ...]` (D79-9 as revised): only
//! that kernel-resolved path edge carries the outbound client rights, so the shell
//! itself never holds network authority. The child entry below runs in that Cell.
//!
//! Host trust comes from the VFS store (S9-3). An unknown key is offered for first trust
//! (S9-4c): VFS registers a challenge with the input owner, which itself prompts on this
//! session and captures the answer; this client only begins the confirmation and polls
//! VFS for the stored result, never reading the answer. Nothing beyond KEX is sent before
//! the key is trusted.
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use api::services::ssh_completion as sc;
use ostd::clients::kms::{KmsClient, KmsClientError, KmsErrorCode, KmsSshKeyCustody, SshSignResponsePayload};
use vissh::args::{self, Host};
use vissh::session::{Credential, Dispatch, Ending, Outcome, Phase, SignRequest, Signed};

/// Install path of the cellbox ELF that carries the ssh client rights.
pub const SSH_PATH: &str = "/bin/cellbox-ssh";
/// argv[0] the shell passes so the child takes the ssh entry.
pub const SSH_ENTRY: &str = "ssh";

const USAGE: &str = "usage: ssh --user USER --auth password|publickey [--identity KEY_ID] \
[-p PORT] [--connect-timeout S] [--timeout S] HOST [--command COMMAND]";
const USAGE_STATUS: i32 = 2;
const REMOTE_FAILURE: i32 = 255;
const TIMED_OUT: i32 = 124;
/// The local terminal session was lost (§4.2).
pub const SESSION_LOST: i32 = 125;
/// The foreground child was cancelled (exec-mode Ctrl-C or an SDK operation cancel).
pub const CANCELLED: i32 = 130;
/// An unknown host key on a terminal that cannot run the input owner's confirmation.
pub const NO_FRONTEND: &str = "host key not trusted; confirmation needs an interactive frontend";

/// The persisted first-trust answer, reported as what the store guarantees (S9-4a).
pub fn trust_added(host: &str, port: u16) -> String {
    format!("Host '{host}' ({port}) added to the trust store (synced, not crash-proof).")
}

/// What a `TrustCommit` (or `TrustChallenge` refusal) code means for the session.
pub fn commit_trust(code: u8) -> vissh::session::Trust {
    use api::ipc::ssh_trust;
    use vissh::session::Trust;
    match code {
        ssh_trust::KNOWN_EXACT => Trust::KnownExact,
        ssh_trust::MISMATCH => Trust::Mismatch,
        ssh_trust::DENIED => Trust::Denied,
        ssh_trust::REFUSED => Trust::Refused,
        // Still undecided when the client stopped waiting: the owner's deadline passed.
        ssh_trust::EXPIRED | ssh_trust::PENDING => Trust::Expired,
        _ => Trust::Unavailable,
    }
}

/// Shell builtin: refuse bad argv before any launch, then run the client Cell in the
/// foreground of this session. Every invocation leaves a completion record.
pub fn cmd_ssh(argv: &[String]) -> i32 {
    let mode = if argv.iter().any(|a| a == "--command") { sc::Mode::Exec } else { sc::Mode::Pty };
    let refuse = |status: i32, code: &str| {
        crate::executor::set_ssh_completion(admission(mode, code));
        status
    };
    if let Err(e) = args::parse_owned(argv) {
        crate::executor::shell_stderrln(&format!("ssh: {e}"));
        crate::executor::shell_stderrln(USAGE);
        return refuse(USAGE_STATUS, "usage");
    }
    if !crate::executor::interactive_terminal() {
        crate::executor::shell_stderrln("ssh: no interactive terminal here");
        return refuse(1, "no_terminal");
    }
    let mut child_argv: Vec<String> = Vec::with_capacity(argv.len() + 1);
    child_argv.push(String::from(SSH_ENTRY));
    child_argv.extend(argv.iter().cloned());
    let (status, record) = crate::executor::spawn_ssh_foreground(SSH_PATH, &child_argv);
    crate::executor::set_ssh_completion(settle(record, status, mode));
    status
}

/// A record for a session that never left admission (refused argv, no terminal, no launch).
pub fn admission(mode: sc::Mode, code: &str) -> sc::CompletionV1 {
    sc::CompletionV1 {
        mode, phase: sc::Phase::Admission, dispatch: sc::Dispatch::NotSent, outcome: sc::Outcome::LocalError,
        remote_status: None, remote_signal: None, error_code: sc::Text::new(code),
        stdout_complete: false, stderr_complete: false,
    }
}

/// The shell's record of a finished ssh child: its newest frame, except that a cancelled
/// child (killed by the kernel, so it wrote no final frame) is `cancelled` at the phase
/// and dispatch it last reported. A child that wrote nothing never left admission.
pub fn settle(record: Option<sc::CompletionV1>, status: i32, mode: sc::Mode) -> sc::CompletionV1 {
    match record {
        Some(r) if status == CANCELLED => sc::CompletionV1 {
            outcome: sc::Outcome::Cancelled, error_code: sc::Text::new("cancelled"), ..r
        },
        Some(r) => r,
        None => admission(mode, if status == CANCELLED { "cancelled" } else { "no_record" }),
    }
}

/// The completion record's phase for a session phase.
pub fn record_phase(phase: Phase) -> sc::Phase {
    match phase {
        Phase::Trust => sc::Phase::Trust,
        Phase::Auth => sc::Phase::Auth,
        Phase::Request => sc::Phase::Request,
        Phase::Running => sc::Phase::Running,
    }
}

fn record_dispatch(dispatch: Dispatch) -> sc::Dispatch {
    match dispatch {
        Dispatch::NotSent => sc::Dispatch::NotSent,
        Dispatch::MayHaveRun => sc::Dispatch::MayHaveRun,
        Dispatch::Accepted => sc::Dispatch::Accepted,
    }
}

/// A progress frame: the session is at `phase` with `dispatch` and has no outcome yet
/// (`unknown`, which is also what a session interrupted right here ends with).
pub fn progress_record(mode: sc::Mode, phase: sc::Phase, dispatch: Dispatch) -> sc::CompletionV1 {
    sc::CompletionV1 {
        mode, phase, dispatch: record_dispatch(dispatch), outcome: sc::Outcome::Unknown,
        remote_status: None, remote_signal: None, error_code: None, stdout_complete: false, stderr_complete: false,
    }
}

/// The final record of a session (§4.3) that last reported `phase`.
pub fn completion(outcome: &Outcome, mode: sc::Mode, phase: sc::Phase, session_lost: bool) -> sc::CompletionV1 {
    use sc::Outcome as O;
    use vissh::session::Trust;
    let sent = outcome.dispatch != Dispatch::NotSent;
    let uncertain = if sent { O::Unknown } else { O::LocalError };
    let (result, code, phase) = match &outcome.ending {
        _ if session_lost => (O::SessionLost, "session_lost", phase),
        Ending::RemoteExit(_) => (O::RemoteExit, "", sc::Phase::Complete),
        Ending::RemoteSignal(_) => (O::RemoteSignal, "", sc::Phase::Complete),
        Ending::TimedOut => (O::Timeout, "timeout", phase),
        Ending::TransportLost => (uncertain, "transport_lost", phase),
        Ending::Unknown => (O::Unknown, "no_exit_status", phase),
        // A publickey refusal is local: nothing was signed for the server.
        Ending::Credential(credential) => (O::LocalError, credential_code(*credential), sc::Phase::Auth),
        Ending::HostKey(trust, _) => (O::LocalError, match trust {
            Trust::Mismatch => "host_key_mismatch",
            Trust::Unknown | Trust::Refused => "host_key_refused",
            Trust::Expired => "host_key_confirmation_expired",
            Trust::NoConfirmation => "host_key_no_frontend",
            Trust::Denied => "trust_denied",
            Trust::KnownExact | Trust::Unavailable => "trust_unavailable",
        }, sc::Phase::Trust),
        Ending::Auth => (O::LocalError, "auth_failed", sc::Phase::Auth),
        Ending::RequestRefused => (O::LocalError, "request_refused", phase),
        Ending::Disconnected => (uncertain, "disconnected", phase),
        Ending::Error(_) => (uncertain, "protocol", phase),
    };
    sc::CompletionV1 {
        mode, phase, dispatch: record_dispatch(outcome.dispatch), outcome: result,
        remote_status: match outcome.ending { Ending::RemoteExit(n) => Some(n), _ => None },
        remote_signal: match &outcome.ending { Ending::RemoteSignal(s) => sc::Text::new(s), _ => None },
        error_code: sc::Text::new(code),
        stdout_complete: outcome.stdout_complete, stderr_complete: outcome.stderr_complete,
    }
}

#[cfg(all(not(test), not(feature = "shell_test")))]
pub use child::run as run_child;

/// The `~.` / `~~` local escape at line start (command-contracts §4.2), over raw PTY
/// input bytes. `feed` returns the bytes to forward and whether `~.` was typed.
#[derive(Debug)]
pub struct Escape {
    line_start: bool,
    tilde: bool,
}

impl Default for Escape {
    fn default() -> Self {
        Escape { line_start: true, tilde: false }
    }
}

impl Escape {
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) -> bool {
        for &b in input {
            if self.tilde {
                self.tilde = false;
                match b {
                    b'.' => return true,
                    b'~' => out.push(b'~'),
                    other => out.extend_from_slice(&[b'~', other]),
                }
            } else if self.line_start && b == b'~' {
                self.tilde = true;
                continue;
            } else {
                out.push(b);
            }
            self.line_start = matches!(b, b'\r' | b'\n');
        }
        false
    }

    /// End of input: a pending `~` is forwarded.
    pub fn flush(&mut self, out: &mut Vec<u8>) {
        if core::mem::take(&mut self.tilde) {
            out.push(b'~');
        }
    }
}

/// Local status and message for a finished session (§4.3).
pub fn report(outcome: &Outcome, host: &Host, port: u16) -> (i32, Option<String>) {
    use vissh::session::Trust;
    let message = match &outcome.ending {
        Ending::RemoteExit(n) if *n <= 255 => return (*n as i32, None),
        Ending::RemoteExit(n) => format!("remote exit status {n}"),
        Ending::RemoteSignal(sig) => format!("remote command killed by signal {sig}"),
        Ending::HostKey(Trust::Mismatch, key) => format!(
            "HOST KEY CHANGED for {host}:{port}: server offered {} {}; refused",
            key.algorithm,
            key.fingerprint()
        ),
        Ending::HostKey(Trust::Unknown, key) => format!(
            "host key for {host}:{port} is not trusted: {} {}",
            key.algorithm,
            key.fingerprint()
        ),
        Ending::HostKey(Trust::Refused, key) => format!(
            "host key for {host}:{port} was not trusted: {} {} refused",
            key.algorithm,
            key.fingerprint()
        ),
        Ending::HostKey(Trust::Expired, _) => format!("host key confirmation for {host}:{port} timed out; not trusted"),
        Ending::HostKey(Trust::NoConfirmation, _) => String::from(NO_FRONTEND),
        Ending::HostKey(Trust::Denied, _) => String::from("host trust lookup denied"),
        Ending::HostKey(_, _) => String::from("host trust store unavailable"),
        Ending::Credential(credential) => credential_message(*credential, None),
        Ending::Auth => String::from("authentication failed"),
        Ending::RequestRefused => String::from("server refused the session request"),
        Ending::Disconnected => String::from("disconnected"),
        Ending::Unknown => String::from("connection ended without a remote exit status; the command may have run"),
        Ending::TimedOut => return (TIMED_OUT, Some(String::from("timed out"))),
        Ending::TransportLost if outcome.dispatch == Dispatch::NotSent => String::from("connection lost before the command was sent"),
        Ending::TransportLost => String::from("connection lost; the command may have run"),
        Ending::Error(e) => e.clone(),
    };
    (REMOTE_FAILURE, Some(message))
}

/// Longest passphrase (or password) this client takes: the wire bound of the KMS unlock
/// (`KMS_SSH_SECRET_MAX`) and of the password prompt.
pub const MAX_SECRET: usize = 1024;

/// Whether `text` may be appended to a secret that already holds `held` bytes: within the
/// bound, and no NUL (§4.1: typed text only).
pub fn secret_accepts(held: usize, text: &str) -> bool {
    held + text.len() <= MAX_SECRET && !text.contains('\0')
}

/// Completion `error_code` of a publickey refusal (free ASCII text of at most 32 bytes,
/// `ssh_completion.rs`). None of these, nor their messages, carries key, passphrase,
/// session id or signature bytes.
pub fn credential_code(credential: Credential) -> &'static str {
    match credential {
        Credential::Unavailable => "kms_unavailable",
        Credential::Denied => "kms_denied",
        Credential::UnknownIdentity => "identity_unknown",
        Credential::UnlockCancelled => "unlock_cancelled",
        Credential::UnlockNeedsTerminal => "unlock_needs_terminal",
        Credential::UnlockFailed => "unlock_failed",
        Credential::UnlockLocked => "unlock_locked",
        Credential::Timeout => "kms_timeout",
        Credential::Protocol => "kms_protocol",
        Credential::InputMismatch => "kms_input_mismatch",
        Credential::BadSignature => "kms_bad_signature",
    }
}

/// The local message of a publickey refusal. `key_id` is the name the user typed.
pub fn credential_message(credential: Credential, key_id: Option<&str>) -> String {
    match credential {
        Credential::Unavailable => String::from("key service is not available"),
        Credential::Denied => String::from("not permitted to use the key service"),
        Credential::UnknownIdentity => match key_id {
            Some(key_id) => format!("no such identity '{key_id}'"),
            None => String::from("no such identity"),
        },
        Credential::UnlockCancelled => String::from("unlock cancelled"),
        Credential::UnlockNeedsTerminal => String::from("encrypted key needs an interactive terminal"),
        Credential::UnlockFailed => String::from("unlock failed"),
        Credential::UnlockLocked => String::from("too many failed unlocks; key locked"),
        Credential::Timeout => String::from("key service did not answer"),
        Credential::Protocol => String::from("key service protocol error"),
        Credential::InputMismatch => String::from("key service signed a different input"),
        Credential::BadSignature => String::from("key service returned an invalid signature"),
    }
}

/// What a failed key-service call means for the login. A wrong passphrase, a corrupt blob
/// and a key mismatch are one code (`SshUnlockFailed`): there is no oracle to tell apart.
pub fn credential_of(error: &KmsClientError) -> Credential {
    match error {
        KmsClientError::ServiceNotFound | KmsClientError::Service(KmsErrorCode::SshKeyUnavailable) => {
            Credential::Unavailable
        }
        KmsClientError::Service(KmsErrorCode::CallerUnattested | KmsErrorCode::PermissionDenied) => Credential::Denied,
        KmsClientError::Service(KmsErrorCode::SshKeyNotFound) => Credential::UnknownIdentity,
        KmsClientError::Service(KmsErrorCode::SshUnlockFailed) => Credential::UnlockFailed,
        KmsClientError::Service(KmsErrorCode::SshUnlockLocked) => Credential::UnlockLocked,
        KmsClientError::Service(KmsErrorCode::InvalidSignature) => Credential::BadSignature,
        KmsClientError::Timeout => Credential::Timeout,
        // `SshUnlockRequired`, `InvalidRequest`, `SshOrderInvalid`, `SshHandleInvalid`, any
        // other code, and every wire or IPC error.
        _ => Credential::Protocol,
    }
}

/// The two key-service calls a login makes after the preflight, so that the unlock and
/// sign sequence runs under host tests with a scripted service.
pub trait KeyService {
    /// Stage the passphrase for the next signature (Begin and the ordered chunks).
    fn stage_secret(&self, public_key: &[u8; 32], secret: &[u8]) -> Result<(), KmsClientError>;
    /// Sign the userauth challenge.
    fn sign(&self, public_key: &[u8; 32], session_id: &[u8; 32], username: &str) -> Result<SshSignResponsePayload, KmsClientError>;
}

impl KeyService for KmsClient {
    fn stage_secret(&self, public_key: &[u8; 32], secret: &[u8]) -> Result<(), KmsClientError> {
        self.ssh_stage_secret(public_key, secret)
    }
    fn sign(&self, public_key: &[u8; 32], session_id: &[u8; 32], username: &str) -> Result<SshSignResponsePayload, KmsClientError> {
        self.ssh_sign_userauth(public_key, session_id, username.as_bytes())
    }
}

/// One signature: for a passphrase-wrapped identity first ask for the passphrase (once,
/// no re-prompt), stage it and wipe it; then sign. A plain identity skips the unlock. An
/// empty passphrase is not representable on the wire (`secret_len` is 1..=1,024), so it is
/// `unlock_failed` locally, without a key-service call. A staged secret is consumed by the
/// sign (success or failure), so nothing is aborted afterwards.
pub fn kms_sign<K: KeyService>(
    kms: &K,
    public_key: &[u8; 32],
    custody: KmsSshKeyCustody,
    request: &SignRequest<'_>,
    passphrase: impl FnOnce() -> Result<String, Credential>,
) -> Result<Signed, Credential> {
    if custody == KmsSshKeyCustody::PassphraseWrapped {
        let mut secret = passphrase()?;
        let staged = if secret.is_empty() {
            Err(Credential::UnlockFailed)
        } else {
            kms.stage_secret(public_key, secret.as_bytes()).map_err(|error| credential_of(&error))
        };
        ostd::session::zeroize_string(&mut secret);
        staged?;
    }
    let signed = kms
        .sign(public_key, &request.session_id, request.username)
        .map_err(|error| credential_of(&error))?;
    Ok(Signed { signature: signed.signature, message_sha256: signed.message_sha256 })
}

/// The argv of an ssh pipeline stage launched from [`SSH_PATH`]: the child's entry keys on
/// `ssh` as argv[0] ([`SSH_ENTRY`]) and parses what follows, so the stage gets the whole
/// command with its program name first.
pub fn stage_argv(words: &[String]) -> Vec<&str> {
    core::iter::once(SSH_ENTRY).chain(words.iter().skip(1).map(String::as_str)).collect()
}

/// Lane-only key-service fault seam (`kssh-faults`; module text for the prefix rules). The
/// `child` module has its own inline `faults` for `p09-faults`, which it shadows there;
/// the child reaches this one as `super::faults`.
#[cfg(feature = "kssh-faults")]
mod faults;

#[cfg(test)]
mod tests;

#[cfg(all(not(test), not(feature = "shell_test")))]
mod child {
    use super::*;
    use core::cell::Cell;
    use ostd::child_context::{ChildContext, GrantSlot};
    use ostd::clients::net::{NetClient, SocketId};
    use ostd::clients::vfs::VfsClient;
    use ostd::ViError;
    use terminal_sdk::input::EventKind;
    use terminal_sdk::raw_input::RawInput;
    use terminal_sdk::session::TerminalSession;
    use terminal_sdk::stream::{Endpoint, EndpointRef, OperationRef};
    use vissh::client::{HostKey, Transport, TransportError};
    use ostd::clients::kms::{validate_ssh_key_name, validate_ssh_username};
    use vissh::session::{self, AuthKind, Dispatch, Input, Local, Mode, Phase, RekeyPolicy, Trust};
    #[cfg(feature = "p09-faults")]
    use vissh::session::Point;
    use zeroize::Zeroize;

    const FOREVER: u64 = u64::MAX;
    /// Phase budgets (command-contracts §4.2): auth/unlock including prompts, then the
    /// channel open and request until the server's reply.
    const AUTH_MS: u64 = 120_000;
    const REQUEST_MS: u64 = 30_000;
    /// Longest wait for one prompt event; the prompt re-checks its deadline between them.
    const PROMPT_SLICE_MS: u64 = 1_000;
    const ENTER: u32 = 0x110001;
    const ESCAPE: u32 = 0x110002;
    const BACKSPACE: u32 = 0x110003;
    const CTRL: u32 = 2;

    fn granted(context: &ChildContext, slot: GrantSlot) -> Option<Endpoint> {
        let operation = OperationRef::from_raw(context.operation()).ok()?;
        let grant = context.grants().find(|g| g.slot == slot && g.kind == 1)?;
        Some(Endpoint::new(EndpointRef::from_raw(grant.object).ok()?, operation))
    }

    fn write_all(out: Option<Endpoint>, mut data: &[u8]) {
        let Some(out) = out else { return };
        while !data.is_empty() {
            match out.write(data, FOREVER) {
                Ok(n) if n > 0 && n <= data.len() => data = &data[n..],
                _ => return,
            }
        }
    }

    /// Kernel entropy only: every 64-byte chunk must be filled completely.
    fn kernel_entropy(buf: &mut [u8]) -> bool {
        buf.chunks_mut(64).all(|c| ostd::syscall::sys_get_random(c) == c.len())
    }

    fn now_ms() -> u64 {
        ostd::syscall::sys_get_time_ms().unwrap_or(0)
    }

    /// The phase deadline (connect, auth, request, then none) bounded by `--timeout`,
    /// which is absolute from invocation and never pauses (§4.2). Only the connect phase
    /// excludes the human trust confirmation, which ends it.
    struct Deadline {
        phase: Cell<u64>,
        overall: u64,
    }

    impl Deadline {
        fn at(&self) -> u64 {
            self.phase.get().min(self.overall)
        }
        fn passed(&self) -> bool {
            now_ms() >= self.at()
        }
    }

    /// The production rekey policy, or (lane-only `p09-faults` build) the lowered lane
    /// test configuration, which is announced on the kernel log.
    #[cfg(not(feature = "p09-faults"))]
    fn rekey_policy() -> RekeyPolicy {
        RekeyPolicy::PRODUCTION
    }
    #[cfg(feature = "p09-faults")]
    fn rekey_policy() -> RekeyPolicy {
        let _ = ostd::syscall::sys_log("P04 INFO ssh-test-config rekey=lane\n");
        faults::LANE_REKEY
    }

    /// How long the client waits for the owner-run confirmation: the challenge's
    /// 60 seconds plus a margin for the owner and VFS to report its end.
    const CONFIRM_MS: u64 = 65_000;
    /// Pause between `TrustCommit` polls (scheduler ticks).
    const CONFIRM_POLL_TICKS: usize = 10;

    struct NetTransport<'d> {
        net: NetClient,
        socket: SocketId,
        deadline: &'d Deadline,
    }

    impl Transport for NetTransport<'_> {
        fn send(&mut self, data: &[u8]) -> Result<usize, TransportError> {
            let chunk = &data[..data.len().min(api::ipc::NET_TCP_INLINE_DATA_MAX)];
            self.net.tcp_send_some(self.socket, chunk).map_err(|_| TransportError::Io)
        }

        fn recv(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
            match self.net.tcp_recv(self.socket, buf.len() as u32) {
                Ok(data) => {
                    buf[..data.len()].copy_from_slice(&data);
                    Ok(data.len())
                }
                Err(ViError::PermissionDenied) => Err(TransportError::Io),
                // The service refuses a read while the socket cannot receive yet (the
                // TCP handshake). The state is read after that refusal, so a socket that
                // finished its handshake in between reads Established: also "nothing yet".
                // Only a socket past its receive half is closed.
                Err(_) => match self.net.tcp_state(self.socket) {
                    Ok(0x01..=0x03) => Ok(0),
                    _ => Err(TransportError::Closed),
                },
            }
        }

        fn idle(&mut self) -> Result<(), TransportError> {
            if self.deadline.passed() {
                return Err(TransportError::TimedOut);
            }
            ostd::syscall::sys_yield();
            Ok(())
        }

        fn now_ms(&mut self) -> u64 {
            now_ms()
        }
    }

    /// How one secret prompt ended.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Secret {
        Given,
        /// Escape, end of input, an interrupt, a refused character, or a lost terminal.
        Cancelled,
        /// The phase budget or `--timeout` passed while the prompt was open.
        TimedOut,
    }

    /// What the key-service preflight learned about the named identity, and the connection
    /// that learned it (the same client signs later, so its payload ids keep increasing).
    struct Identity {
        client: KmsClient,
        key_id: String,
        public_key: [u8; 32],
        custody: KmsSshKeyCustody,
        /// Lane-only (`kssh-faults`): the one mutation of this invocation's sign call.
        #[cfg(feature = "kssh-faults")]
        fault: Option<super::faults::Mode>,
    }

    impl Identity {
        /// One signature; `passphrase(key_id)` asks for the passphrase of a wrapped identity.
        fn sign(
            &self,
            request: &SignRequest<'_>,
            passphrase: impl FnOnce(&str) -> Result<String, Credential>,
        ) -> Result<Signed, Credential> {
            #[cfg(feature = "kssh-faults")]
            let kms = &super::faults::Faulted::new(&self.client, self.fault);
            #[cfg(not(feature = "kssh-faults"))]
            let kms = &self.client;
            kms_sign(kms, &self.public_key, self.custody, request, || passphrase(&self.key_id))
        }
    }

    /// Ask the key service about the identity before any network traffic (a status query is
    /// not an authentication attempt, so trust-before-signature is unaffected). A wrapped
    /// identity in a pipeline stage is refused here: its passphrase needs a terminal.
    fn preflight(key_id: &str, staged: bool) -> Result<Identity, Credential> {
        if !validate_ssh_key_name(key_id.as_bytes()) {
            return Err(Credential::UnknownIdentity);
        }
        let client = KmsClient::connect().map_err(|error| credential_of(&error))?;
        let status = client.ssh_key_status(key_id.as_bytes()).map_err(|error| credential_of(&error))?;
        if staged && status.custody == KmsSshKeyCustody::PassphraseWrapped {
            return Err(Credential::UnlockNeedsTerminal);
        }
        Ok(Identity {
            client,
            key_id: String::from(key_id),
            public_key: status.public_key,
            custody: status.custody,
            #[cfg(feature = "kssh-faults")]
            fault: None,
        })
    }

    /// The VFS trust store's answer for `key` of `host:port`.
    fn lookup_trust(host: &str, port: u16, key: &HostKey) -> Trust {
        match VfsClient::new().trust_lookup(host, port, key.algorithm, &key.blob()) {
            Ok(api::ipc::ssh_trust::KNOWN_EXACT) => Trust::KnownExact,
            Ok(api::ipc::ssh_trust::UNKNOWN) => Trust::Unknown,
            Ok(api::ipc::ssh_trust::MISMATCH) => Trust::Mismatch,
            Ok(api::ipc::ssh_trust::DENIED) => Trust::Denied,
            _ => Trust::Unavailable,
        }
    }

    /// Phase budgets (§4.2): trust runs on the connect deadline, then auth, request (channel
    /// open to the server's reply) and running (none but `--timeout`); every change is
    /// recorded write-ahead on the shell's completion stream.
    fn advance(deadline: &Deadline, completion: Option<Endpoint>, mode: sc::Mode, current: &mut sc::Phase, phase: Phase, dispatch: Dispatch) {
        let next = record_phase(phase);
        if next != *current {
            let budget = match phase {
                Phase::Trust => None,
                Phase::Auth => Some(now_ms().saturating_add(AUTH_MS)),
                Phase::Request => Some(now_ms().saturating_add(REQUEST_MS)),
                Phase::Running => Some(u64::MAX),
            };
            if let Some(at) = budget {
                deadline.phase.set(at);
            }
            *current = next;
        }
        write_all(completion, &progress_record(mode, *current, dispatch).encode());
    }

    /// Blocking write of all of `data`; false when the stream failed or closed.
    fn push(out: Option<Endpoint>, mut data: &[u8]) -> bool {
        let Some(out) = out else { return true };
        while !data.is_empty() {
            match out.write(data, FOREVER) {
                Ok(n) if n > 0 && n <= data.len() => data = &data[n..],
                _ => return false,
            }
        }
        true
    }

    /// The local terminal side: trust through the VFS, the password under the input
    /// owner's echo-off lease, then raw (PTY) or canonical (exec) input.
    struct Term<'s, 'd> {
        session: &'s mut TerminalSession,
        /// The raw-input lease while the PTY bridge runs.
        raw: Option<RawInput>,
        host: String,
        port: u16,
        user: String,
        pty: bool,
        escape: Escape,
        size: (u16, u16),
        pending_resize: Option<(u16, u16)>,
        deadline: &'d Deadline,
        stdin_ended: bool,
        /// The shell's write-only completion stream (slot 5) and what it last recorded.
        completion: Option<Endpoint>,
        mode: sc::Mode,
        phase: sc::Phase,
        /// The local terminal session itself is gone (§4.2: status 125, `session_lost`).
        session_lost: bool,
        /// Publickey: what the preflight learned (and the key-service connection).
        identity: Option<Identity>,
        /// A secret prompt ran into the phase budget or `--timeout`.
        prompt_timeout: bool,
        #[cfg(feature = "p09-faults")]
        faults: faults::Faults,
    }

    impl Term<'_, '_> {
        fn session(&mut self) -> &mut TerminalSession {
            self.session
        }

        /// A session operation failed: these answers mean the session (or this child's
        /// foreground in it) is gone for good, not a transient condition.
        fn note(&mut self, status: api::cell_session::Status) {
            use api::cell_session::Status as S;
            if matches!(status, S::Stale | S::Revoked | S::BrokenPipe | S::WrongSession | S::WrongEpoch) {
                self.session_lost = true;
            }
        }

        /// Prompt on stderr, then read one secret line under the input owner's separate
        /// echo-off lease (typed text only, Escape cancels, bounded by [`MAX_SECRET`] and
        /// by the remaining phase budget). `secret` is wiped unless it was given.
        fn ask_secret(&mut self, prompt: &str, secret: &mut String) -> Secret {
            self.write(true, prompt.as_bytes());
            let got = match self.session.set_mode(2, 0) {
                Ok(mut guard) => {
                    let got = read_secret(guard.session(), secret, self.deadline);
                    let released = guard.release().is_ok();
                    Ok(if released { got } else { Secret::Cancelled })
                }
                Err(status) => Err(status),
            };
            let got = match got {
                Ok(got) => got,
                Err(status) => {
                    self.note(status);
                    return Secret::Cancelled;
                }
            };
            self.write(true, b"\n");
            if got != Secret::Given {
                secret.zeroize();
            }
            if got == Secret::TimedOut {
                self.prompt_timeout = true;
            }
            got
        }

        /// The passphrase of the wrapped identity `key_id`, asked once (no re-prompt).
        fn passphrase(&mut self, key_id: &str) -> Result<String, Credential> {
            let mut secret = String::new();
            match self.ask_secret(&format!("Enter passphrase for key '{key_id}': "), &mut secret) {
                Secret::Given => Ok(secret),
                Secret::Cancelled | Secret::TimedOut => Err(Credential::UnlockCancelled),
            }
        }

        fn write(&mut self, stderr: bool, mut data: &[u8]) {
            while !data.is_empty() {
                let r = if stderr { self.session().write_stderr(data, FOREVER) } else { self.session().write_stdout(data, FOREVER) };
                match r {
                    Ok(n) if n > 0 && n <= data.len() => data = &data[n..],
                    Err(status) => return self.note(status),
                    Ok(_) => return,
                }
            }
        }

        /// Raw bytes on stdin (the SDK raw lease: raw routing plus bracketed paste); the
        /// remote PTY does the echo. The owner refuses the lease while typed input is
        /// still unread (`WouldBlock`): that input predates the remote shell and is dropped.
        fn enter_raw(&mut self) {
            if self.raw.is_some() || self.stdin_ended {
                return;
            }
            match RawInput::enter(self.session) {
                Ok(raw) => self.raw = Some(raw),
                Err(api::cell_session::Status::WouldBlock) => {
                    if let Ok(mut stale) = self.session.next_event(0) {
                        stale.scrub();
                    }
                }
                Err(status) => {
                    self.note(status);
                    self.stdin_ended = true;
                }
            }
        }

        fn poll_resize(&mut self) {
            while let Ok(event) = self.session().next_control(0) {
                match event.kind {
                    EventKind::Resize { rows, cols } => {
                        self.size = (rows, cols);
                        self.pending_resize = Some((rows, cols));
                    }
                    EventKind::Eof => self.stdin_ended = true,
                    _ => {}
                }
            }
        }

        fn pty_input(&mut self, buf: &mut [u8]) -> Input {
            self.enter_raw();
            if self.raw.is_none() {
                return if self.stdin_ended { Input::Eof } else { Input::Idle };
            }
            self.poll_resize();
            if self.stdin_ended {
                return Input::Eof;
            }
            // One escape byte may expand to two forwarded bytes.
            let mut raw = [0u8; 512];
            let take = raw.len().min(buf.len().saturating_sub(1));
            if take == 0 {
                return Input::Idle;
            }
            match self.session().read_input(&mut raw[..take], 0) {
                Ok(n) if n > 0 => {
                    let mut out = Vec::with_capacity(n + 1);
                    let disconnect = self.escape.feed(&raw[..n], &mut out);
                    raw.zeroize();
                    let len = out.len();
                    buf[..len].copy_from_slice(&out);
                    out.zeroize();
                    if disconnect {
                        Input::Disconnect
                    } else if len == 0 {
                        Input::Idle
                    } else {
                        Input::Data(len)
                    }
                }
                Err(api::cell_session::Status::Eof) => {
                    self.stdin_ended = true;
                    let mut out = Vec::new();
                    self.escape.flush(&mut out);
                    if out.is_empty() {
                        Input::Eof
                    } else {
                        buf[..out.len()].copy_from_slice(&out);
                        Input::Data(out.len())
                    }
                }
                Err(status) => {
                    self.note(status);
                    Input::Idle
                }
                Ok(_) => Input::Idle,
            }
        }

        /// Exec from the terminal: canonical typed text, Enter as LF, Ctrl-D as EOF.
        fn exec_input(&mut self, buf: &mut [u8]) -> Input {
            if self.stdin_ended {
                return Input::Eof;
            }
            match self.session().next_event(0) {
                Ok(event) => match &event.kind {
                    EventKind::Text(text) if text.len() <= buf.len() => {
                        buf[..text.len()].copy_from_slice(text.as_bytes());
                        Input::Data(text.len())
                    }
                    EventKind::Key { code: ENTER, modifiers: 0, pressed: 1 | 2 } if !buf.is_empty() => {
                        buf[0] = b'\n';
                        Input::Data(1)
                    }
                    // Ctrl-D arrives as a key, not as end of input: it ends the stdin.
                    EventKind::Key { code: 0x64 | 0x44, modifiers: CTRL, pressed: 1 | 2 } | EventKind::Eof => {
                        self.stdin_ended = true;
                        Input::Eof
                    }
                    _ => Input::Idle,
                },
                Err(status) => {
                    self.note(status);
                    Input::Idle
                }
            }
        }

        /// Challenge (VFS), begin (input owner, on this session at its current epoch), then
        /// poll VFS for the stored outcome while the owner holds the terminal. Returns the
        /// answer and whether this confirmation stored the key.
        fn confirm(&mut self, key: &HostKey) -> (Trust, bool) {
            let (session, epoch) = (self.session.reference().raw(), self.session.epoch());
            let mut vfs = VfsClient::new();
            let challenge = match vfs.trust_challenge(&self.host, self.port, key.algorithm, &key.blob(), session, epoch) {
                Ok(Ok((confirmation, challenge))) => match self.session.begin_trust_confirmation(confirmation) {
                    Ok(()) => challenge,
                    // A Serial or headless terminal has no trusted way to answer.
                    Err(api::cell_session::Status::Unsupported) => return (Trust::NoConfirmation, false),
                    Err(_) => return (Trust::Unavailable, false),
                },
                // Stored meanwhile by another client (or changed): no prompt.
                Ok(Err(code)) => return (commit_trust(code), false),
                Err(_) => return (Trust::Unavailable, false),
            };
            let give_up = now_ms().saturating_add(CONFIRM_MS);
            loop {
                match vfs.trust_commit(challenge) {
                    Ok(api::ipc::ssh_trust::PENDING) if now_ms() < give_up => {
                        let _ = ostd::syscall::sys_set_timer(CONFIRM_POLL_TICKS);
                    }
                    Ok(code) => return (commit_trust(code), code == api::ipc::ssh_trust::KNOWN_EXACT),
                    Err(_) => return (Trust::Unavailable, false),
                }
            }
        }
    }

    impl Local for Term<'_, '_> {
        fn trust(&mut self, key: &HostKey) -> Trust {
            lookup_trust(&self.host, self.port, key)
        }

        /// First trust (S9-4c): VFS registers a challenge for this exact key and session;
        /// the input owner prompts and captures the answer itself (this client reads no
        /// input meanwhile) and VFS stores an accepted key. Only a stored key continues.
        fn confirm_unknown(&mut self, key: &HostKey) -> Trust {
            let started = now_ms();
            let (trust, stored) = self.confirm(key);
            // The connect phase excludes the human decision; `--timeout` keeps running.
            let decided = now_ms().saturating_sub(started);
            self.deadline.phase.set(self.deadline.phase.get().saturating_add(decided));
            if trust == Trust::KnownExact && stored {
                let added = trust_added(&self.host, self.port);
                self.write(true, format!("{added}\n").as_bytes());
            }
            trust
        }

        fn password(&mut self, secret: &mut String) -> bool {
            let prompt = format!("{}@{}'s password: ", self.user, self.host);
            self.ask_secret(&prompt, secret) == Secret::Given
        }

        fn identity(&mut self) -> Result<[u8; 32], Credential> {
            self.identity.as_ref().map(|identity| identity.public_key).ok_or(Credential::Unavailable)
        }

        fn sign(&mut self, request: &SignRequest<'_>) -> Result<Signed, Credential> {
            let identity = self.identity.take().ok_or(Credential::Unavailable)?;
            identity.sign(request, |key_id| self.passphrase(key_id))
        }

        fn input(&mut self, buf: &mut [u8]) -> Input {
            if self.session_lost {
                return Input::Disconnect;
            }
            if buf.is_empty() {
                return Input::Idle;
            }
            if self.pty { self.pty_input(buf) } else { self.exec_input(buf) }
        }

        #[cfg(not(feature = "p09-faults"))]
        fn stdout(&mut self, data: &[u8]) {
            self.write(false, data);
        }

        #[cfg(feature = "p09-faults")]
        fn stdout(&mut self, data: &[u8]) {
            if !self.faults.sink(data) {
                self.write(false, data);
            }
        }

        #[cfg(feature = "p09-faults")]
        fn checkpoint(&mut self, point: Point) -> u64 {
            self.faults.checkpoint(point)
        }

        /// Phase budgets (§4.2): trust runs on the connect deadline, then auth, request
        /// (channel open to the server's reply) and running (none but `--timeout`).
        fn progress(&mut self, phase: Phase, dispatch: Dispatch) {
            advance(self.deadline, self.completion, self.mode, &mut self.phase, phase, dispatch);
        }

        fn stderr(&mut self, data: &[u8]) {
            self.write(true, data);
        }

        fn resized(&mut self) -> Option<(u16, u16)> {
            if self.pty && self.raw.is_some() {
                self.poll_resize();
            }
            self.pending_resize.take()
        }
    }

    /// The local side of a pipeline stage (publickey exec): host trust through the VFS, then
    /// the stage's own stdin, stdout and stderr streams, byte for byte. There is no terminal:
    /// nothing is prompted, an unknown host key is refused (`host_key_no_frontend`) and a
    /// wrapped identity was refused by the preflight (`unlock_needs_terminal`).
    struct PipeTerm<'d> {
        stdin: Option<Endpoint>,
        stdout: Option<Endpoint>,
        stderr: Option<Endpoint>,
        host: String,
        port: u16,
        deadline: &'d Deadline,
        identity: Option<Identity>,
        completion: Option<Endpoint>,
        mode: sc::Mode,
        phase: sc::Phase,
        stdin_ended: bool,
        /// The stage's output (or input) stream failed: end the session.
        broken: bool,
    }

    impl Local for PipeTerm<'_> {
        fn trust(&mut self, key: &HostKey) -> Trust {
            lookup_trust(&self.host, self.port, key)
        }

        /// First-trust confirmation needs the input owner's terminal prompt.
        fn confirm_unknown(&mut self, _key: &HostKey) -> Trust {
            Trust::NoConfirmation
        }

        /// Password mode is never a pipeline stage; this cannot be reached.
        fn password(&mut self, _secret: &mut String) -> bool {
            false
        }

        fn identity(&mut self) -> Result<[u8; 32], Credential> {
            self.identity.as_ref().map(|identity| identity.public_key).ok_or(Credential::Unavailable)
        }

        fn sign(&mut self, request: &SignRequest<'_>) -> Result<Signed, Credential> {
            let identity = self.identity.take().ok_or(Credential::Unavailable)?;
            identity.sign(request, |_| Err(Credential::UnlockNeedsTerminal))
        }

        /// The upstream stage's bytes, unchanged; one EOF when the stream ends.
        fn input(&mut self, buf: &mut [u8]) -> Input {
            if self.broken {
                return Input::Disconnect;
            }
            if self.stdin_ended {
                return Input::Eof;
            }
            let Some(stdin) = self.stdin else {
                self.stdin_ended = true;
                return Input::Eof;
            };
            if buf.is_empty() {
                return Input::Idle;
            }
            match stdin.read(buf, 0) {
                Ok(n) if n > 0 && n <= buf.len() => Input::Data(n),
                Ok(_) | Err(api::cell_session::Status::WouldBlock | api::cell_session::Status::TimedOut) => Input::Idle,
                Err(api::cell_session::Status::Eof) => {
                    self.stdin_ended = true;
                    Input::Eof
                }
                Err(_) => {
                    self.broken = true;
                    Input::Disconnect
                }
            }
        }

        fn stdout(&mut self, data: &[u8]) {
            if !push(self.stdout, data) {
                self.broken = true;
            }
        }

        /// Remote stderr is a diagnostic stream here, never data.
        fn stderr(&mut self, data: &[u8]) {
            let _ = push(self.stderr, data);
        }

        fn resized(&mut self) -> Option<(u16, u16)> {
            None
        }

        fn progress(&mut self, phase: Phase, dispatch: Dispatch) {
            advance(self.deadline, self.completion, self.mode, &mut self.phase, phase, dispatch);
        }
    }

    /// One line under the echo-off lease: text, Backspace, Enter; Escape cancels. Pasted
    /// text is refused (§4.1: no newline/NUL, typed only). The wait is bounded by the
    /// remaining phase budget and `--timeout` (K6 O-K6-6), also for the password.
    fn read_secret(session: &mut TerminalSession, secret: &mut String, deadline: &Deadline) -> Secret {
        let mut paste = false;
        loop {
            let remaining_ms = deadline.at().saturating_sub(now_ms());
            if remaining_ms == 0 {
                return Secret::TimedOut;
            }
            // The monotonic clock bounds each wait; without it the wait is a poll.
            let until = ostd::session::monotonic_ns()
                .map_or(0, |now| now.saturating_add(remaining_ms.min(PROMPT_SLICE_MS).saturating_mul(1_000_000)));
            let mut event = match session.next_event(until) {
                Ok(event) => event,
                Err(api::cell_session::Status::TimedOut | api::cell_session::Status::WouldBlock) => {
                    if until == 0 {
                        let _ = ostd::syscall::sys_set_timer(1);
                    }
                    continue;
                }
                Err(_) => return Secret::Cancelled,
            };
            let done = match &event.kind {
                EventKind::PasteBegin => {
                    paste = true;
                    None
                }
                EventKind::PasteEnd => {
                    paste = false;
                    None
                }
                EventKind::Text(text) if !paste => {
                    if secret_accepts(secret.len(), text) {
                        secret.push_str(text);
                        None
                    } else {
                        Some(Secret::Cancelled)
                    }
                }
                EventKind::Key { code, modifiers, pressed: 1 | 2 } if !paste => match *code {
                    _ if crate::executor::submits_line(*code, *modifiers) => Some(Secret::Given),
                    ESCAPE if *modifiers == 0 => Some(Secret::Cancelled),
                    BACKSPACE if *modifiers == 0 => {
                        secret.pop();
                        None
                    }
                    _ => None,
                },
                EventKind::Interrupt | EventKind::Eof => Some(Secret::Cancelled),
                _ => None,
            };
            event.scrub();
            if let Some(done) = done {
                return done;
            }
        }
    }

    /// Entry of `/bin/cellbox-ssh ssh ...`: a foreground child with the terminal session, or
    /// (publickey exec only) a pipeline stage on the stage's own stdin, stdout and stderr.
    pub fn run(context: &ChildContext) -> i32 {
        let stderr = granted(context, GrantSlot::Stderr);
        let completion = terminal_sdk::completion_writer(context);
        let rmode = if context.argv().any(|a| a == "--command") { sc::Mode::Exec } else { sc::Mode::Pty };
        // A final record and the local message for an ending outside the SSH session.
        let end = |status: i32, phase: sc::Phase, outcome: sc::Outcome, code: &str, message: &str| {
            let record = sc::CompletionV1 {
                mode: rmode, phase, dispatch: sc::Dispatch::NotSent, outcome,
                remote_status: None, remote_signal: None, error_code: sc::Text::new(code),
                stdout_complete: false, stderr_complete: false,
            };
            write_all(completion, &record.encode());
            write_all(stderr, format!("ssh: {message}\n").as_bytes());
            status
        };
        let refuse = |status: i32, code: &str, message: &str| {
            end(status, sc::Phase::Admission, sc::Outcome::LocalError, code, message)
        };
        let parsed = match args::parse(context.argv().skip(1)) {
            Ok(parsed) => parsed,
            Err(e) => return refuse(USAGE_STATUS, "usage", &format!("{e}")),
        };
        let key_id = match &parsed.auth {
            args::Auth::PublicKey { key_id } => Some(key_id.clone()),
            args::Auth::Password => None,
        };
        // A foreground child has a control grant (slot 3); a pipeline stage never does. Only a
        // publickey exec may run as a stage: password and PTY sessions need the terminal.
        let staged = !context.grants().any(|g| g.slot == GrantSlot::Control);
        if staged && !(key_id.is_some() && parsed.command.is_some()) {
            return refuse(REMOTE_FAILURE, "no_terminal", "an interactive terminal session is required");
        }
        let mut session = if staged {
            None
        } else {
            match TerminalSession::from_own_context(context) {
                Ok(session) => Some(session),
                Err(_) => return refuse(REMOTE_FAILURE, "no_terminal", "an interactive terminal session is required"),
            }
        };
        // Publickey: the key service is asked about the identity before any network traffic,
        // so an unknown KEY_ID never touches the network or the server's login-grace slot.
        let identity = match &key_id {
            Some(key_id) => {
                if !validate_ssh_username(parsed.user.as_bytes()) {
                    return refuse(
                        USAGE_STATUS,
                        "usage",
                        "publickey authentication needs a user name of 1 to 31 characters from [A-Za-z0-9._-]",
                    );
                }
                match preflight(key_id, staged) {
                    Ok(identity) => Some(identity),
                    Err(credential) => {
                        return refuse(REMOTE_FAILURE, credential_code(credential), &credential_message(credential, Some(key_id)));
                    }
                }
            }
            None => None,
        };
        // Lane-only (`kssh-faults`): the `: kssh-fault MODE;` command prefix selects the one
        // mutation of this invocation's sign call.
        #[cfg(feature = "kssh-faults")]
        let identity =
            identity.map(|identity| Identity { fault: super::faults::Mode::of_command(parsed.command.as_deref()), ..identity });
        let auth = if key_id.is_some() { AuthKind::PublicKey } else { AuthKind::Password };
        if !vissh::install_entropy(kernel_entropy) {
            return refuse(REMOTE_FAILURE, "entropy", "entropy source already installed");
        }
        let start = now_ms();
        let deadline = Deadline {
            phase: Cell::new(start + u64::from(parsed.connect_timeout) * 1000),
            overall: parsed.timeout.map_or(u64::MAX, |t| start + u64::from(t) * 1000),
        };
        write_all(completion, &progress_record(rmode, sc::Phase::Connect, Dispatch::NotSent).encode());
        let policy = rekey_policy();
        #[cfg(feature = "kssh-faults")]
        let _ = ostd::syscall::sys_log(super::faults::ANNOUNCE);
        let connect = |code: &str, message: &str| end(REMOTE_FAILURE, sc::Phase::Connect, sc::Outcome::LocalError, code, message);
        let timed_out = || end(TIMED_OUT, sc::Phase::Connect, sc::Outcome::Timeout, "timeout", "timed out");
        let mut net = NetClient::new();
        let addr = match &parsed.host {
            Host::V4(a) => a.octets(),
            Host::V6(_) => return connect("ipv6_unsupported", "IPv6 is not supported by the network service"),
            Host::Name(name) => match net.dns_lookup(name) {
                Ok(a) => a,
                Err(ViError::PermissionDenied) => return connect("resolve_denied", "name resolution denied"),
                Err(_) if deadline.passed() => return timed_out(),
                Err(e) => return connect("resolve_failed", &format!("cannot resolve {name}: {e:?}")),
            },
        };
        if deadline.passed() {
            return timed_out();
        }
        let socket = match net.tcp_connect(addr, parsed.port) {
            Ok(s) => s,
            Err(ViError::PermissionDenied) => return connect("connect_denied", "network connection denied"),
            Err(_) if deadline.passed() => return timed_out(),
            Err(e) => return connect("connect_failed", &format!("connect failed: {e:?}")),
        };
        if deadline.passed() {
            let _ = net.tcp_close_graceful(socket);
            return timed_out();
        }
        let mut transport = NetTransport { net, socket, deadline: &deadline };
        let (mut outcome, phase, lost, prompt_timeout) = match session.as_mut() {
            Some(session) => {
                let size = session.capabilities().map(|c| (c.rows, c.cols)).ok().filter(|&(r, c)| r > 0 && c > 0).unwrap_or((24, 80));
                let mode = match &parsed.command {
                    Some(cmd) => Mode::Exec(cmd.clone()),
                    None => Mode::Pty { term: "cellos-vt1", rows: size.0, cols: size.1 },
                };
                let mut term = Term {
                    session,
                    raw: None,
                    host: format!("{}", parsed.host),
                    port: parsed.port,
                    user: parsed.user.clone(),
                    pty: parsed.command.is_none(),
                    escape: Escape::default(),
                    size,
                    pending_resize: None,
                    deadline: &deadline,
                    stdin_ended: false,
                    completion,
                    mode: rmode,
                    phase: sc::Phase::Connect,
                    session_lost: false,
                    identity,
                    prompt_timeout: false,
                    #[cfg(feature = "p09-faults")]
                    faults: faults::Faults::new(parsed.command.as_deref()),
                };
                let outcome = session::run(&mut transport, &mut term, &parsed.user, auth, &mode, &policy);
                // Leave raw mode before any local message.
                if let Some(mut raw) = term.raw.take() {
                    let _ = raw.leave(term.session);
                }
                #[cfg(feature = "p09-faults")]
                term.faults.finish(&outcome);
                (outcome, term.phase, term.session_lost, term.prompt_timeout)
            }
            None => {
                let mode = Mode::Exec(parsed.command.clone().unwrap_or_default());
                let mut term = PipeTerm {
                    stdin: granted(context, GrantSlot::Stdin),
                    stdout: granted(context, GrantSlot::Stdout),
                    stderr,
                    host: format!("{}", parsed.host),
                    port: parsed.port,
                    deadline: &deadline,
                    identity,
                    completion,
                    mode: rmode,
                    phase: sc::Phase::Connect,
                    stdin_ended: false,
                    broken: false,
                };
                let outcome = session::run(&mut transport, &mut term, &parsed.user, auth, &mode, &policy);
                (outcome, term.phase, false, false)
            }
        };
        // A secret prompt that ran into the phase budget or `--timeout` is a timeout, not a
        // refusal (the prompt itself ended the wait).
        if prompt_timeout && matches!(outcome.ending, Ending::Auth | Ending::Credential(_)) {
            outcome.ending = Ending::TimedOut;
        }
        // FIN, not a silent discard: the server must see the connection end (a peer left
        // half-open waits out its login grace time and penalises this source).
        let _ = transport.net.tcp_close_graceful(socket);
        write_all(completion, &super::completion(&outcome, rmode, phase, lost).encode());
        let (status, message) = if lost {
            (SESSION_LOST, Some(String::from("terminal session lost")))
        } else {
            report(&outcome, &parsed.host, parsed.port)
        };
        match message {
            Some(message) => {
                write_all(stderr, format!("ssh: {message}\n").as_bytes());
                status
            }
            None => status,
        }
    }

    /// Lane-only fault hooks and measurements (`p09-faults`; command-contracts §4.4 lane).
    /// A remote command starting `: p09-fault <point> <action>;` makes the client announce
    /// `<point>` on the kernel log and hold there (transport serviced, no packet
    /// processed) so the lane runner can act; `: p09-sink;` consumes stdout into a digest
    /// at a throttled pace instead of the terminal.
    #[cfg(feature = "p09-faults")]
    mod faults {
        use super::*;
        use sha2::{Digest, Sha256};
        use vissh::session::Outcome;

        /// Lowered rekey triggers so one lane run crosses them (production: 1 h, 1 GiB).
        pub const LANE_REKEY: RekeyPolicy = RekeyPolicy {
            max_age_ms: 20_000,
            limits: vissh::sunset::RekeyLimits { bytes: 2 << 20, packets: 4096 },
            kex_ms: 30_000,
        };
        const HOLD_MS: u64 = 5_000;
        const THROTTLE: usize = 4096;

        pub struct Faults {
            hook: Option<(String, String)>,
            sink: Option<(u64, Sha256, usize)>,
        }

        fn point_name(point: Point) -> &'static str {
            match point {
                Point::BeforeRequest => "before-request",
                Point::RequestSent => "may-have-run",
                Point::Accepted => "accepted",
                Point::ExitStatus => "exit-before-drain",
                Point::RekeyStart => "rekey-start",
            }
        }

        impl Faults {
            pub fn new(command: Option<&str>) -> Self {
                let command = command.unwrap_or("");
                let hook = command.strip_prefix(": p09-fault ").and_then(|rest| {
                    let (spec, _) = rest.split_once(';')?;
                    let mut words = spec.split_whitespace();
                    let (point, action) = (words.next()?, words.next()?);
                    words.next().is_none().then(|| (String::from(point), String::from(action)))
                });
                let sink = command.starts_with(": p09-sink;").then(|| (0, Sha256::new(), 0));
                Faults { hook, sink }
            }

            /// The hooked point is announced and held, so the runner's action (seen up to
            /// half a second later) lands before the session moves past it.
            pub fn checkpoint(&self, point: Point) -> u64 {
                let name = point_name(point);
                let Some((hooked, action)) = &self.hook else { return 0 };
                if hooked != name {
                    return 0;
                }
                let _ = ostd::syscall::sys_log(&format!("P04 INFO p09-hook {name} {action}\n"));
                HOLD_MS
            }

            /// Consume stdout into the digest; false when no sink is configured.
            pub fn sink(&mut self, data: &[u8]) -> bool {
                let Some((count, hash, since)) = &mut self.sink else { return false };
                *count += data.len() as u64;
                hash.update(data);
                *since += data.len();
                // A slow consumer: one tick per 4 KiB, so flow control has to hold.
                while *since >= THROTTLE {
                    *since -= THROTTLE;
                    let _ = ostd::syscall::sys_set_timer(1);
                }
                true
            }

            pub fn finish(&mut self, outcome: &Outcome) {
                let _ = ostd::syscall::sys_log(&format!("P04 INFO ssh-rekeys {}\n", outcome.rekeys));
                if let Some((count, hash, _)) = self.sink.take() {
                    let digest = hash.finalize();
                    let mut hex = String::with_capacity(64);
                    for b in digest.iter() {
                        hex.push_str(&format!("{b:02x}"));
                    }
                    let _ = ostd::syscall::sys_log(&format!(
                        "P04 INFO ssh-bytes stdout={count} sha256={hex} peak_pending={} peak_chunk={}\n",
                        outcome.peaks.pending, outcome.peaks.chunk
                    ));
                }
            }
        }
    }
}
