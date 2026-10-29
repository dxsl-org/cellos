//! One connection, one session channel (command-contracts §4.2/§4.3): host trust before
//! any credential, password authentication, then a PTY shell or one exec command.
//!
//! S9-6: the rekey policy is the embedder's ([`RekeyPolicy`]); every KEX (the first and
//! each rekey, from either side) must finish within its deadline; a rekey's host key must
//! be exactly the one accepted at the first KEX and is never offered to the trust store
//! again; the session's lifecycle is reported to [`Local::progress`] write-ahead, so a
//! request frame is never handed to the transport before `MayHaveRun` is recorded.
use alloc::string::String;
use alloc::vec::Vec;

use sunset::packets::WinChange;
use sunset::{ChanData, ChanHandle, CliEvent, CliSessionExit, Client, Event, PubKey, Pty, RekeyLimits, Runner};
use zeroize::Zeroize;

use crate::client::{pump, ClientError, HostKey, Transport, TransportError, RECV_CHUNK};

/// D03 trust lookup result (abi-vfs-telemetry §4), and the end of a first-trust
/// confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    KnownExact,
    Unknown,
    Mismatch,
    Denied,
    Unavailable,
    /// The user refused the unknown key (or the confirmation was cancelled).
    Refused,
    /// The confirmation's time ran out.
    Expired,
    /// This terminal cannot run a trusted confirmation (no interactive frontend).
    NoConfirmation,
}

/// What the session runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// PTY shell with `TERM` and the terminal size at start.
    Pty { term: &'static str, rows: u16, cols: u16 },
    /// One non-PTY command string, sent byte-for-byte.
    Exec(String),
}

/// One unit of local input for the channel.
#[derive(Debug, PartialEq, Eq)]
pub enum Input {
    /// `n` bytes were written to the buffer.
    Data(usize),
    /// Nothing available now.
    Idle,
    /// End of local input: send channel EOF once.
    Eof,
    /// The local user asked to disconnect (`~.`).
    Disconnect,
}

/// Session phase (`phase` of the completion record, §4.3). `Connect` is the embedder's
/// own phase until the host key is offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Trust,
    Auth,
    Request,
    Running,
}

/// Named points of the session an embedder may observe (and, in a fault-injection
/// build, hold): see [`Local::checkpoint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Point {
    /// Authenticated; the channel open and the request are not sent yet.
    BeforeRequest,
    /// The request frame exists (`MayHaveRun` is recorded) and is next to be written to
    /// the transport. A hold here writes nothing: an interruption during it leaves the
    /// request unsent while the record already says it may have run.
    RequestSent,
    /// The server accepted the request.
    Accepted,
    /// The remote exit status (or signal) arrived; channel output may still be pending.
    ExitStatus,
    /// A rekey (a KEX after the first) started, from either side; the peer's KEX packets
    /// are not processed during a hold.
    RekeyStart,
}

/// The rekey and KEX-deadline policy. [`RekeyPolicy::PRODUCTION`] is the contract
/// (§4.2): rekey at the first of one hour since NEWKEYS, 1 GiB or 2^30 packets per
/// direction; every KEX finishes within 30 seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RekeyPolicy {
    pub max_age_ms: u64,
    pub limits: RekeyLimits,
    pub kex_ms: u64,
}

impl RekeyPolicy {
    pub const PRODUCTION: Self = Self { max_age_ms: 3_600_000, limits: RekeyLimits::PRODUCTION, kex_ms: 30_000 };
}

