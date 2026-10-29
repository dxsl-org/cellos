// SPDX-License-Identifier: MPL-2.0
//! Host tests of the owner-run first-trust confirmation (selectors 29–31) through the
//! real dispatch: a GUI session whose controller (the shell) runs a committed foreground
//! child (the ssh client), the live VFS registering and collecting, and the frontend
//! holder typing. The kernel is the in-process model of `session_tests.rs`.
use super::*;
use api::cell_session::trust::{self as tr, BeginTrustConfirmationV1, TrustChallengeV1, TrustDecisionV1, TrustPromptV1};
use api::services::session::{event_kind, key_code, modifiers};

const VFS: Principal = Principal { cell_id: 40, generation: 1 };
const VFS_TID: usize = 50;
const SSH: Principal = Principal { cell_id: 30, generation: 1 };
const SSH_CHILD: ChildRefV1 = ChildRefV1 { principal: SSH, root_tid: 88, publication_generation: 6 };
const C_OP: Ref = Ref { id: 4000, generation: 1 };
const PROMPT: TrustPromptV1<'static> = TrustPromptV1 { port: 2222, host: "gateway", algorithm: "ssh-ed25519", fingerprint: "SHA256:q0FkTQ5Ty1Wl7ixbhN5bNqH8eqbt4o2" };
const SECOND: u64 = 1_000_000_000;

/// The controller's baseline control writer (`ResolveBootstrap` writers are slots 6/7).
fn controller_control() -> Ref { endpoint(BOOT, 7) }
fn child_stdin_writer() -> Ref { child_endpoint(SSH_CHILD, 4) }
fn now_ns() -> u64 { fake::NOW_MS.with(|n| n.get()) * 1_000_000 }
fn advance(ms: u64) { fake::NOW_MS.with(|n| n.set(n.get() + ms)); }

fn text(s: &str) -> Vec<u8> {
    let mut frame = vec![0u8; 24]; frame[16..18].copy_from_slice(&2u16.to_le_bytes()); frame[18..20].copy_from_slice(&(s.len() as u16).to_le_bytes());
    frame.extend_from_slice(s.as_bytes()); frame
}
fn key(code: u32, mods: u32, pressed: u8) -> Vec<u8> {
    let mut frame = vec![0u8; 40]; frame[16..18].copy_from_slice(&1u16.to_le_bytes()); frame[18..20].copy_from_slice(&16u16.to_le_bytes());
    frame[24..28].copy_from_slice(&code.to_le_bytes()); frame[28..32].copy_from_slice(&mods.to_le_bytes()); frame[32] = pressed; frame
}
fn enter() -> Vec<u8> { key(key_code::ENTER, 0, 1) }
fn descriptor(challenge: Ref, target: Principal, expiry_ns: u64) -> Vec<u8> {
    let mut out = vec![0u8; TrustChallengeV1::MAX_LEN];
    let len = TrustChallengeV1 { challenge, target, expiry_ns, prompt: PROMPT }.encode(&mut out).unwrap(); out.truncate(len); out
}
fn challenge(n: u64) -> Ref { Ref { id: 7000 + n, generation: 3 } }

