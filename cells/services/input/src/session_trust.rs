// SPDX-License-Identifier: MPL-2.0
//! Owner-run first-trust confirmation: SessionV1 selectors 29–31 (D79-11, D79-13).
//!
//! VFS registers an immutable descriptor (29). The descriptor's target, as the live
//! committed foreground child with its exact lease and epoch, begins the confirmation
//! (30) on a session whose frontend submits framed events (a sel-25 frontend; Serial
//! sessions, including the physical UART, are byte streams and answer `Unsupported`).
//! Begin discards the input still queued for the child and answers its waiting poll;
//! from then on the session is *quarantined*: none of its typed, raw or paste input
//! reaches the child until the confirmation is over. Only a fresh typed `yes` followed by
//! Enter accepts; any other line, Escape, Ctrl-C, any paste or any byte without key/text
//! provenance (SubmitRaw) rejects; the deadline expires it; frontend loss, end of input,
//! foreground restoration and session close cancel it. Every release wipes the capture
//! line, paste staging and the serial decoder. Only the registering VFS principal
//! collects the decision (31), final exactly once.
//!
//! Input has no output path of its own, so the prompt is drawn by the session
//! controller: `TrustPrompt` (the descriptor's host/port/algorithm/fingerprint) and
//! `TrustPromptEnd` are owner notices written to the controller's own control ring
//! (`baseline_writers[1]`), never to the child's. The controller acknowledges each one
//! after drawing it (AcknowledgeControl, controller-only while a foreground child is the
//! reader). The phases of a capture are therefore:
//! - `Arming`: the prompt may not be on screen yet. Input is dropped and never counts.
//! - `Armed`: the controller drew the prompt. Typed key/text events build the answer.
//! - `Ending`: the decision is recorded; the prompt may still be on screen. Input is
//!   dropped until the controller acknowledges `TrustPromptEnd`, then routing returns to
//!   the child untouched (modes, lease and epoch never change).
//!
//! Restoration, close, frontend loss, end of input and a failed write to the controller's
//! ring end the quarantine at once (and cancel an undecided capture; a recorded decision
//! is kept, whether or not VFS already collected it). The kernel flushes the controller's
//! ring when it restores the controller (RestoreSessionInput), so restore and close drop
//! unsent notices; the controller ends its prompt on restoration itself.
//!
//! Accepted limitation: the attached frontend holder is the session's input authority
//! (D02), so whatever it submits as typed key/text events is, as for every other
//! keystroke, the user's typing. A holder that declares itself a GUI frontend and
//! synthesizes `yes` and Enter therefore answers the prompt; the confirmation is only as
//! trustworthy as the frontend that the session controller attached. Input cannot tell
//! synthesized from physical keystrokes on that path and does not try to.
//!
//! Known gap: bytes input wrote to the child's kernel stdin ring before Begin (raw-mode
//! routing or SubmitRaw) stay readable by the child during the prompt. Input cannot see
//! that ring's occupancy: kernel InspectEndpoint reports only holder, operation, rights,
//! state and direction (`kernel/src/task/cell_session.rs:873-881`), and the pipe layer
//! exposes reader/writer counts, not buffered bytes (`kernel/src/task/pipe.rs:462-464`).
use super::*;
use api::cell_session::trust::{self as tr, BeginTrustConfirmationV1, TrustChallengeV1, TrustDecisionV1, TrustPromptV1};
use api::services::session::{event_kind, key_code, modifiers};

/// An uncollected final record is released this long after its deadline.
const RETAIN_NS: u64 = tr::EXPIRY_MAX_NS;
/// Owner notices queued per session for the controller's control ring.
const NOTICE_LIMIT: usize = 8;

