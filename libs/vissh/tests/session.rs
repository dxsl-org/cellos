//! `session::run` against an in-memory Sunset server: trust gates every credential,
//! password is asked once, the exec command arrives byte-for-byte, stdout and stderr stay
//! apart, stdin and its EOF reach the server, and the request reply sets `Accepted`.
//! S9-6: rekey on bytes, packets, age and peer request with the first host key only,
//! KEX deadlines, write-ahead dispatch and interruption at each named point.
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::*;
use vissh::client::{HostKey, Transport, TransportError};
use vissh::session::{run, Dispatch, Ending, Input, Local, Mode, Phase, Point, RekeyPolicy, Trust, CHUNK};
use vissh::sunset::{ChanData, ChanHandle, Event, KeyType, RekeyLimits, Runner, ServEvent, Server, SignKey};

#[derive(Default)]
struct Seen {
    passwords: u32,
    command: Option<String>,
    execs: u32,
    stdin: Vec<u8>,
    stdin_eof: bool,
}

struct ToServer {
    srv: Runner<'static, Server>,
    key: SignKey,
    /// Offered from the second key exchange on (a host key change on rekey).
    next_key: Option<SignKey>,
    to_srv: Wire,
    from_srv: Vec<u8>,
    chan: Option<ChanHandle>,
    seen: Seen,
    replied: bool,
    /// Client data read but not echoed yet (the client's window allows the echo).
    echo: Vec<u8>,
    idle: u32,
    /// Monotonic test clock: one millisecond per `now_ms` call.
    clock: u64,
    /// The connection dropped (set by the local side's fault hold).
    killed: Rc<Cell<bool>>,
    /// The server has seen the exec request (read by the local side, write-ahead check).
    exec_seen: Rc<Cell<bool>>,
}

impl ToServer {
    fn new() -> Self {
        vissh::install_entropy(os_entropy);
        ToServer {
            srv: Runner::new_server_owned(),
            key: SignKey::generate(KeyType::Ed25519, None).unwrap(),
            next_key: None,
            to_srv: Wire::default(),
            from_srv: Vec::new(),
            chan: None,
            seen: Seen::default(),
            replied: false,
            echo: Vec::new(),
            idle: 0,
            clock: 0,
            killed: Rc::new(Cell::new(false)),
            exec_seen: Rc::new(Cell::new(false)),
        }
    }

    fn step(&mut self) {
        let mut accepted = None;
        match self.srv.progress() {
            Ok(Event::Serv(ServEvent::Hostkeys(h))) => {
                h.hostkeys(&[&self.key]).unwrap();
                if let Some(next) = self.next_key.take() {
                    self.key = next;
                }
            }
            Ok(Event::Serv(ServEvent::FirstAuth(a))) => a.reject().unwrap(),
            Ok(Event::Serv(ServEvent::PasswordAuth(a))) => {
                self.seen.passwords += 1;
                if a.password().unwrap() == "right" { a.allow().unwrap() } else { a.reject().unwrap() }
            }
            Ok(Event::Serv(ServEvent::OpenSession(o))) => accepted = Some(o.accept().unwrap()),
            Ok(Event::Serv(ServEvent::SessionExec(x))) => {
                self.seen.command = Some(x.command().unwrap().to_string());
                self.seen.execs += 1;
                self.exec_seen.set(true);
                x.succeed().unwrap();
            }
            _ => {}
        }
        if accepted.is_some() {
            self.chan = accepted;
        }
        if let Some(chan) = self.chan.as_ref() {
            if self.seen.command.is_some() && !self.replied {
                self.replied = true;
                self.srv.write_channel(chan, ChanData::Normal, b"out:").unwrap();
                self.srv.write_channel(chan, ChanData::Stderr, b"err!").unwrap();
            }
            // Like a real server, read the client's data as it arrives (bounded by the
            // window it granted) and echo it as the client's window allows.
            let mut b = [0u8; 256];
            if let Ok(n) = self.srv.read_channel(chan, ChanData::Normal, &mut b) {
                self.seen.stdin.extend_from_slice(&b[..n]);
                self.echo.extend_from_slice(&b[..n]);
            }
            let room = self.srv.write_channel_ready(chan, ChanData::Normal).ok().flatten().unwrap_or(0).min(256).min(self.echo.len());
            if room > 0 {
                let n = self.srv.write_channel(chan, ChanData::Normal, &self.echo[..room]).unwrap();
                self.echo.drain(..n);
            }
            if self.srv.is_channel_eof(chan) {
                self.seen.stdin_eof = true;
            }
        }
        self.to_srv.give(&mut self.srv).unwrap();
        self.from_srv.extend(drain(&mut self.srv));
    }
}

