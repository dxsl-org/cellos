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
use vissh::args::{self, Host};
use vissh::session::{Dispatch, Ending, Outcome, Phase};

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
    use vissh::session::{self, Dispatch, Input, Local, Mode, Phase, RekeyPolicy, Trust};
    #[cfg(feature = "p09-faults")]
    use vissh::session::Point;
    use zeroize::Zeroize;

    const FOREVER: u64 = u64::MAX;
    /// Phase budgets (command-contracts §4.2): auth/unlock including prompts, then the
    /// channel open and request until the server's reply.
    const AUTH_MS: u64 = 120_000;
    const REQUEST_MS: u64 = 30_000;
    const MAX_PASSWORD: usize = 1024;
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

        /// Write-ahead lifecycle frame for the shell (newest whole frame wins).
        fn record(&self, record: &sc::CompletionV1) {
            write_all(self.completion, &record.encode());
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
            let code = VfsClient::new().trust_lookup(&self.host, self.port, key.algorithm, &key.blob());
            match code {
                Ok(api::ipc::ssh_trust::KNOWN_EXACT) => Trust::KnownExact,
                Ok(api::ipc::ssh_trust::UNKNOWN) => Trust::Unknown,
                Ok(api::ipc::ssh_trust::MISMATCH) => Trust::Mismatch,
                Ok(api::ipc::ssh_trust::DENIED) => Trust::Denied,
                _ => Trust::Unavailable,
            }
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
            self.write(true, prompt.as_bytes());
            let ok = match self.session.set_mode(2, 0) {
                Ok(mut guard) => {
                    let ok = read_secret(guard.session(), secret);
                    let released = guard.release().is_ok();
                    Ok(ok && released)
                }
                Err(status) => Err(status),
            };
            let ok = match ok {
                Ok(ok) => ok,
                Err(status) => {
                    self.note(status);
                    return false;
                }
            };
            self.write(true, b"\n");
            if !ok {
                secret.zeroize();
            }
            ok
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
            let next = record_phase(phase);
            if next != self.phase {
                let budget = match phase {
                    Phase::Trust => None,
                    Phase::Auth => Some(now_ms().saturating_add(AUTH_MS)),
                    Phase::Request => Some(now_ms().saturating_add(REQUEST_MS)),
                    Phase::Running => Some(u64::MAX),
                };
                if let Some(at) = budget {
                    self.deadline.phase.set(at);
                }
                self.phase = next;
            }
            self.record(&progress_record(self.mode, self.phase, dispatch));
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

    /// One line under the echo-off lease: text, Backspace, Enter; Escape cancels.
    /// Pasted text is refused (§4.1: no newline/NUL, typed only).
    fn read_secret(session: &mut TerminalSession, secret: &mut String) -> bool {
        let mut paste = false;
        loop {
            let Ok(mut event) = session.next_event(u64::MAX) else { return false };
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
                    if secret.len() + text.len() > MAX_PASSWORD || text.contains('\0') {
                        Some(false)
                    } else {
                        secret.push_str(text);
                        None
                    }
                }
                EventKind::Key { code, modifiers, pressed: 1 | 2 } if !paste => match *code {
                    _ if crate::executor::submits_line(*code, *modifiers) => Some(true),
                    ESCAPE if *modifiers == 0 => Some(false),
                    BACKSPACE if *modifiers == 0 => {
                        secret.pop();
                        None
                    }
                    _ => None,
                },
                EventKind::Interrupt | EventKind::Eof => Some(false),
                _ => None,
            };
            event.scrub();
            if let Some(ok) = done {
                return ok;
            }
        }
    }

    /// Entry of `/bin/cellbox-ssh ssh ...`.
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
        if matches!(parsed.auth, args::Auth::PublicKey { .. }) {
            return refuse(REMOTE_FAILURE, "publickey_unavailable", "publickey authentication is not available yet");
        }
        let mut session = match TerminalSession::from_own_context(context) {
            Ok(session) => session,
            Err(_) => return refuse(REMOTE_FAILURE, "no_terminal", "an interactive terminal session is required"),
        };
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
        let size = session.capabilities().map(|c| (c.rows, c.cols)).ok().filter(|&(r, c)| r > 0 && c > 0).unwrap_or((24, 80));
        let mode = match &parsed.command {
            Some(cmd) => Mode::Exec(cmd.clone()),
            None => Mode::Pty { term: "cellos-vt1", rows: size.0, cols: size.1 },
        };
        let mut transport = NetTransport { net, socket, deadline: &deadline };
        let mut term = Term {
            session: &mut session,
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
            #[cfg(feature = "p09-faults")]
            faults: faults::Faults::new(parsed.command.as_deref()),
        };
        let outcome = session::run(&mut transport, &mut term, &parsed.user, &mode, &policy);
        // Leave raw mode before any local message.
        if let Some(mut raw) = term.raw.take() {
            let _ = raw.leave(term.session);
        }
        #[cfg(feature = "p09-faults")]
        term.faults.finish(&outcome);
        let (phase, lost) = (term.phase, term.session_lost);
        drop(term);
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