/// A GUI session with the ssh child committed in the foreground.
struct Trust { h: Harness, a: Attached, lease: Ref, epoch: u64 }
impl Trust {
    fn new() -> Self {
        let mut h = Harness::new();
        fake::SERVICES.with(|s| s.borrow_mut().push((api::syscall::service::VFS, VFS_TID)));
        let a = h.attach();
        Self::foreground(h, a)
    }
    /// The shell's own Serial session (Create + self-AttachFrontend): with no compositor
    /// it is the physical UART session, a byte stream without key/paste provenance.
    fn serial() -> Self {
        let mut h = Harness::new();
        fake::SERVICES.with(|s| s.borrow_mut().push((api::syscall::service::VFS, VFS_TID)));
        let mut create = [0; 40]; let mut w = Writer::new(&mut create);
        w.reference(S_OP).unwrap(); w.reference(BOOT).unwrap(); w.u8(0).unwrap(); w.u8(0).unwrap(); w.u16(24).unwrap(); w.u16(80).unwrap(); w.u16(0).unwrap();
        let session = Ref::from_bytes(&h.ok(SHELL, S_OP, 1, &create)[..16]).unwrap();
        let mut body = Vec::new();
        body.extend_from_slice(&session.to_bytes()); body.extend_from_slice(&SHELL.to_bytes()); body.extend_from_slice(&S_OP.to_bytes());
        body.extend_from_slice(&endpoint(BOOT, 0).to_bytes()); body.extend_from_slice(&endpoint(BOOT, 4).to_bytes());
        let attachment = Ref::from_bytes(&h.ok(SHELL, S_OP, 2, &body)).unwrap();
        assert_eq!(h.sessions.serial, Some(session));
        Self::foreground(h, Attached { frontend: ZERO, session, attachment, resume: ZERO, receipt: ZERO })
    }
    /// The shell commits the ssh child in the foreground of `a`.
    fn foreground(mut h: Harness, a: Attached) -> Self {
        model(|m| {
            m.set_operation(C_OP, 0);
            m.prepared.push(Prep { child: SSH_CHILD, operation: C_OP, preparer: SHELL, owner: SHELL, frontend: ZERO, committed: false, aborted: false, exited: false });
        });
        let mut body = Vec::new();
        body.extend_from_slice(&a.session.to_bytes()); body.extend_from_slice(&1u64.to_le_bytes()); body.extend_from_slice(&SSH_CHILD.to_bytes());
        for n in 0..6 { body.extend_from_slice(&child_endpoint(SSH_CHILD, n).to_bytes()); }
        let reply = h.ok(SHELL, S_OP, 3, &body);
        let mut r = Reader::new(&reply); let lease = r.reference().unwrap(); r.reference().unwrap(); let epoch = r.u64().unwrap();
        model(|m| m.commit(SSH_CHILD));
        h.sessions.maintain();
        let s = h.session(a.session);
        assert_eq!((s.reader, s.lease, s.epoch), (SSH, lease, epoch));
        Self { h, a, lease, epoch }
    }
    fn register(&mut self, n: u64, target: Principal) -> (Status, Vec<u8>) {
        self.h.call_as(VFS, VFS_TID, challenge(n), 29, &descriptor(challenge(n), target, now_ns() + 30 * SECOND))
    }
    fn confirmation(&mut self, n: u64) -> Ref { let (status, body) = self.register(n, SSH); assert_eq!(status, Status::Ok); Ref::from_bytes(&body).unwrap() }
    fn begin_as(&mut self, p: Principal, lane: Ref, lease: Ref, epoch: u64, confirmation: Ref) -> Status {
        let mut body = [0u8; BeginTrustConfirmationV1::LEN];
        BeginTrustConfirmationV1 { session: self.a.session, lease, epoch, confirmation }.encode(&mut body).unwrap();
        self.h.status(p, lane, 30, &body)
    }
    fn begin(&mut self, confirmation: Ref) -> Status { let (lease, epoch) = (self.lease, self.epoch); self.begin_as(SSH, C_OP, lease, epoch, confirmation) }
    fn collect_as(&mut self, p: Principal, tid: usize, confirmation: Ref) -> (Status, Option<TrustDecisionV1>) {
        let (status, body) = self.h.call_as(p, tid, confirmation, 31, &confirmation.to_bytes());
        (status, (status == Status::Ok).then(|| TrustDecisionV1::decode(&body).unwrap()))
    }
    fn collect(&mut self, confirmation: Ref) -> (Status, Option<TrustDecisionV1>) { self.collect_as(VFS, VFS_TID, confirmation) }
    /// The frontend holder types `frames` (one SubmitEvents batch; holders send epoch 0).
    fn submit(&mut self, frames: &[Vec<u8>]) -> Status {
        let events: Vec<u8> = frames.concat();
        let mut body = Vec::new();
        body.extend_from_slice(&self.a.session.to_bytes()); body.extend_from_slice(&self.a.attachment.to_bytes()); body.extend_from_slice(&0u64.to_le_bytes());
        body.extend_from_slice(&(frames.len() as u16).to_le_bytes()); body.extend_from_slice(&0u16.to_le_bytes()); body.extend_from_slice(&(events.len() as u32).to_le_bytes());
        body.extend_from_slice(&events);
        self.h.status(HOLDER, H_OP, 13, &body)
    }
    fn submit_raw(&mut self, bytes: &[u8]) -> Status {
        let mut body = Vec::new();
        body.extend_from_slice(&self.a.session.to_bytes()); body.extend_from_slice(&self.a.attachment.to_bytes()); body.extend_from_slice(&0u64.to_le_bytes());
        body.extend_from_slice(&(bytes.len() as u32).to_le_bytes()); body.extend_from_slice(&0u32.to_le_bytes()); body.extend_from_slice(bytes);
        self.h.status(HOLDER, H_OP, 20, &body)
    }
    fn prepare_paste(&mut self) -> Status {
        let mut body = Vec::new();
        body.extend_from_slice(&self.a.session.to_bytes()); body.extend_from_slice(&self.a.attachment.to_bytes()); body.extend_from_slice(&0u64.to_le_bytes());
        body.extend_from_slice(&4u32.to_le_bytes()); body.extend_from_slice(&0u32.to_le_bytes());
        self.h.status(HOLDER, H_OP, 14, &body)
    }
    /// The child switches its input to raw bytes (SetMode RAW).
    fn raw_mode(&mut self) {
        let mut body = Vec::new();
        body.extend_from_slice(&self.a.session.to_bytes()); body.extend_from_slice(&self.lease.to_bytes()); body.extend_from_slice(&self.epoch.to_le_bytes());
        body.extend_from_slice(&1u32.to_le_bytes()); body.extend_from_slice(&1u32.to_le_bytes());
        self.h.ok(SSH, C_OP, 7, &body);
    }
    /// Owner notices delivered to the controller's own control ring so far, decoded.
    fn notices(&mut self) -> Vec<(u16, Vec<u8>)> { self.delivered().into_iter().map(|(kind, payload, _)| (kind, payload)).collect() }
    fn delivered(&mut self) -> Vec<(u16, Vec<u8>, u64)> {
        self.h.sessions.maintain();
        let bytes: Vec<u8> = model(|m| m.writes.iter().filter(|w| w.0 == controller_control()).flat_map(|w| w.1.clone()).collect());
        let mut out = Vec::new(); let mut offset = 0;
        while offset < bytes.len() {
            let header = api::services::session::EventHeaderV1::decode(&bytes[offset..offset + 24 + u16::from_le_bytes([bytes[offset + 18], bytes[offset + 19]]) as usize]).unwrap();
            assert_eq!(header.epoch, self.epoch, "notices carry the foreground epoch");
            out.push((header.kind, bytes[offset + 24..offset + 24 + header.payload_len as usize].to_vec(), header.sequence));
            offset += 24 + header.payload_len as usize;
        }
        out
    }
    /// AcknowledgeControl of `through` by `p` with `lease` (the controller uses its baseline lease).
    fn ack_as(&mut self, p: Principal, lane: Ref, lease: Ref, through: u64) -> Status {
        let mut body = Vec::new();
        body.extend_from_slice(&self.a.session.to_bytes()); body.extend_from_slice(&lease.to_bytes()); body.extend_from_slice(&self.epoch.to_le_bytes()); body.extend_from_slice(&through.to_le_bytes());
        self.h.status(p, lane, 22, &body)
    }
    /// The shell acknowledges every owner notice delivered so far (it drew them).
    fn ack(&mut self) {
        let through = self.delivered().last().expect("a delivered notice").2;
        let baseline = self.h.session(self.a.session).baseline_lease;
        assert_eq!(self.ack_as(SHELL, S_OP, baseline, through), Status::Ok);
    }
    /// Begin, and the shell draws and acknowledges the prompt: the capture is armed.
    fn begin_armed(&mut self, confirmation: Ref) { assert_eq!(self.begin(confirmation), Status::Ok); self.ack(); }
    fn child_writes(&self) -> usize { model(|m| m.writes.iter().filter(|w| w.0 == child_stdin_writer()).count()) }
    fn routing(&self) -> (Principal, Ref, u64, u32) { let s = self.h.session(self.a.session); (s.reader, s.lease, s.epoch, s.mode) }
}