pub(super) struct Challenge {
    confirmation: Ref, challenge: Ref, registrar: Principal, target: Principal, expiry: u64,
    port: u16, host: Vec<u8>, algorithm: Vec<u8>, fingerprint: Vec<u8>, digest: [u8; 32],
    session: Ref, epoch: u64, begun: bool, decision: u32,
}
impl Challenge {
    fn prompt(&self) -> Result<TrustPromptV1<'_>, Status> {
        fn text(bytes: &[u8]) -> Result<&str, Status> { core::str::from_utf8(bytes).map_err(|_| Status::Malformed) }
        Ok(TrustPromptV1 { port: self.port, host: text(&self.host)?, algorithm: text(&self.algorithm)?, fingerprint: text(&self.fingerprint)? })
    }
}

/// Capture phase; the `u64` is the sequence of the notice the controller must acknowledge.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase { Arming(u64), Armed, Ending(u64) }

/// The captured line. At most eight bytes are kept: anything longer can never be `yes`.
pub(super) struct Capture { confirmation: Ref, phase: Phase, line: [u8; 8], len: usize, overflow: bool }
impl Capture { fn wipe(&mut self) { ostd::session::zeroize(&mut self.line); self.len = 0; } }
impl Drop for Capture { fn drop(&mut self) { self.wipe(); } }

/// How a capture ends.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum End {
    /// Tell the controller and keep the session quarantined until it acknowledges.
    Await,
    /// Tell the controller and release at once (frontend loss, end of input).
    Release,
    /// Release at once without a notice (restore, close: the kernel flushes the ring).
    Silent,
}

/// One captured input item.
enum Typed<'a> { Text(&'a [u8]), Enter, Erase, Refuse, Ignore }

/// Never part of `yes`: stands in for a key that is not plain text.
const OTHER: &[u8] = &[0];

/// Does a live challenge keep the request lane `(caller, operation)` fenced? The lane of
/// RegisterTrustChallenge is the challenge Ref, the lane of CollectTrustDecision the
/// confirmation Ref.
pub(super) fn challenge_lane(challenges: &[Challenge], caller: Principal, operation: Ref) -> bool {
    challenges.iter().any(|c| c.registrar == caller && (c.challenge == operation || c.confirmation == operation))
}

/// A validated Key frame. Plain characters arrive as Text, so every other key (Alt/Ctrl
/// chords, navigation, function keys) is never part of `yes`.
fn key_item(payload: &[u8]) -> Typed<'static> {
    let word = |at: usize| u32::from_le_bytes([payload[at], payload[at + 1], payload[at + 2], payload[at + 3]]);
    let (code, mods, pressed) = (word(0), word(4), payload[8]);
    if pressed == 0 { return Typed::Ignore; }
    match code {
        67 | 99 if mods & modifiers::CONTROL != 0 => Typed::Refuse,
        key_code::ENTER => Typed::Enter,
        key_code::BACKSPACE => Typed::Erase,
        key_code::ESCAPE => Typed::Refuse,
        _ => Typed::Text(OTHER),
    }
}

impl Sessions {
    /// Sel 29/31 admission. 29: the attested sender is the live registered VFS task
    /// (as VFS checks live KMS). 31: the principal that registered the challenge.
    pub(super) fn admit_trust(&self, selector: u16, body: &[u8], p: Principal, sender: usize) -> Result<(), Status> {
        if selector == 29 {
            return if sys_lookup_service(api::syscall::service::VFS).is_some_and(|tid| tid == sender) { Ok(()) } else { Err(Status::Denied) };
        }
        let confirmation = Ref::from_bytes(body)?;
        let c = self.challenges.iter().find(|c| c.confirmation == confirmation).ok_or(Status::Stale)?;
        if c.registrar == p { Ok(()) } else { Err(Status::Denied) }
    }

    pub(super) fn trust_operation(&mut self, selector: u16, body: &[u8], p: Principal, w: &mut Writer<'_>) -> Result<(), Status> {
        match selector {
            29 => self.register_challenge(body, p, w),
            30 => self.begin_confirmation(BeginTrustConfirmationV1::decode(body)?, p),
            31 => self.collect_decision(Ref::from_bytes(body)?, p, w),
            _ => Err(Status::UnknownOperation),
        }
    }