impl Transport for ToServer {
    fn send(&mut self, data: &[u8]) -> Result<usize, TransportError> {
        if self.killed.get() {
            return Err(TransportError::Closed);
        }
        self.to_srv.0.extend_from_slice(data);
        self.step();
        Ok(data.len())
    }
    fn recv(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        if self.killed.get() {
            return Err(TransportError::Closed);
        }
        self.step();
        let n = buf.len().min(self.from_srv.len());
        buf[..n].copy_from_slice(&self.from_srv[..n]);
        self.from_srv.drain(..n);
        Ok(n)
    }
    fn idle(&mut self) -> Result<(), TransportError> {
        self.idle += 1;
        if self.idle > 200_000 { Err(TransportError::TimedOut) } else { Ok(()) }
    }
    fn now_ms(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }
}

struct Me {
    trust: Trust,
    /// What a first-trust confirmation ends with.
    confirm: Trust,
    confirmed: u32,
    password: &'static str,
    asked: u32,
    keys: Vec<HostKey>,
    stdin: Vec<u8>,
    /// Everything this side will send; the echo of it ends the session.
    expect: Vec<u8>,
    sent_eof: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    progress: Vec<(Phase, Dispatch)>,
    points: Vec<Point>,
    /// Drop the connection while held at this point.
    kill_at: Option<Point>,
    killed: Rc<Cell<bool>>,
    exec_seen: Rc<Cell<bool>>,
}

impl Me {
    fn new(trust: Trust, password: &'static str) -> Self {
        Me {
            trust, confirm: Trust::Refused, confirmed: 0, password, asked: 0, keys: Vec::new(),
            stdin: b"a\0\xff".to_vec(), expect: b"a\0\xff".to_vec(), sent_eof: false, stdout: Vec::new(), stderr: Vec::new(),
            progress: Vec::new(), points: Vec::new(), kill_at: None, killed: Rc::new(Cell::new(false)), exec_seen: Rc::new(Cell::new(false)),
        }
    }
    /// Share the server's fault cells.
    fn wired(mut self, t: &ToServer) -> Self {
        self.killed = t.killed.clone();
        self.exec_seen = t.exec_seen.clone();
        self
    }
    /// Bulk stdin; the session ends by disconnect once the echo is back (no EOF: the
    /// in-memory peer would echo the EOF ahead of data it still has to echo).
    fn with_stdin(mut self, bytes: Vec<u8>) -> Self {
        self.expect = bytes.clone();
        self.stdin = bytes;
        self.sent_eof = true;
        self
    }
}

impl Local for Me {
    fn trust(&mut self, key: &HostKey) -> Trust {
        self.keys.push(key.clone());
        self.trust
    }
    fn confirm_unknown(&mut self, key: &HostKey) -> Trust {
        assert_eq!(self.keys.last(), Some(key), "confirms the key it was asked about");
        assert_eq!(self.asked, 0, "before any credential");
        self.confirmed += 1;
        self.confirm
    }
    fn password(&mut self, secret: &mut String) -> bool {
        self.asked += 1;
        secret.push_str(self.password);
        true
    }
    fn input(&mut self, buf: &mut [u8]) -> Input {
        if !self.stdin.is_empty() {
            let n = self.stdin.len().min(buf.len());
            buf[..n].copy_from_slice(&self.stdin[..n]);
            self.stdin.drain(..n);
            return Input::Data(n);
        }
        if !self.sent_eof {
            self.sent_eof = true;
            return Input::Eof;
        }
        // Once the echo and the stderr line are back, end the session locally.
        if self.stdout.len() == 4 + self.expect.len() && !self.stderr.is_empty() { Input::Disconnect } else { Input::Idle }
    }
    fn stdout(&mut self, data: &[u8]) {
        self.stdout.extend_from_slice(data);
    }
    fn stderr(&mut self, data: &[u8]) {
        self.stderr.extend_from_slice(data);
    }
    fn resized(&mut self) -> Option<(u16, u16)> {
        None
    }
    fn progress(&mut self, phase: Phase, dispatch: Dispatch) {
        if dispatch == Dispatch::MayHaveRun && self.progress.last() != Some(&(phase, dispatch)) {
            // Write-ahead: recorded before the server can have seen the request.
            assert!(!self.exec_seen.get(), "MayHaveRun after the request was sent");
        }
        self.progress.push((phase, dispatch));
    }
    fn checkpoint(&mut self, point: Point) -> u64 {
        self.points.push(point);
        if self.kill_at == Some(point) {
            self.killed.set(true);
            return 50;
        }
        0
    }
}

