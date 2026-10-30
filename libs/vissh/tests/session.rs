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
use vissh::session::{run, AuthKind, Credential, Dispatch, Ending, Input, Local, Mode, Phase, Point, RekeyPolicy, SignRequest, Signed, Trust, CHUNK};
use vissh::sunset::ed25519_dalek::{Signer, SigningKey};
use vissh::sunset::{ChanData, ChanHandle, Event, KeyType, PubKey, RekeyLimits, Runner, ServEvent, Server, SignKey};
use vissh::userauth;

#[derive(Default)]
struct Seen {
    passwords: u32,
    /// `PubkeyAuth` events: the query for the key, then the signed request.
    pubkey_events: u32,
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
    /// The one public key this server authorizes.
    authorized: Option<[u8; 32]>,
    /// Offered authentication methods `(password, publickey)`.
    methods: (bool, bool),
    /// Refuse the signed request although the key query was accepted.
    refuse_signed: bool,
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
            authorized: None,
            methods: (true, true),
            refuse_signed: false,
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
            Ok(Event::Serv(ServEvent::FirstAuth(mut a))) => {
                a.set_auth_methods(self.methods.0, self.methods.1).unwrap();
                a.reject().unwrap()
            }
            Ok(Event::Serv(ServEvent::PasswordAuth(a))) => {
                self.seen.passwords += 1;
                if a.password().unwrap() == "right" { a.allow().unwrap() } else { a.reject().unwrap() }
            }
            Ok(Event::Serv(ServEvent::PubkeyAuth(a))) => {
                self.seen.pubkey_events += 1;
                let known = matches!(a.pubkey(), Ok(PubKey::Ed25519(k)) if Some(k.key.0) == self.authorized);
                if known && !(self.refuse_signed && self.seen.pubkey_events >= 2) { a.allow().unwrap() } else { a.reject().unwrap() }
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
    pk: Pk,
}

impl Me {
    fn new(trust: Trust, password: &'static str) -> Self {
        Me {
            trust, confirm: Trust::Refused, confirmed: 0, password, asked: 0, keys: Vec::new(),
            stdin: b"a\0\xff".to_vec(), expect: b"a\0\xff".to_vec(), sent_eof: false, stdout: Vec::new(), stderr: Vec::new(),
            progress: Vec::new(), points: Vec::new(), kill_at: None, killed: Rc::new(Cell::new(false)), exec_seen: Rc::new(Cell::new(false)),
            pk: Pk::default(),
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

/// What the local side does for publickey: the offered key, and how it signs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sign {
    /// A correct signature over the exact input.
    #[default]
    Good,
    /// A correct signature over the input of another session.
    OtherSession,
    /// A correct signature over the input of another user name.
    OtherUser,
    /// A correct signature, but the reported digest is of other bytes.
    WrongDigest,
    /// Bytes that are no signature.
    Garbage,
    /// A refusal from the key service.
    Refuse(Credential),
}

#[derive(Default)]
struct Pk {
    identity: Option<Result<[u8; 32], Credential>>,
    sign: Sign,
    identity_calls: u32,
    sign_calls: u32,
    requests: Vec<([u8; 32], String, [u8; 32])>,
}

/// A fixed test seed, never a generated key.
fn test_key() -> SigningKey {
    SigningKey::from_bytes(&[0x42; 32])
}

fn test_public() -> [u8; 32] {
    test_key().verifying_key().to_bytes()
}

impl Me {
    /// Publickey with the fixed test key and a correct signer.
    fn publickey(mut self) -> Self {
        self.pk.identity = Some(Ok(test_public()));
        self
    }
    fn signing(mut self, sign: Sign) -> Self {
        self.pk.sign = sign;
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
    fn identity(&mut self) -> Result<[u8; 32], Credential> {
        self.pk.identity_calls += 1;
        self.pk.identity.unwrap_or(Err(Credential::Unavailable))
    }
    fn sign(&mut self, r: &SignRequest<'_>) -> Result<Signed, Credential> {
        self.pk.sign_calls += 1;
        self.pk.requests.push((r.session_id, r.username.to_string(), r.message_sha256));
        let public = test_public();
        let input = |session: &[u8; 32], user: &str| userauth::signing_input(session, user, &public).unwrap();
        let (signed, reported) = match self.pk.sign {
            Sign::Refuse(credential) => return Err(credential),
            Sign::Good => (input(&r.session_id, r.username), r.message_sha256),
            Sign::OtherSession => (input(&[0xAB; 32], r.username), r.message_sha256),
            Sign::OtherUser => (input(&r.session_id, "someone"), r.message_sha256),
            Sign::WrongDigest => (input(&r.session_id, r.username), [0x77; 32]),
            Sign::Garbage => return Ok(Signed { signature: [0x11; 64], message_sha256: r.message_sha256 }),
        };
        Ok(Signed { signature: test_key().sign(&signed).to_bytes(), message_sha256: reported })
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
    run(t, me, "tester", AuthKind::Password, &Mode::Exec("cmd".into()), policy)
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
    let out = run(&mut t, &mut me, "tester", AuthKind::Password, &Mode::Exec(cmd.into()), PROD);
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
    let out = run(&mut Silent(0), &mut me, "tester", AuthKind::Password, &Mode::Exec("true".into()), PROD);
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
    let out = run(&mut stalled, &mut me, "tester", AuthKind::Password, &Mode::Exec("true".into()), &policy);
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

// ---- publickey (K6): the local side signs, the in-memory Sunset server verifies ----

fn pk_server() -> ToServer {
    let mut t = ToServer::new();
    t.authorized = Some(test_public());
    t
}

fn login_with(t: &mut ToServer, me: &mut Me, policy: &RekeyPolicy) -> vissh::session::Outcome {
    run(t, me, "tester", AuthKind::PublicKey, &Mode::Exec("cmd".into()), policy)
}

fn login(t: &mut ToServer, me: &mut Me) -> vissh::session::Outcome {
    login_with(t, me, PROD)
}

/// The login stopped before the server decided: nothing ran, no password was asked.
fn assert_refused_locally(out: &vissh::session::Outcome, me: &Me, t: &ToServer, credential: Credential) {
    assert_eq!(out.ending, Ending::Credential(credential), "{out:?}");
    assert_eq!(out.dispatch, Dispatch::NotSent);
    assert_eq!(me.progress.last(), Some(&(Phase::Auth, Dispatch::NotSent)));
    assert_eq!((me.asked, t.seen.passwords, t.seen.command.as_deref()), (0, 0, None));
}

#[test]
fn publickey_login_signs_through_the_local_side_and_the_server_verifies_the_signature() {
    let mut t = pk_server();
    let mut me = Me::new(Trust::KnownExact, "unused").wired(&t).publickey();
    let out = login(&mut t, &mut me);
    assert_eq!((out.ending.clone(), out.dispatch), (Ending::Disconnected, Dispatch::Accepted), "{out:?}");
    // One identity call, one signature, no password anywhere; the server saw the key
    // query and then the signed request (which it verified against its own session id).
    assert_eq!((me.pk.identity_calls, me.pk.sign_calls, me.asked, t.seen.passwords), (1, 1, 0, 0));
    assert_eq!(t.seen.pubkey_events, 2);
    assert_eq!(t.seen.command.as_deref(), Some("cmd"));
    assert_eq!(me.stdout, b"out:a\0\xff");
    // What the key service was asked: the connection's session id, the user name and the
    // digest of the RFC 4252 input built independently of Sunset.
    let (session, user, digest) = &me.pk.requests[0];
    assert_eq!((user.as_str(), *session == [0u8; 32]), ("tester", false));
    let input = userauth::signing_input(session, "tester", &test_public()).unwrap();
    assert_eq!(*digest, userauth::digest(&input));
    assert_eq!(&me.progress[..3], [(Phase::Trust, Dispatch::NotSent), (Phase::Auth, Dispatch::NotSent), (Phase::Request, Dispatch::NotSent)]);
    assert_eq!(me.keys.len(), 1);
}

#[test]
fn a_signature_for_another_session_or_user_never_reaches_the_server() {
    // The server would refuse either; the client refuses first, on its own verification
    // of the key service's answer, so exactly one `sign` call is made, the signed request
    // is never sent (the server saw only the key query) and no password is offered.
    for sign in [Sign::OtherSession, Sign::OtherUser, Sign::Garbage] {
        let mut t = pk_server();
        let mut me = Me::new(Trust::KnownExact, "right").publickey().signing(sign);
        let out = login(&mut t, &mut me);
        assert_refused_locally(&out, &me, &t, Credential::BadSignature);
        assert_eq!((me.pk.sign_calls, t.seen.pubkey_events), (1, 1), "{sign:?}");
    }
}

#[test]
fn a_digest_of_other_bytes_is_an_input_mismatch_before_anything_is_sent() {
    let mut t = pk_server();
    let mut me = Me::new(Trust::KnownExact, "right").publickey().signing(Sign::WrongDigest);
    let out = login(&mut t, &mut me);
    assert_refused_locally(&out, &me, &t, Credential::InputMismatch);
    assert_eq!((me.pk.sign_calls, t.seen.pubkey_events), (1, 1));
}

#[test]
fn a_refusal_of_identity_or_signing_ends_the_login_with_that_credential() {
    for credential in [Credential::UnknownIdentity, Credential::Denied, Credential::Unavailable, Credential::Protocol] {
        let mut t = pk_server();
        let mut me = Me::new(Trust::KnownExact, "right");
        me.pk.identity = Some(Err(credential));
        let out = login(&mut t, &mut me);
        assert_refused_locally(&out, &me, &t, credential);
        // No key was offered: the server never saw a publickey request.
        assert_eq!((me.pk.identity_calls, me.pk.sign_calls, t.seen.pubkey_events), (1, 0, 0));
    }
    for credential in [Credential::UnlockCancelled, Credential::UnlockFailed, Credential::UnlockLocked, Credential::Timeout, Credential::UnlockNeedsTerminal] {
        let mut t = pk_server();
        let mut me = Me::new(Trust::KnownExact, "right").publickey().signing(Sign::Refuse(credential));
        let out = login(&mut t, &mut me);
        assert_refused_locally(&out, &me, &t, credential);
        assert_eq!((me.pk.identity_calls, me.pk.sign_calls, t.seen.pubkey_events), (1, 1, 1));
    }
    // A key that is not a valid curve point is refused as a protocol error.
    let mut t = pk_server();
    let mut me = Me::new(Trust::KnownExact, "right");
    me.pk.identity = Some(Ok([0x02; 32]));
    let out = login(&mut t, &mut me);
    assert_refused_locally(&out, &me, &t, Credential::Protocol);
}

#[test]
fn a_server_without_publickey_ends_auth_with_no_identity_and_no_password_prompt() {
    let mut t = pk_server();
    t.methods = (true, false);
    let mut me = Me::new(Trust::KnownExact, "right").publickey();
    let out = login(&mut t, &mut me);
    assert_eq!(out.ending, Ending::Auth, "{out:?}");
    assert_eq!(out.dispatch, Dispatch::NotSent);
    assert_eq!((me.pk.identity_calls, me.pk.sign_calls, me.asked, t.seen.passwords, t.seen.pubkey_events), (0, 0, 0, 0, 0));
}

#[test]
fn a_server_that_does_not_know_the_key_never_costs_a_signature_or_a_password() {
    let mut t = ToServer::new();
    let mut me = Me::new(Trust::KnownExact, "right").publickey();
    let out = login(&mut t, &mut me);
    assert_eq!(out.ending, Ending::Auth, "{out:?}");
    assert_eq!(out.dispatch, Dispatch::NotSent);
    // The key was offered once; `sign` (the passphrase prompt) was never reached.
    assert_eq!((me.pk.identity_calls, me.pk.sign_calls, me.asked, t.seen.passwords, t.seen.pubkey_events), (1, 0, 0, 0, 1));
}

#[test]
fn a_refused_signature_is_tried_once_and_nothing_else_is_offered() {
    let mut t = pk_server();
    t.refuse_signed = true;
    let mut me = Me::new(Trust::KnownExact, "right").publickey();
    let out = login(&mut t, &mut me);
    assert_eq!(out.ending, Ending::Auth, "{out:?}");
    assert_eq!(out.dispatch, Dispatch::NotSent);
    assert_eq!((me.pk.identity_calls, me.pk.sign_calls, me.asked, t.seen.passwords, t.seen.pubkey_events), (1, 1, 0, 0, 2));
    assert_eq!(t.seen.command, None);
}

#[test]
fn an_untrusted_or_changed_host_key_never_reaches_identity_or_sign() {
    for (trust, confirm) in [(Trust::Mismatch, Trust::Refused), (Trust::Denied, Trust::Refused), (Trust::Unavailable, Trust::Refused), (Trust::Unknown, Trust::Refused), (Trust::Unknown, Trust::NoConfirmation)] {
        let mut t = pk_server();
        let mut me = Me::new(trust, "right").publickey();
        me.confirm = confirm;
        let out = login(&mut t, &mut me);
        assert!(matches!(out.ending, Ending::HostKey(..)), "{out:?}");
        assert_eq!((me.pk.identity_calls, me.pk.sign_calls, me.asked), (0, 0, 0));
        for _ in 0..20 {
            t.step();
        }
        assert_eq!(t.seen.pubkey_events, 0);
    }
}

#[test]
fn password_mode_with_both_methods_offered_never_touches_the_key() {
    let mut t = pk_server();
    let mut me = Me::new(Trust::KnownExact, "right").wired(&t).publickey();
    let out = exec(&mut t, &mut me, PROD);
    assert_eq!((out.ending, out.dispatch), (Ending::Disconnected, Dispatch::Accepted));
    // `Pubkey`/`AgentSign` skipped, the password asked once.
    assert_eq!((me.asked, t.seen.passwords), (1, 1));
    assert_eq!((me.pk.identity_calls, me.pk.sign_calls, t.seen.pubkey_events), (0, 0, 0));
}

#[test]
fn a_publickey_session_rekeys_without_a_second_authentication() {
    let mut t = pk_server();
    let mut me = Me::new(Trust::KnownExact, "right").publickey().with_stdin(bulk());
    let policy = RekeyPolicy { limits: RekeyLimits { bytes: 8 * 1024, packets: 1 << 30 }, ..RekeyPolicy::PRODUCTION };
    let out = login_with(&mut t, &mut me, &policy);
    assert_eq!(out.ending, Ending::Disconnected, "{out:?}");
    assert!(out.rekeys >= 1, "{out:?}");
    // The host key rule holds as in password mode: one trust decision, one signature.
    assert_eq!((me.keys.len(), me.pk.identity_calls, me.pk.sign_calls, t.seen.pubkey_events), (1, 1, 1, 2));
    assert_eq!(&me.stdout[4..], &bulk()[..]);
}

#[test]
fn sunsets_own_message_equals_the_rfc_4252_construction_for_every_user_name_length() {
    // `answer_sign` compares Sunset's serialisation with `userauth::signing_input` byte for
    // byte; a login that reaches the server proves they agree, here at the length extremes.
    for user in ["u", "a.b_c-9", "tester", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"] {
        let mut t = pk_server();
        let mut me = Me::new(Trust::KnownExact, "unused").wired(&t).publickey();
        let out = run(&mut t, &mut me, user, AuthKind::PublicKey, &Mode::Exec("cmd".into()), PROD);
        assert_eq!((out.ending.clone(), out.dispatch), (Ending::Disconnected, Dispatch::Accepted), "{user}: {out:?}");
        assert_eq!((me.pk.requests[0].1.as_str(), t.seen.pubkey_events), (user, 2));
    }
}