    fn register_challenge(&mut self, body: &[u8], p: Principal, w: &mut Writer<'_>) -> Result<(), Status> {
        let request = TrustChallengeV1::decode(body)?;
        let now = now_ns();
        if request.expiry_ns <= now { return Err(Status::TimedOut); }
        if request.expiry_ns - now > tr::EXPIRY_MAX_NS { return Err(Status::Malformed); }
        // Challenge refs are single-use: a second registration never rebinds one.
        if self.challenges.iter().any(|c| c.registrar == p && c.challenge == request.challenge) { return Err(Status::Denied); }
        // Uncollected records hold memory until collected or released after their deadline
        // (also those of a dead VFS incarnation, which nobody can collect any more); a target
        // has at most one confirmation that can still be decided.
        if self.challenges.len() >= tr::MAX_TOTAL
            || self.challenges.iter().filter(|c| c.registrar == p).count() >= tr::MAX_PER_REGISTRAR
            || self.challenges.iter().filter(|c| c.target == request.target && c.decision == tr::DECISION_PENDING).count() >= tr::MAX_PER_TARGET {
            return Err(Status::QuotaExceeded);
        }
        let copy = |text: &str| -> Result<Vec<u8>, Status> { let mut v = Vec::new(); reserve(&mut v, text.len())?; v.extend_from_slice(text.as_bytes()); Ok(v) };
        let (host, algorithm, fingerprint) = (copy(request.prompt.host)?, copy(request.prompt.algorithm)?, copy(request.prompt.fingerprint)?);
        reserve(&mut self.challenges, 1)?;
        let confirmation = self.reference()?;
        self.challenges.push(Challenge {
            confirmation, challenge: request.challenge, registrar: p, target: request.target, expiry: request.expiry_ns,
            port: request.prompt.port, host, algorithm, fingerprint, digest: ostd::session::sha256(body),
            session: ZERO, epoch: 0, begun: false, decision: tr::DECISION_PENDING,
        });
        w.reference(confirmation)
    }

    fn begin_confirmation(&mut self, request: BeginTrustConfirmationV1, p: Principal) -> Result<(), Status> {
        let i = self.index(request.session)?;
        let n = self.challenges.iter().position(|c| c.confirmation == request.confirmation).ok_or(Status::Stale)?;
        if self.challenges[n].target != p { return Err(Status::Denied); }
        let s = &self.sessions[i];
        let foreground = s.foreground.as_ref().is_some_and(|f| f.committed && !f.restored && f.child.principal == p);
        if !foreground || s.reader != p || s.lease != request.lease { return Err(Status::Denied); }
        if s.frozen { return Err(Status::WouldBlock); }
        if s.epoch != request.epoch { return Err(Status::WrongEpoch); }
        if now_ns() >= self.challenges[n].expiry {
            if self.challenges[n].decision == tr::DECISION_PENDING { self.challenges[n].decision = tr::DECISION_EXPIRED; }
            return Err(Status::TimedOut);
        }
        let c = &self.challenges[n];
        if c.begun || c.decision != tr::DECISION_PENDING { return Err(Status::Stale); }
        // A trusted answer needs a frontend whose input carries key/text/paste provenance
        // (SubmitEvents). Serial sessions (the physical UART or the controller's own
        // self-attachment) are byte streams where an unframed paste is indistinguishable from
        // typing; headless or detached input has no frontend at all.
        if s.attachment.is_none() || s.kind == fe::FRONTEND_KIND_SERIAL || self.serial == Some(request.session) { return Err(Status::Unsupported); }
        if s.capture.is_some() || s.paste.is_some() || s.physical_paste.is_some() || s.physical_paste_discard || s.draining { return Err(Status::WouldBlock); }
        if s.notices.len() + 2 > NOTICE_LIMIT { return Err(Status::QuotaExceeded); }
        let mut payload = [0u8; TrustPromptV1::MAX_LEN];
        let len = c.prompt()?.encode(&mut payload)?;
        let confirmation = c.confirmation;
        self.sessions[i].notices.try_reserve(1).map_err(|_| Status::QuotaExceeded)?;
        let event = self.event(i, event_kind::TRUST_PROMPT, &payload[..len])?;
        // Nothing queued before Begin reaches the child while the prompt is up: its unread
        // input is discarded and an outstanding poll is answered empty, in this same step.
        self.invalidate_poll(i, Status::WouldBlock);
        let s = &mut self.sessions[i];
        s.typed.clear(); zeroize_deque(&mut s.raw);
        let prompt = event.sequence; s.sequence = prompt;
        s.notices.push_back(event);
        let c = &mut self.challenges[n]; c.begun = true; c.session = request.session; c.epoch = request.epoch;
        self.sessions[i].capture = Some(Capture { confirmation, phase: Phase::Arming(prompt), line: [0; 8], len: 0, overflow: false });
        Ok(())
    }