fn exec(t: &mut ToServer, me: &mut Me, policy: &RekeyPolicy) -> vissh::session::Outcome {
    run(t, me, "tester", &Mode::Exec("cmd".into()), policy)
}

const PROD: &RekeyPolicy = &RekeyPolicy::PRODUCTION;

#[test]
fn untrusted_host_key_ends_before_any_password() {
    for trust in [Trust::Mismatch, Trust::Denied, Trust::Unavailable] {
        let mut t = ToServer::new();
        let mut me = Me::new(trust, "right");
        let out = exec(&mut t, &mut me, PROD);
        assert!(matches!(out.ending, Ending::HostKey(got, _) if got == trust), "{out:?}");
        // A known-different or unavailable answer never prompts.
        assert_eq!((me.asked, me.confirmed, out.dispatch), (0, 0, Dispatch::NotSent));
        assert_eq!(me.progress, [(Phase::Trust, Dispatch::NotSent)]);
        for _ in 0..20 {
            t.step();
        }
        assert_eq!(t.seen.passwords, 0);
    }
}

#[test]
fn an_unknown_key_continues_only_after_a_stored_confirmation() {
    // Any confirmation outcome but KnownExact ends the session before any credential.
    for confirm in [Trust::Refused, Trust::Expired, Trust::NoConfirmation, Trust::Mismatch, Trust::Unavailable, Trust::Unknown] {
        let mut t = ToServer::new();
        let mut me = Me::new(Trust::Unknown, "right");
        me.confirm = confirm;
        let out = exec(&mut t, &mut me, PROD);
        assert!(matches!(out.ending, Ending::HostKey(got, _) if got == confirm), "{out:?}");
        assert_eq!((me.confirmed, me.asked, out.dispatch), (1, 0, Dispatch::NotSent));
        for _ in 0..20 {
            t.step();
        }
        assert_eq!((t.seen.passwords, t.seen.command.as_deref()), (0, None));
    }
    // A stored confirmation lets the session run as for a known key.
    let mut t = ToServer::new();
    let mut me = Me::new(Trust::Unknown, "right");
    me.confirm = Trust::KnownExact;
    let out = exec(&mut t, &mut me, PROD);
    assert_eq!((out.ending, out.dispatch), (Ending::Disconnected, Dispatch::Accepted));
    assert_eq!((me.confirmed, me.asked, t.seen.passwords), (1, 1, 1));
    assert_eq!(t.seen.command.as_deref(), Some("cmd"));
}

#[test]
fn wrong_password_is_tried_once_and_nothing_runs() {
    let mut t = ToServer::new();
    let mut me = Me::new(Trust::KnownExact, "wrong");
    let out = exec(&mut t, &mut me, PROD);
    assert_eq!(out.ending, Ending::Auth, "{out:?}");
    assert_eq!((me.asked, t.seen.passwords, out.dispatch), (1, 1, Dispatch::NotSent));
    assert_eq!(t.seen.command, None);
}