fn end(accepted: bool) -> (u16, Vec<u8>) { (event_kind::TRUST_PROMPT_END, u32::from(accepted).to_le_bytes().to_vec()) }

#[test]
fn only_the_live_vfs_task_registers_within_the_sixty_second_window() {
    let mut t = Trust::new();
    let body = descriptor(challenge(1), SSH, now_ns() + 10 * SECOND);
    // Another Cell, and the VFS principal from a task that is not the registered VFS, are refused.
    assert_eq!(t.h.call_as(OTHER, TID, challenge(1), 29, &body).0, Status::Denied);
    assert_eq!(t.h.call_as(VFS, TID, challenge(1), 29, &body).0, Status::Denied);
    fake::SERVICES.with(|s| s.borrow_mut().clear());
    assert_eq!(t.h.call_as(VFS, VFS_TID, challenge(1), 29, &body).0, Status::Denied);
    fake::SERVICES.with(|s| s.borrow_mut().push((api::syscall::service::VFS, VFS_TID)));
    // The deadline must lie in (now, now + 60 s].
    assert_eq!(t.h.call_as(VFS, VFS_TID, challenge(2), 29, &descriptor(challenge(2), SSH, now_ns())).0, Status::TimedOut);
    assert_eq!(t.h.call_as(VFS, VFS_TID, challenge(3), 29, &descriptor(challenge(3), SSH, now_ns() + 61 * SECOND)).0, Status::Malformed);
    assert!(t.h.sessions.challenges.is_empty());
    let (status, confirmation) = t.h.call_as(VFS, VFS_TID, challenge(1), 29, &body);
    assert_eq!(status, Status::Ok);
    assert!(!Ref::from_bytes(&confirmation).unwrap().is_zero());
    // A challenge Ref is single-use: a changed descriptor cannot rebind it.
    assert_eq!(t.h.call_as(VFS, VFS_TID, challenge(1), 29, &descriptor(challenge(1), SSH, now_ns() + 11 * SECOND)).0, Status::Denied);
}