    fn collect_decision(&mut self, confirmation: Ref, p: Principal, w: &mut Writer<'_>) -> Result<(), Status> {
        let n = self.challenges.iter().position(|c| c.confirmation == confirmation).ok_or(Status::Stale)?;
        if self.challenges[n].registrar != p { return Err(Status::Denied); }
        if self.challenges[n].decision == tr::DECISION_PENDING && now_ns() >= self.challenges[n].expiry {
            if let Some(i) = self.capturing(confirmation) { self.end_capture(i, tr::DECISION_EXPIRED, End::Await); }
            self.challenges[n].decision = tr::DECISION_EXPIRED;
        }
        let c = &self.challenges[n];
        let record = TrustDecisionV1 { decision: c.decision, challenge: c.challenge, target: c.target, session: c.session, epoch: c.epoch, descriptor_sha256: c.digest };
        let mut bytes = [0u8; TrustDecisionV1::LEN]; record.encode(&mut bytes)?; w.bytes(&bytes)?;
        // A final decision is handed out once; the retained outcome still answers an exact replay.
        if record.is_final() { self.challenges.swap_remove(n); }
        Ok(())
    }

    fn capturing(&self, confirmation: Ref) -> Option<usize> {
        self.sessions.iter().position(|s| !s.closed && s.capture.as_ref().is_some_and(|c| c.confirmation == confirmation))
    }

    /// Drop matching records; a capture of a dropped record ends cancelled.
    fn release_challenges(&mut self, dead: impl Fn(&Challenge) -> bool) {
        let mut n = 0;
        while n < self.challenges.len() {
            if !dead(&self.challenges[n]) { n += 1; continue; }
            if let Some(i) = self.capturing(self.challenges[n].confirmation) { self.end_capture(i, tr::DECISION_CANCELLED, End::Release); }
            self.challenges.swap_remove(n);
        }
    }

    /// Queue one owner notice for the controller's control ring; returns its sequence.
    fn notice(&mut self, i: usize, kind: u16, payload: &[u8]) -> Result<u64, Status> {
        if self.sessions[i].notices.len() >= NOTICE_LIMIT { return Err(Status::QuotaExceeded); }
        self.sessions[i].notices.try_reserve(1).map_err(|_| Status::QuotaExceeded)?;
        let event = self.event(i, kind, payload)?; let sequence = event.sequence;
        let s = &mut self.sessions[i]; s.sequence = sequence; s.notices.push_back(event);
        Ok(sequence)
    }

    /// End the session's capture with `decision` unless the record already has a final one.
    /// A capture already `Ending` only leaves quarantine on an escape path (`Release`/`Silent`).
    pub(super) fn end_capture(&mut self, i: usize, decision: u32, end: End) {
        let Some(capture) = self.sessions[i].capture.as_mut() else { return; };
        if let Phase::Ending(_) = capture.phase {
            if end != End::Await { self.release_quarantine(i); }
            return;
        }
        capture.wipe();
        let confirmation = capture.confirmation;
        let mut accepted = false;
        if let Some(c) = self.challenges.iter_mut().find(|c| c.confirmation == confirmation) {
            if c.decision == tr::DECISION_PENDING { c.decision = decision; }
            accepted = c.decision == tr::DECISION_ACCEPT;
        }
        let ended = if end == End::Silent { None } else { self.notice(i, event_kind::TRUST_PROMPT_END, &u32::from(accepted).to_le_bytes()).ok() };
        match (end, ended) {
            // Bytes typed while the prompt may still be on screen are dropped until the
            // controller confirms it ended the prompt. Begin reserved the notice slot.
            (End::Await, Some(sequence)) => { if let Some(c) = self.sessions[i].capture.as_mut() { c.phase = Phase::Ending(sequence); } }
            _ => self.release_quarantine(i),
        }
    }