#[test]
fn exec_sends_exact_command_keeps_streams_apart_and_reports_lifecycle_write_ahead() {
    let mut t = ToServer::new();
    let mut me = Me::new(Trust::KnownExact, "right").wired(&t);
    let cmd = "printf 'x y'; exit 7 # \u{e9}";
    let out = run(&mut t, &mut me, "tester", &Mode::Exec(cmd.into()), PROD);
    assert_eq!(out.ending, Ending::Disconnected, "{out:?}");
    assert_eq!((out.dispatch, out.rekeys), (Dispatch::Accepted, 0));
    assert_eq!(t.seen.command.as_deref(), Some(cmd));
    assert_eq!(t.seen.stdin, b"a\0\xff");
    assert!(t.seen.stdin_eof || { for _ in 0..20 { t.step(); } t.seen.stdin_eof });
    assert_eq!(me.stdout, b"out:a\0\xff");
    assert_eq!(me.stderr, b"err!");
    assert_eq!(me.keys.len(), 1);
    assert_eq!(me.progress, [
        (Phase::Trust, Dispatch::NotSent), (Phase::Auth, Dispatch::NotSent), (Phase::Request, Dispatch::NotSent),
        (Phase::Request, Dispatch::MayHaveRun), (Phase::Running, Dispatch::Accepted),
    ]);
    assert_eq!(me.points, [Point::BeforeRequest, Point::RequestSent, Point::Accepted]);
}

#[test]
fn a_silent_peer_times_out_before_anything_is_sent() {
    vissh::install_entropy(os_entropy);
    struct Silent(u32);
    impl Transport for Silent {
        fn send(&mut self, d: &[u8]) -> Result<usize, TransportError> { Ok(d.len()) }
        fn recv(&mut self, _: &mut [u8]) -> Result<usize, TransportError> { Ok(0) }
        fn idle(&mut self) -> Result<(), TransportError> {
            self.0 += 1;
            if self.0 > 5 { Err(TransportError::TimedOut) } else { Ok(()) }
        }
        fn now_ms(&mut self) -> u64 { 0 }
    }
    let mut me = Me::new(Trust::KnownExact, "right");
    let out = run(&mut Silent(0), &mut me, "tester", &Mode::Exec("true".into()), PROD);
    assert_eq!((out.ending, out.dispatch, me.keys.len()), (Ending::TimedOut, Dispatch::NotSent, 0));
}

#[test]
fn a_stalled_key_exchange_ends_at_its_own_deadline() {
    vissh::install_entropy(os_entropy);
    // The peer never answers and the transport itself never times out: only the
    // per-KEX deadline can end this (the clock advances one ms per reading).
    struct Stalled(u64);
    impl Transport for Stalled {
        fn send(&mut self, d: &[u8]) -> Result<usize, TransportError> { Ok(d.len()) }
        fn recv(&mut self, _: &mut [u8]) -> Result<usize, TransportError> { Ok(0) }
        fn idle(&mut self) -> Result<(), TransportError> { Ok(()) }
        fn now_ms(&mut self) -> u64 { self.0 += 1; self.0 }
    }
    let mut me = Me::new(Trust::KnownExact, "right");
    let policy = RekeyPolicy { kex_ms: 500, ..RekeyPolicy::PRODUCTION };
    let mut stalled = Stalled(0);
    let out = run(&mut stalled, &mut me, "tester", &Mode::Exec("true".into()), &policy);
    assert_eq!((out.ending, out.dispatch), (Ending::TimedOut, Dispatch::NotSent));
    assert!(stalled.0 >= 500 && stalled.0 < 10_000, "ended at the KEX deadline, clock {}", stalled.0);
}

/// Enough stdin to cross the lowered thresholds in both directions (it is echoed).
fn bulk() -> Vec<u8> {
    (0..160 * 1024u32).map(|i| (i % 251) as u8).collect()
}