#[test]
fn begin_is_admitted_only_for_the_target_as_the_exact_live_foreground() {
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    let (lease, epoch) = (t.lease, t.epoch);
    // The controller is a participant but not the target, and not the foreground reader.
    assert_eq!(t.begin_as(SHELL, S_OP, lease, epoch, confirmation), Status::Denied);
    // A Cell outside the session is refused at admission.
    assert_eq!(t.begin_as(OTHER, O_OP, lease, epoch, confirmation), Status::Denied);
    // Wrong lease, stale epoch, unknown confirmation.
    assert_eq!(t.begin_as(SSH, C_OP, t.a.attachment, epoch, confirmation), Status::Denied);
    assert_eq!(t.begin_as(SSH, C_OP, lease, epoch - 1, confirmation), Status::WrongEpoch);
    assert_eq!(t.begin_as(SSH, C_OP, lease, epoch, Ref { id: 999, generation: 1 }), Status::Stale);
    // A descriptor naming the controller cannot be begun by it: it is not the live foreground.
    let (status, body) = t.register(2, SHELL); assert_eq!(status, Status::Ok);
    let for_shell = Ref::from_bytes(&body).unwrap();
    let baseline = t.h.session(t.a.session).baseline_lease;
    assert_eq!(t.begin_as(SHELL, S_OP, baseline, epoch, for_shell), Status::Denied);
    assert!(t.h.session(t.a.session).capture.is_none());
    assert_eq!(t.begin(confirmation), Status::Ok);
    // Begun once: a second begin is stale.
    assert_eq!(t.begin(confirmation), Status::Stale);
}

#[test]
fn typed_yes_enter_accepts_once_with_exact_bindings_and_never_reaches_the_child() {
    let mut t = Trust::new();
    let before = t.routing();
    let expiry = now_ns() + 30 * SECOND;
    let confirmation = t.confirmation(1);
    // Pending before begin: reported, not consumed.
    let (status, pending) = t.collect(confirmation);
    assert_eq!((status, pending.map(|d| (d.decision, d.session, d.epoch))), (Status::Ok, Some((tr::DECISION_PENDING, ZERO, 0))));
    assert_eq!(t.begin(confirmation), Status::Ok);
    // The controller is told to draw the prompt from the registered descriptor.
    let notices = t.notices();
    assert_eq!(notices.len(), 1);
    assert_eq!((notices[0].0, TrustPromptV1::decode(&notices[0].1).unwrap()), (event_kind::TRUST_PROMPT, PROMPT));
    t.ack();
    // A key release (here Enter's) is no keystroke: it neither submits nor joins the line.
    assert_eq!(t.submit(&[text("y"), key(key_code::ENTER, 0, 0), text("es")]), Status::Ok);
    assert_eq!(t.collect(confirmation).1.unwrap().decision, tr::DECISION_PENDING);
    assert_eq!(t.submit(&[enter()]), Status::Ok);
    let decision = t.collect(confirmation).1.unwrap();
    // Every binding VFS compares: the SHA-256 is over its exact registered descriptor bytes.
    assert_eq!(decision, TrustDecisionV1 {
        decision: tr::DECISION_ACCEPT, challenge: challenge(1), target: SSH, session: t.a.session, epoch: t.epoch,
        descriptor_sha256: ostd::session::sha256(&descriptor(challenge(1), SSH, expiry)),
    });
    // Final exactly once.
    assert_eq!(t.collect(confirmation).0, Status::Stale);
    assert_eq!(t.notices()[1..], [end(true)]);
    // Until the shell acknowledges the end, the prompt may still be on screen: typing is dropped.
    assert_eq!(t.submit(&[text("ls"), enter()]), Status::Ok);
    assert!(t.h.session(t.a.session).typed.is_empty());
    t.ack();
    // Nothing typed during the capture reached the child; routing is exactly as before.
    let s = t.h.session(t.a.session);
    assert!(s.typed.is_empty() && s.raw.is_empty() && s.capture.is_none());
    assert_eq!(t.child_writes(), 0);
    assert_eq!(t.routing(), before);
    // Typing now goes to the child again.
    assert_eq!(t.submit(&[text("a")]), Status::Ok);
    assert_eq!(t.h.session(t.a.session).typed.len(), 1);
}