    /// Routing returns to the child. Every incremental input state that could still carry
    /// bytes typed during the quarantine is wiped first, so no partial sequence (escape,
    /// UTF-8, paste staging) completes afterwards: the capture line, any paste staging and
    /// the serial decoder.
    fn release_quarantine(&mut self, i: usize) {
        let s = &mut self.sessions[i];
        s.capture = None;
        s.paste = None; s.physical_paste = None; s.physical_paste_complete = false; s.physical_paste_discard = false;
        s.decoder = api::services::session::SerialDecoder::new();
    }

    /// AcknowledgeControl from the controller while a foreground child is the reader: the
    /// controller drew (or ended) the prompt of every delivered owner notice up to `through`.
    pub(super) fn acknowledge_notices(&mut self, i: usize, lease: Ref, epoch: u64, through: u64) -> Result<(), Status> {
        let s = &self.sessions[i];
        if s.frozen { return Err(Status::WouldBlock); }
        if lease != s.baseline_lease { return Err(Status::Denied); }
        if epoch != s.epoch { return Err(Status::WrongEpoch); }
        // Only notices fully written to the controller's ring can have been drawn.
        if through > s.sequence || s.notices.iter().any(|e| e.sequence <= through) { return Err(Status::Malformed); }
        let s = &mut self.sessions[i];
        let Some(capture) = s.capture.as_mut() else { return Ok(()); };
        match capture.phase {
            Phase::Arming(sequence) if through >= sequence => capture.phase = Phase::Armed,
            Phase::Ending(sequence) if through >= sequence => self.release_quarantine(i),
            _ => {}
        }
        Ok(())
    }

    /// Is the capture accepting an answer now? A passed deadline first ends it expired.
    /// While arming or ending (or with no capture) the caller drops the input.
    fn armed(&mut self, i: usize) -> bool {
        let Some(capture) = self.sessions[i].capture.as_ref() else { return false; };
        if let Phase::Ending(_) = capture.phase { return false; }
        let (confirmation, phase) = (capture.confirmation, capture.phase);
        let now = now_ns();
        if self.challenges.iter().find(|c| c.confirmation == confirmation).is_none_or(|c| now >= c.expiry) {
            self.end_capture(i, tr::DECISION_EXPIRED, End::Await);
            return false;
        }
        phase == Phase::Armed
    }

    /// Feed one item to an armed capture; true once it ended.
    fn capture_item(&mut self, i: usize, item: Typed<'_>) -> bool {
        let Some(c) = self.sessions[i].capture.as_mut() else { return true; };
        let decision = match item {
            Typed::Ignore => return false,
            Typed::Text(bytes) => {
                for &byte in bytes { if c.len < c.line.len() { c.line[c.len] = byte; c.len += 1; } else { c.overflow = true; } }
                return false;
            }
            Typed::Erase => { if !c.overflow && c.len > 0 { c.len -= 1; c.line[c.len] = 0; } return false; }
            Typed::Enter if !c.overflow && &c.line[..c.len] == b"yes" => tr::DECISION_ACCEPT,
            Typed::Enter | Typed::Refuse => tr::DECISION_REJECT,
        };
        self.end_capture(i, decision, End::Await);
        true
    }