#[test]
fn rekey_on_bytes_and_on_packets_keeps_the_session_and_its_bytes() {
    let lowered = [
        RekeyLimits { bytes: 8 * 1024, packets: 1 << 30 },
        // The in-memory peer echoes in 256-byte packets (about 640 inbound packets); up to
        // a 64 KiB window of them is in flight when the threshold is crossed.
        RekeyLimits { bytes: 1 << 30, packets: 200 },
    ];
    for limits in lowered {
        let mut t = ToServer::new();
        let mut me = Me::new(Trust::KnownExact, "right").with_stdin(bulk());
        let policy = RekeyPolicy { limits, ..RekeyPolicy::PRODUCTION };
        let out = exec(&mut t, &mut me, &policy);
        assert_eq!(out.ending, Ending::Disconnected, "{limits:?} {out:?}");
        assert!(out.rekeys >= 1, "{limits:?}: no rekey");
        // The trust store was consulted once; rekeys reuse the accepted key.
        assert_eq!(me.keys.len(), 1);
        assert_eq!(&me.stdout[4..], &bulk()[..]);
        assert_eq!(t.seen.stdin, bulk());
        assert!(me.points.iter().filter(|p| **p == Point::RekeyStart).count() as u32 >= out.rekeys);
        // Bounded local buffers: inbound pending at most one transport read, reads at most one chunk.
        assert!(out.peaks.pending <= 2048 && out.peaks.chunk <= CHUNK && out.peaks.chunk > 0, "{:?}", out.peaks);
    }
}

#[test]
fn rekey_on_age_and_at_the_peers_request_with_the_same_host_key() {
    // Age: the embedder's clock (one ms per reading) passes the lowered age.
    let mut t = ToServer::new();
    let mut me = Me::new(Trust::KnownExact, "right").with_stdin(bulk());
    let policy = RekeyPolicy { max_age_ms: 50, ..RekeyPolicy::PRODUCTION };
    let out = exec(&mut t, &mut me, &policy);
    assert_eq!(out.ending, Ending::Disconnected, "{out:?}");
    assert!(out.rekeys >= 1, "{out:?}");
    assert_eq!(me.keys.len(), 1);
    // The peer rekeys (its own lowered limits); simultaneous when both sides are low.
    for client in [RekeyLimits::PRODUCTION, RekeyLimits { bytes: 8 * 1024, packets: 1 << 30 }] {
        let mut t = ToServer::new();
        t.srv.set_rekey_limits(RekeyLimits { bytes: 8 * 1024, packets: 1 << 30 });
        let mut me = Me::new(Trust::KnownExact, "right").with_stdin(bulk());
        let policy = RekeyPolicy { limits: client, ..RekeyPolicy::PRODUCTION };
        let out = exec(&mut t, &mut me, &policy);
        assert_eq!(out.ending, Ending::Disconnected, "{client:?} {out:?}");
        assert!(out.rekeys >= 1);
        assert_eq!((me.keys.len(), &me.stdout[4..]), (1, &bulk()[..]));
    }
}

#[test]
fn a_changed_host_key_on_rekey_is_refused_without_asking_the_trust_store() {
    let mut t = ToServer::new();
    t.next_key = Some(SignKey::generate(KeyType::Ed25519, None).unwrap());
    let mut me = Me::new(Trust::KnownExact, "right").with_stdin(bulk());
    let policy = RekeyPolicy { limits: RekeyLimits { bytes: 8 * 1024, packets: 1 << 30 }, ..RekeyPolicy::PRODUCTION };
    let out = exec(&mut t, &mut me, &policy);
    let Ending::HostKey(Trust::Mismatch, offered) = &out.ending else { panic!("{out:?}") };
    assert_ne!(Some(offered), me.keys.first());
    assert_eq!((me.keys.len(), out.dispatch, out.rekeys), (1, Dispatch::Accepted, 0));
}

#[test]
fn interruption_at_each_point_reports_dispatch_and_never_replays() {
    for (point, dispatch) in [
        (Point::BeforeRequest, Dispatch::NotSent),
        (Point::RequestSent, Dispatch::MayHaveRun),
        (Point::Accepted, Dispatch::Accepted),
    ] {
        let mut t = ToServer::new();
        let mut me = Me::new(Trust::KnownExact, "right").wired(&t);
        me.kill_at = Some(point);
        let out = exec(&mut t, &mut me, PROD);
        assert_eq!((out.ending.clone(), out.dispatch), (Ending::TransportLost, dispatch), "{point:?}");
        assert!(!out.stdout_complete && !out.stderr_complete);
        // The request reached the server at most once and was never resent. At
        // RequestSent the frame exists but was never written: the record still says it
        // may have run, though this server never saw it.
        assert_eq!(t.seen.execs, u32::from(point == Point::Accepted), "{point:?}");
        assert_eq!(me.points.last(), Some(&point));
    }
}