#[test]
fn an_answer_typed_before_the_prompt_is_acknowledged_never_counts() {
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    assert_eq!(t.begin(confirmation), Status::Ok);
    let prompt = t.delivered()[0].2;
    // Typed before the shell drew the prompt: dropped, neither routed nor counted.
    assert_eq!(t.submit(&[text("yes"), enter()]), Status::Ok);
    assert_eq!(t.prepare_paste(), Status::Cancelled);
    assert_eq!(t.collect(confirmation).1.unwrap().decision, tr::DECISION_PENDING);
    assert!(t.h.session(t.a.session).typed.is_empty());
    // Only the controller's own acknowledgement arms: the child's AcknowledgeControl and a
    // controller acknowledgement with the wrong lease do not.
    let (lease, attachment) = (t.lease, t.a.attachment);
    assert_eq!(t.ack_as(SSH, C_OP, lease, prompt), Status::Ok);
    assert_eq!(t.ack_as(SHELL, S_OP, attachment, prompt), Status::Denied);
    assert_eq!(t.submit(&[text("yes"), enter()]), Status::Ok);
    assert_eq!(t.collect(confirmation).1.unwrap().decision, tr::DECISION_PENDING);
    // A fresh line after the prompt is drawn is the answer; the earlier `yes` left nothing behind.
    t.ack();
    assert_eq!(t.submit(&[enter()]), Status::Ok);
    assert_eq!(t.collect(confirmation).1.unwrap().decision, tr::DECISION_REJECT);
}

/// A named way of answering an open capture.
type Answer = (&'static str, fn(&mut Trust) -> Status);

#[test]
fn any_other_line_escape_ctrl_c_or_paste_rejects() {
    let cases: [Answer; 6] = [
        ("no", |t| t.submit(&[text("no"), enter()])),
        ("yess", |t| t.submit(&[text("yess"), enter()])),
        ("ye", |t| t.submit(&[text("ye"), enter()])),
        ("escape", |t| t.submit(&[text("yes"), key(key_code::ESCAPE, 0, 1)])),
        ("ctrl-c", |t| t.submit(&[text("yes"), key(u32::from(b'c'), modifiers::CONTROL, 1), enter()])),
        ("paste", |t| { assert_eq!(t.prepare_paste(), Status::Cancelled); Status::Ok }),
    ];
    for (name, act) in cases {
        let mut t = Trust::new();
        let confirmation = t.confirmation(1);
        t.begin_armed(confirmation);
        assert_eq!(act(&mut t), Status::Ok, "{name}");
        assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_REJECT), "{name}");
        assert_eq!(t.notices()[1..], [end(false)], "{name}");
        t.ack();
        let s = t.h.session(t.a.session);
        // Ctrl-C never cancels the child, and nothing (not even the Enter after it) was routed.
        assert!(s.typed.is_empty() && s.controls.is_empty() && s.paste.is_none() && s.capture.is_none(), "{name}");
        assert!(!s.foreground.as_ref().unwrap().cancelled, "{name}");
    }
    // Erasing back to `yes` still accepts: the line, not the keystroke history, decides.
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    t.begin_armed(confirmation);
    assert_eq!(t.submit(&[text("yex"), key(key_code::BACKSPACE, 0, 1), text("s"), enter()]), Status::Ok);
    assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_ACCEPT));
}

#[test]
fn raw_bytes_never_answer_but_typed_events_do_in_raw_mode() {
    // SubmitRaw has no key/paste provenance: even `yes\r` rejects.
    for bytes in [&b"yes\r"[..], &b"\x1b[200~yes\r\x1b[201~"[..], &b"y"[..]] {
        let mut t = Trust::new();
        t.raw_mode();
        let before = t.routing();
        let confirmation = t.confirmation(1);
        t.begin_armed(confirmation);
        assert_eq!(t.submit_raw(bytes), Status::Ok);
        assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_REJECT), "{bytes:?}");
        t.ack();
        // No raw byte reached the child's stdin; its raw mode is untouched.
        assert_eq!(t.child_writes(), 0);
        assert_eq!(t.routing(), before);
        assert_eq!(t.submit_raw(b"x"), Status::Ok);
        assert_eq!(t.child_writes(), 1);
    }
    // The same child in raw mode is answered through typed events.
    let mut t = Trust::new();
    t.raw_mode();
    let confirmation = t.confirmation(1);
    t.begin_armed(confirmation);
    assert_eq!(t.submit(&[text("yes"), enter()]), Status::Ok);
    assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_ACCEPT));
    t.ack();
    assert_eq!(t.child_writes(), 0);
    assert!(t.h.session(t.a.session).raw.is_empty());
}