/// The local side of the session.
pub trait Local {
    /// Decide on the server's host key. Only `KnownExact` lets the session continue.
    fn trust(&mut self, key: &HostKey) -> Trust;
    /// Called only when `trust` answered `Unknown`, before anything else is sent: run the
    /// first-trust confirmation. `KnownExact` (the key is now stored) lets the session
    /// continue; any other answer ends it with that answer.
    fn confirm_unknown(&mut self, key: &HostKey) -> Trust;
    /// Ask once for the password, after the host key was trusted. Write it into
    /// `secret` and return true, or return false to cancel. `secret` is wiped after use.
    fn password(&mut self, secret: &mut String) -> bool;
    /// Fill `buf` with local input.
    fn input(&mut self, buf: &mut [u8]) -> Input;
    /// Remote standard output (or, in PTY mode, the whole terminal output).
    fn stdout(&mut self, data: &[u8]);
    /// Remote extended data type 1 (exec mode only).
    fn stderr(&mut self, data: &[u8]);
    /// A new terminal size since the last call, if any (PTY mode).
    fn resized(&mut self) -> Option<(u16, u16)>;
    /// The session entered `phase` with `dispatch` (write-ahead: `MayHaveRun` is reported
    /// before the request frame exists).
    fn progress(&mut self, phase: Phase, dispatch: Dispatch);
    /// The session reached `point`. The answer is how long to hold there, in ms, while
    /// only the transport is serviced (no packet is processed; at
    /// [`Point::RequestSent`] not even the request is written). Production answers 0.
    fn checkpoint(&mut self, _point: Point) -> u64 {
        0
    }
}

/// `dispatch` of the completion record (§4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    NotSent,
    MayHaveRun,
    Accepted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    RemoteExit(u32),
    RemoteSignal(String),
    /// The host key was not accepted (`Trust` other than `KnownExact`), or a rekey
    /// offered a key other than the one accepted first (`Mismatch`).
    HostKey(Trust, HostKey),
    /// Authentication was refused or cancelled.
    Auth,
    /// The server refused the shell/exec request.
    RequestRefused,
    /// The local user disconnected.
    Disconnected,
    /// The connection ended without an exit status or signal.
    Unknown,
    /// A phase, KEX or overall deadline passed.
    TimedOut,
    /// The transport ended (closed or failed) before the session completed.
    TransportLost,
    Error(String),
}

/// High-water marks of the session's local buffers (bytes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Peaks {
    /// Inbound transport bytes not yet taken by the SSH core (allocation high-water).
    pub pending: usize,
    /// Largest channel read handed to the local side.
    pub chunk: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub dispatch: Dispatch,
    pub ending: Ending,
    pub stdout_complete: bool,
    pub stderr_complete: bool,
    /// Completed rekeys (key exchanges after the first).
    pub rekeys: u32,
    pub peaks: Peaks,
}

impl Outcome {
    /// Local shell status (§4.3): the remote status when 0..=255, otherwise 255.
    pub fn status(&self) -> i32 {
        match self.ending {
            Ending::RemoteExit(n) if n <= 255 => n as i32,
            _ => 255,
        }
    }
}

/// Largest channel data read at once (§4.2: chunks of at most 16 KiB).
pub const CHUNK: usize = 16 * 1024;

pub fn run<T: Transport, L: Local>(t: &mut T, local: &mut L, user: &str, mode: &Mode, policy: &RekeyPolicy) -> Outcome {
    let mut runner: Runner<'static, Client> = Runner::new_client_owned();
    runner.set_rekey_limits(policy.limits);
    let mut state = State {
        dispatch: Dispatch::NotSent, chan: None, exit: None, password_asked: false, eof_sent: false,
        host: None, clock: RekeyClock::default(), peaks: Peaks::default(),
    };
    let ending = drive(&mut runner, t, local, user, mode, policy, &mut state).unwrap_or_else(|e| match e {
        ClientError::Transport(TransportError::TimedOut) => Ending::TimedOut,
        ClientError::Transport(_) | ClientError::Ended => Ending::TransportLost,
        ClientError::Protocol(sunset::Error::NoAuthMethods) => Ending::Auth,
        other => Ending::Error(alloc::format!("{other}")),
    });
    // An exit status that arrived before the transport ended stands; the incomplete
    // output is reported by `stdout_complete`.
    let ending = match (ending, state.exit.take()) {
        (Ending::Unknown | Ending::TransportLost, Some(exit)) => exit,
        (ending, _) => ending,
    };
    let complete = state.chan.as_ref().is_some_and(|c| runner.is_channel_eof(c));
    Outcome {
        dispatch: state.dispatch, ending, stdout_complete: complete, stderr_complete: complete,
        rekeys: runner.newkeys_count().saturating_sub(1), peaks: state.peaks,
    }
}