    /// SubmitEvents / physical keys / decoded UART during a capture: the batch is validated
    /// exactly as routed input, then consumed here. It answers only while armed; whatever
    /// follows the deciding item, and everything while arming or ending, is dropped.
    pub(super) fn capture_events(&mut self, i: usize, bytes: &[u8], count: usize) -> Result<(), Status> {
        let mut offset = 0;
        for _ in 0..count {
            let frame = bytes.get(offset..offset + 24).ok_or(Status::Malformed)?;
            let kind = u16::from_le_bytes([frame[16], frame[17]]);
            let length = u16::from_le_bytes([frame[18], frame[19]]) as usize;
            if frame[..16] != [0; 16] || frame[20..24] != [0; 4] || !(1..=2).contains(&kind) { return Err(Status::Malformed); }
            let payload = bytes.get(offset + 24..offset + 24 + length).ok_or(Status::Malformed)?;
            api::services::session::validate_event(kind, payload)?;
            if kind == 2 { api::services::session::validate_text(payload)?; }
            offset += 24 + length;
        }
        if offset != bytes.len() { return Err(Status::Malformed); }
        if !self.armed(i) { return Ok(()); }
        let mut offset = 0;
        for _ in 0..count {
            let kind = u16::from_le_bytes([bytes[offset + 16], bytes[offset + 17]]);
            let length = u16::from_le_bytes([bytes[offset + 18], bytes[offset + 19]]) as usize;
            let payload = &bytes[offset + 24..offset + 24 + length];
            offset += 24 + length;
            let item = if kind == 2 { Typed::Text(payload) } else { key_item(payload) };
            if self.capture_item(i, item) { break; }
        }
        Ok(())
    }

    /// SubmitRaw during a capture. Raw bytes carry no key/paste provenance, so they never
    /// answer: while armed they reject, otherwise they are dropped. Never routed.
    pub(super) fn capture_raw(&mut self, i: usize) {
        if self.armed(i) { self.end_capture(i, tr::DECISION_REJECT, End::Await); }
    }

    /// Any paste during a capture is refused; while armed it also rejects the capture.
    pub(super) fn refuse_paste(&mut self, i: usize) -> bool {
        if self.sessions[i].capture.is_none() { return false; }
        if self.armed(i) { self.end_capture(i, tr::DECISION_REJECT, End::Await); }
        true
    }

    /// Deadlines: expire captures and pending records; release uncollected final records.
    pub(super) fn maintain_trust(&mut self) {
        if self.challenges.is_empty() { return; }
        for i in 0..self.sessions.len() {
            if !self.sessions[i].closed && self.sessions[i].capture.is_some() { self.armed(i); }
        }
        let now = now_ns();
        for c in self.challenges.iter_mut().filter(|c| c.decision == tr::DECISION_PENDING && now >= c.expiry) { c.decision = tr::DECISION_EXPIRED; }
        self.release_challenges(|c| now >= c.expiry.saturating_add(RETAIN_NS));
    }

    /// Write queued owner notices to the controller's own control ring.
    pub(super) fn drain_notices(&mut self, i: usize) {
        for _ in 0..NOTICE_LIMIT {
            let s = &self.sessions[i];
            if s.closed { return; }
            let Some(event) = s.notices.front() else { return; };
            let mut bytes = [0u8; 512]; let count = (event.bytes.len() - event.sent).min(512);
            bytes[..count].copy_from_slice(&event.bytes[event.sent..event.sent + count]);
            let endpoint = s.baseline_writers[1];
            self.kernel_operation = s.operation;
            match self.write_stream(endpoint, &bytes[..count]) {
                Ok(written) => {
                    let s = &mut self.sessions[i];
                    if let Some(e) = s.notices.front_mut() { e.sent += written; if e.sent == e.bytes.len() { s.notices.pop_front(); } }
                }
                Err(Status::WouldBlock) => return,
                // The controller's ring is gone: no prompt can be drawn or acknowledged, so a
                // capture can neither arm nor end. Cancel it (an undecided one) and release now.
                Err(_) => {
                    self.sessions[i].notices.clear();
                    self.end_capture(i, tr::DECISION_CANCELLED, End::Silent);
                    return;
                }
            }
        }
    }
}