#[test]
fn the_deadline_expires_a_capture_and_is_never_extended() {
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    advance(20_000);
    t.begin_armed(confirmation);
    assert_eq!(t.submit(&[text("ye")]), Status::Ok);
    // Neither typing nor polling extends the registered deadline (30 s after registration).
    advance(9_000);
    assert_eq!(t.collect(confirmation).1.unwrap().decision, tr::DECISION_PENDING);
    advance(1_000);
    assert_eq!(t.collect(confirmation).1.unwrap().decision, tr::DECISION_EXPIRED);
    assert_eq!(t.notices()[1..], [end(false)]);
    // What was typed during the capture, or before the end is acknowledged, never reaches the child.
    assert_eq!(t.submit(&[text("s")]), Status::Ok);
    assert!(t.h.session(t.a.session).typed.is_empty());
    t.ack();
    assert_eq!(t.submit(&[text("s")]), Status::Ok);
    assert_eq!(t.h.session(t.a.session).typed.len(), 1);
    // An expired, never-begun challenge reports expired too, and cannot be begun.
    let other = t.confirmation(2);
    advance(31_000);
    assert_eq!(t.begin(other), Status::TimedOut);
    assert_eq!(t.collect(other).1.unwrap().decision, tr::DECISION_EXPIRED);
}

#[test]
fn foreground_restoration_or_frontend_loss_cancels_the_capture() {
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    t.begin_armed(confirmation);
    assert_eq!(t.submit(&[text("yes")]), Status::Ok);
    // The ssh child exits; the shell restores its foreground.
    model(|m| m.exit(SSH_CHILD));
    let mut body = Vec::new();
    body.extend_from_slice(&t.a.session.to_bytes()); body.extend_from_slice(&t.lease.to_bytes()); body.extend_from_slice(&t.epoch.to_le_bytes()); body.extend_from_slice(&SSH_CHILD.to_bytes());
    t.h.ok(SHELL, S_OP, 5, &body);
    let s = t.h.session(t.a.session);
    assert!(s.capture.is_none() && s.notices.is_empty());
    assert_eq!(s.reader, SHELL);
    assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_CANCELLED));
    // The Enter after restoration goes to the shell, never completing the old capture.
    assert_eq!(t.submit(&[enter()]), Status::Ok);
    assert_eq!(t.h.session(t.a.session).typed.len(), 1);

    // Frontend loss while armed cancels and releases at once.
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    t.begin_armed(confirmation);
    assert_eq!(t.h.release(t.a.frontend), Status::Ok);
    assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_CANCELLED));
    assert_eq!(t.notices()[1..], [end(false)]);
    assert!(t.h.session(t.a.session).capture.is_none());

    // Frontend loss while a decided prompt awaits its end acknowledgement releases too,
    // keeping the decision.
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    t.begin_armed(confirmation);
    assert_eq!(t.submit(&[text("no"), enter()]), Status::Ok);
    assert!(t.h.session(t.a.session).capture.is_some());
    assert_eq!(t.h.release(t.a.frontend), Status::Ok);
    assert!(t.h.session(t.a.session).capture.is_none());
    assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_REJECT));
}

#[test]
fn end_of_input_or_session_close_cancels_and_releases_the_quarantine() {
    // EndInput while still arming.
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    assert_eq!(t.begin(confirmation), Status::Ok);
    let mut body = Vec::new();
    body.extend_from_slice(&t.a.session.to_bytes()); body.extend_from_slice(&t.a.attachment.to_bytes()); body.extend_from_slice(&0u64.to_le_bytes());
    assert_eq!(t.h.status(HOLDER, H_OP, 21, &body), Status::Ok);
    assert!(t.h.session(t.a.session).capture.is_none());
    assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_CANCELLED));
    assert_eq!(t.notices()[1..], [end(false)]);

    // CloseSession while armed.
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    t.begin_armed(confirmation);
    assert_eq!(t.submit(&[text("ye")]), Status::Ok);
    t.h.ok(SHELL, S_OP, 11, &t.a.session.to_bytes());
    let s = t.h.session(t.a.session);
    assert!(s.closed && s.capture.is_none() && s.notices.is_empty());
    assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_CANCELLED));
}