struct State {
    dispatch: Dispatch,
    chan: Option<ChanHandle>,
    exit: Option<Ending>,
    password_asked: bool,
    eof_sent: bool,
    /// The host key accepted at the first KEX; every rekey must offer exactly it.
    host: Option<HostKey>,
    clock: RekeyClock,
    peaks: Peaks,
}

#[derive(Default)]
struct RekeyClock {
    /// When the KEX in progress was first seen.
    kex_started: Option<u64>,
    /// Completed key exchanges seen so far, and when the last one completed.
    newkeys: u32,
    newkeys_at: u64,
}

/// Service only the transport for `ms` (a fault-injection hold): runner output goes
/// out and input is queued, but no packet is processed. A closed or failed transport
/// ends the hold with its error.
fn hold<T: Transport>(runner: &mut Runner<'static, Client>, t: &mut T, pending: &mut Vec<u8>, ms: u64) -> Result<(), ClientError> {
    if ms == 0 {
        return Ok(());
    }
    let end = t.now_ms().saturating_add(ms);
    while t.now_ms() < end {
        pump(runner, t, pending)?;
        t.idle()?;
    }
    Ok(())
}

/// Rekey policy and KEX deadline, once per turn. Returns how long to hold at a rekey's
/// start ([`Point::RekeyStart`]).
fn rekey_turn<T: Transport, L: Local>(
    runner: &mut Runner<'static, Client>,
    t: &mut T,
    local: &mut L,
    policy: &RekeyPolicy,
    st: &mut RekeyClock,
) -> Result<u64, ClientError> {
    let mut hold_ms = 0;
    let now = t.now_ms();
    let newkeys = runner.newkeys_count();
    if newkeys != st.newkeys {
        st.newkeys = newkeys;
        st.newkeys_at = now;
    }
    if runner.kex_active() {
        let started = *st.kex_started.get_or_insert_with(|| {
            if newkeys > 0 {
                hold_ms = local.checkpoint(Point::RekeyStart);
            }
            now
        });
        if now.saturating_sub(started) >= policy.kex_ms {
            return Err(ClientError::Transport(TransportError::TimedOut));
        }
    } else {
        st.kex_started = None;
        if newkeys > 0 && now.saturating_sub(st.newkeys_at) >= policy.max_age_ms {
            runner.request_rekey();
        }
    }
    Ok(hold_ms)
}