#[test]
fn bounds_and_collection_are_bound_to_the_registering_principal() {
    let mut t = Trust::new();
    let first = t.confirmation(1);
    // One undecided confirmation per target.
    assert_eq!(t.register(2, SSH).0, Status::QuotaExceeded);
    // Four uncollected per registrar.
    for n in 2..=4 { assert_eq!(t.register(n, Principal { cell_id: 100 + n, generation: 1 }).0, Status::Ok); }
    assert_eq!(t.register(5, Principal { cell_id: 105, generation: 1 }).0, Status::QuotaExceeded);
    // Collection belongs to the registering principal only: another Cell, or a restarted
    // VFS (new generation), is refused.
    let vfs2 = Principal { cell_id: VFS.cell_id, generation: 2 };
    assert_eq!(t.collect_as(OTHER, TID, first).0, Status::Denied);
    assert_eq!(t.collect_as(vfs2, VFS_TID, first).0, Status::Denied);
    // 32 system-wide: seven more VFS incarnations fill the table.
    for generation in 2..=8u64 {
        let vfs = Principal { cell_id: VFS.cell_id, generation };
        for n in 0..4 {
            let id = generation * 10 + n;
            let status = t.h.call_as(vfs, VFS_TID, challenge(id), 29, &descriptor(challenge(id), Principal { cell_id: 1000 + id, generation: 1 }, now_ns() + 30 * SECOND)).0;
            assert_eq!(status, Status::Ok, "registrar {generation} #{n}");
        }
    }
    assert_eq!(t.h.sessions.challenges.len(), tr::MAX_TOTAL);
    let vfs9 = Principal { cell_id: VFS.cell_id, generation: 9 };
    assert_eq!(t.h.call_as(vfs9, VFS_TID, challenge(999), 29, &descriptor(challenge(999), OTHER, now_ns() + 30 * SECOND)).0, Status::QuotaExceeded);
    // Once decided, the target may be challenged again; collecting the final record frees a slot.
    t.begin_armed(first);
    assert_eq!(t.submit(&[text("no"), enter()]), Status::Ok);
    assert_eq!(t.collect(first).1.map(|d| d.decision), Some(tr::DECISION_REJECT));
    assert_eq!(t.h.call_as(vfs9, VFS_TID, challenge(999), 29, &descriptor(challenge(999), SSH, now_ns() + 30 * SECOND)).0, Status::Ok);
    // Uncollected records of dead incarnations are released after their deadline.
    advance(91_000);
    t.h.sessions.maintain();
    assert!(t.h.sessions.challenges.is_empty());
}

#[test]
fn serial_sessions_have_no_trusted_answer_path() {
    // The physical UART (and any Serial self-attachment) is a byte stream: an unframed
    // paste of `yes\r` would decode as typing, so confirmation is unsupported there.
    let mut t = Trust::serial();
    let confirmation = t.confirmation(1);
    assert_eq!(t.begin(confirmation), Status::Unsupported);
    let s = t.h.session(t.a.session);
    assert!(s.capture.is_none() && s.notices.is_empty());
    assert_eq!(t.collect(confirmation).1.unwrap().decision, tr::DECISION_PENDING);
}

/// Every envelope input sends for one request (deferred answers included), as
/// `(selector, request id, status)`.
fn send(t: &mut Trust, p: Principal, lane: Ref, selector: u16, body: &[u8]) -> Vec<(u16, u64, Status)> {
    let id = t.h.sessions.fences.iter().find(|f| f.caller == p && f.operation == lane).map_or(1, |f| f.next);
    let mut record = vec![0u8; HEADER_LEN + body.len()];
    RequestHeaderV1 { selector, total_len: record.len() as u32, request_id: id }.encode(&mut record[..HEADER_LEN]).unwrap();
    record[HEADER_LEN..].copy_from_slice(body);
    fake::SENT.with(|s| s.borrow_mut().clear());
    let identity = CallerIdentity { flags: 0, cell_id: p.cell_id, generation: p.generation, sender_tid: TID as u64 };
    t.h.sessions.request(&record, Some(identity), TID);
    fake::SENT.with(|s| s.borrow_mut().drain(..).collect::<Vec<_>>()).iter().map(|envelope| {
        let api::ipc::InputResponse::SessionV1(result) = api::ipc::decode(envelope).unwrap() else { panic!("wrong response") };
        let header = ResultHeaderV1::decode(result).unwrap();
        (header.selector, header.request_id, header.status)
    }).collect()
}