#[allow(clippy::too_many_arguments)]
fn drive<T: Transport, L: Local>(
    runner: &mut Runner<'static, Client>,
    t: &mut T,
    local: &mut L,
    user: &str,
    mode: &Mode,
    policy: &RekeyPolicy,
    st: &mut State,
) -> Result<Ending, ClientError> {
    let mut pending = Vec::with_capacity(RECV_CHUNK);
    let mut buf = alloc::vec![0u8; CHUNK];
    // Local input not yet taken by the channel: one chunk, wiped as it is sent and on drop.
    let mut out = zeroize::Zeroizing::new(alloc::vec![0u8; CHUNK]);
    let mut unsent = (0usize, 0usize);
    let mut open_session = false;
    loop {
        let hold_ms = rekey_turn(runner, t, local, policy, &mut st.clock)?;
        hold(runner, t, &mut pending, hold_ms)?;
        let mut progressed = false;
        let mut stop = None;
        let mut point = None;
        match runner.progress() {
            // Every offered method was refused (or the password was not given).
            Err(sunset::Error::NoAuthMethods) => return Ok(Ending::Auth),
            Err(e) => return Err(e.into()),
            Ok(Event::Cli(CliEvent::Hostkey(h))) => {
                let key = match h.hostkey()? {
                    PubKey::Ed25519(k) => HostKey { algorithm: "ssh-ed25519", key: k.key.0 },
                    #[allow(unreachable_patterns)]
                    _ => return Ok(Ending::Error(String::from("unsupported host key type"))),
                };
                let decision = match &st.host {
                    // A rekey: exactly the accepted key, never a new trust decision.
                    Some(accepted) if *accepted == key => Trust::KnownExact,
                    Some(_) => Trust::Mismatch,
                    None => {
                        local.progress(Phase::Trust, st.dispatch);
                        match local.trust(&key) {
                            Trust::Unknown => local.confirm_unknown(&key),
                            other => other,
                        }
                    }
                };
                match decision {
                    Trust::KnownExact => {
                        h.accept()?;
                        if st.host.is_none() {
                            st.host = Some(key);
                            local.progress(Phase::Auth, st.dispatch);
                        }
                    }
                    other => {
                        // Upstream reports the refusal as `BadUsage`; it is intended here.
                        let _ = h.reject();
                        stop = Some(Ending::HostKey(other, key));
                    }
                }
            }
            Ok(Event::Cli(CliEvent::Username(u))) => u.username(user)?,
            Ok(Event::Cli(CliEvent::Password(p))) => {
                if st.password_asked {
                    // Exclusive auth, no retry: a second request means the first failed.
                    p.skip()?;
                    stop = Some(Ending::Auth);
                } else {
                    st.password_asked = true;
                    let mut secret = String::new();
                    let given = local.password(&mut secret);
                    let r = if given { p.password(&secret) } else { p.skip() };
                    secret.zeroize();
                    r?;
                    if !given {
                        stop = Some(Ending::Auth);
                    }
                }
            }
            Ok(Event::Cli(CliEvent::Pubkey(p))) => p.skip()?,
            Ok(Event::Cli(CliEvent::AgentSign(s))) => s.skip()?,
            Ok(Event::Cli(CliEvent::Authenticated)) => {
                local.progress(Phase::Request, st.dispatch);
                point = Some(Point::BeforeRequest);
                open_session = true;
            }
            Ok(Event::Cli(CliEvent::SessionOpened(mut o))) => {
                // Write-ahead: the request may run remotely once its frame exists.
                st.dispatch = Dispatch::MayHaveRun;
                local.progress(Phase::Request, st.dispatch);
                match mode {
                    Mode::Pty { term, rows, cols } => {
                        let mut pty = Pty {
                            term: heapless::String::new(),
                            cols: u32::from(*cols),
                            rows: u32::from(*rows),
                            width: 0,
                            height: 0,
                            modes: heapless::Vec::new(),
                        };
                        pty.term.push_str(term).map_err(|_| ClientError::Protocol(sunset::Error::msg("TERM")))?;
                        o.pty(pty)?;
                        o.shell()?;
                    }
                    Mode::Exec(cmd) => o.exec(cmd)?,
                }
                point = Some(Point::RequestSent);
            }
            Ok(Event::Cli(CliEvent::SessionExit(x))) => {
                if st.exit.is_some() {
                    // Duplicate or conflicting outcomes: uncertainty, not last-wins.
                    st.exit = None;
                    stop = Some(Ending::Error(String::from("duplicate remote exit outcome")));
                } else {
                    st.exit = Some(match x {
                        CliSessionExit::Status(n) => Ending::RemoteExit(n),
                        CliSessionExit::Signal(s) => Ending::RemoteSignal(String::from(s.signal)),
                    });
                    point = Some(Point::ExitStatus);
                }
            }
            Ok(Event::Cli(CliEvent::Banner(_))) => {}
            Ok(Event::Cli(CliEvent::Defunct)) => {
                return Ok(st.exit.take().unwrap_or(match st.dispatch {
                    Dispatch::NotSent if !st.password_asked => Ending::Error(String::from("server closed the connection")),
                    Dispatch::NotSent => Ending::Auth,
                    _ => Ending::Unknown,
                }));
            }
            Ok(Event::None) => {}
            Ok(_) => progressed = true,
        }
        if let Some(ending) = stop {
            let _ = pump(runner, t, &mut pending);
            return Ok(ending);
        }
        if let Some(point) = point {
            if point == Point::RequestSent {
                // The request frame is queued, not yet written: a hold here writes nothing.
                let ms = local.checkpoint(point);
                let end = t.now_ms().saturating_add(ms);
                while t.now_ms() < end {
                    t.idle()?;
                }
                pump(runner, t, &mut pending)?;
            } else {
                // Anything queued (the channel open, a reply) reaches the transport first.
                pump(runner, t, &mut pending)?;
                let ms = local.checkpoint(point);
                hold(runner, t, &mut pending, ms)?;
            }
            progressed = true;
        }
        st.peaks.pending = st.peaks.pending.max(pending.capacity());
        if open_session && st.chan.is_none() {
            st.chan = Some(runner.open_client_session()?);
            progressed = true;
        }
        if let Some(chan) = st.chan.as_ref() {
            if st.dispatch == Dispatch::MayHaveRun {
                match runner.channel_request_reply(chan) {
                    Some(true) => {
                        st.dispatch = Dispatch::Accepted;
                        local.progress(Phase::Running, st.dispatch);
                        pump(runner, t, &mut pending)?;
                        let ms = local.checkpoint(Point::Accepted);
                        hold(runner, t, &mut pending, ms)?;
                    }
                    Some(false) => return Ok(Ending::RequestRefused),
                    None => {}
                }
            }
            // Remote output first, so local input cannot starve it. In PTY mode extended
            // data is part of the one terminal output stream.
            for dt in [ChanData::Normal, ChanData::Stderr] {
                match runner.read_channel(chan, dt, &mut buf) {
                    Ok(0) | Err(_) => {}
                    Ok(n) => {
                        progressed = true;
                        st.peaks.chunk = st.peaks.chunk.max(n);
                        match (mode, dt) {
                            (Mode::Exec(_), ChanData::Stderr) => local.stderr(&buf[..n]),
                            _ => local.stdout(&buf[..n]),
                        }
                    }
                }
            }
            if runner.is_channel_closed(chan) {
                let _ = pump(runner, t, &mut pending);
                return Ok(st.exit.take().unwrap_or(Ending::Unknown));
            }
            if st.eof_sent {
                // Local input has ended; only a local disconnect can still come.
                if local.input(&mut []) == Input::Disconnect {
                    let _ = pump(runner, t, &mut pending);
                    return Ok(Ending::Disconnected);
                }
            } else if st.dispatch != Dispatch::NotSent {
                if let Some((rows, cols)) = local.resized() {
                    if matches!(mode, Mode::Pty { .. }) {
                        let w = WinChange { cols: u32::from(cols), rows: u32::from(rows), width: 0, height: 0 };
                        runner.term_window_change(chan, &w)?;
                        progressed = true;
                    }
                }
                // At most one unsent chunk waits (KEX pause, window or buffer full); the
                // next turn retries it after `progress()` had its chance to move the KEX.
                if unsent.1 > unsent.0 {
                    let w = runner.write_channel(chan, ChanData::Normal, &out[unsent.0..unsent.1])?;
                    out[unsent.0..unsent.0 + w].zeroize();
                    unsent.0 += w;
                    progressed |= w > 0;
                } else {
                    let room = runner.write_channel_ready(chan, ChanData::Normal)?.unwrap_or(0).min(CHUNK);
                    if room > 0 {
                        match local.input(&mut out[..room]) {
                            Input::Data(n) => {
                                let w = runner.write_channel(chan, ChanData::Normal, &out[..n])?;
                                out[..w].zeroize();
                                unsent = (w, n);
                                progressed = true;
                            }
                            Input::Eof => {
                                runner.send_channel_eof(chan)?;
                                st.eof_sent = true;
                                progressed = true;
                            }
                            Input::Disconnect => {
                                let _ = pump(runner, t, &mut pending);
                                return Ok(Ending::Disconnected);
                            }
                            Input::Idle => {}
                        }
                    }
                }
            }
        }
        if !pump(runner, t, &mut pending)? && !progressed {
            t.idle()?;
        }
    }
}