#[test]
fn begin_discards_input_queued_for_the_child_and_answers_its_poll_empty() {
    // Typed input still queued for the child at Begin never reaches it.
    let mut t = Trust::new();
    assert_eq!(t.submit(&[text("rm -rf"), enter()]), Status::Ok);
    assert_eq!(t.h.session(t.a.session).typed.len(), 2);
    let confirmation = t.confirmation(1);
    t.begin_armed(confirmation);
    assert!(t.h.session(t.a.session).typed.is_empty());
    assert_eq!(t.submit(&[text("yes"), enter()]), Status::Ok);
    t.ack();
    assert!(t.h.session(t.a.session).typed.is_empty());

    // A poll the child left waiting is answered (empty) in the same step that queues the prompt.
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    fake::IPC_TOKEN.with(|token| token.set(7));
    let mut poll = Vec::new();
    poll.extend_from_slice(&t.a.session.to_bytes()); poll.extend_from_slice(&t.lease.to_bytes()); poll.extend_from_slice(&0u64.to_le_bytes());
    poll.extend_from_slice(&64u16.to_le_bytes()); poll.extend_from_slice(&0u16.to_le_bytes()); poll.extend_from_slice(&0u32.to_le_bytes()); poll.extend_from_slice(&u64::MAX.to_le_bytes());
    assert!(send(&mut t, SSH, C_OP, 9, &poll).is_empty(), "the poll waits");
    assert!(t.h.session(t.a.session).poll.is_some());
    let mut begin = [0u8; BeginTrustConfirmationV1::LEN];
    BeginTrustConfirmationV1 { session: t.a.session, lease: t.lease, epoch: t.epoch, confirmation }.encode(&mut begin).unwrap();
    let sent = send(&mut t, SSH, C_OP, 30, &begin);
    assert_eq!(sent.iter().map(|e| (e.0, e.2)).collect::<Vec<_>>(), [(9, Status::WouldBlock), (30, Status::Ok)]);
    assert!(t.h.session(t.a.session).poll.is_none());
}

#[test]
fn nothing_started_during_the_quarantine_survives_its_release() {
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    t.begin_armed(confirmation);
    assert_eq!(t.submit(&[text("ye")]), Status::Ok);
    assert_eq!(t.submit(&[text("s"), enter()]), Status::Ok);
    // Decided, prompt still up: a paste is refused and stages nothing.
    assert_eq!(t.prepare_paste(), Status::Cancelled);
    assert!(t.h.session(t.a.session).paste.is_none());
    t.ack();
    let s = t.h.session(t.a.session);
    assert!(s.capture.is_none() && s.paste.is_none() && s.physical_paste.is_none() && s.typed.is_empty());
    // After release a fresh paste is staged normally, for the child.
    assert_eq!(t.prepare_paste(), Status::Ok);
    assert!(t.h.session(t.a.session).paste.is_some());
    assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_ACCEPT));
}

#[test]
fn a_lost_controller_ring_releases_the_quarantine_at_once() {
    fn breaks(t: &mut Trust) { model(|m| m.broken.push(controller_control())); t.h.sessions.maintain(); }
    // While arming: the prompt can never be drawn. Cancelled, released, routing back.
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    assert_eq!(t.begin(confirmation), Status::Ok);
    breaks(&mut t);
    assert!(t.h.session(t.a.session).capture.is_none() && t.h.session(t.a.session).notices.is_empty());
    assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_CANCELLED));
    assert_eq!(t.submit(&[text("a")]), Status::Ok);
    assert_eq!(t.h.session(t.a.session).typed.len(), 1);

    // Decided and not yet collected: the end can never be acknowledged. The decision stays.
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    t.begin_armed(confirmation);
    assert_eq!(t.submit(&[text("no"), enter()]), Status::Ok);
    breaks(&mut t);
    assert!(t.h.session(t.a.session).capture.is_none());
    assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_REJECT));

    // Decided and already collected by VFS (the record is gone): still released.
    let mut t = Trust::new();
    let confirmation = t.confirmation(1);
    t.begin_armed(confirmation);
    assert_eq!(t.submit(&[text("yes"), enter()]), Status::Ok);
    model(|m| m.broken.push(controller_control()));
    assert_eq!(t.collect(confirmation).1.map(|d| d.decision), Some(tr::DECISION_ACCEPT));
    assert!(t.h.sessions.challenges.is_empty());
    t.h.sessions.maintain();
    assert!(t.h.session(t.a.session).capture.is_none());
    assert_eq!(t.collect(confirmation).0, Status::Stale);
    assert_eq!(t.submit(&[text("a")]), Status::Ok);
    assert_eq!(t.h.session(t.a.session).typed.len(), 1);
}
