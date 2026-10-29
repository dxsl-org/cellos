// SPDX-License-Identifier: Apache-2.0
//! SessionV1 first-trust confirmation records (selectors 29–31; D79-11, D79-13).
//!
//! VFS registers one immutable host-trust descriptor with the input owner
//! (`RegisterTrustChallenge`, admitted only from the live registered VFS service).
//! The attested foreground child of the descriptor's `target` principal begins the
//! owner-run confirmation with its exact session, lease and epoch
//! (`BeginTrustConfirmation`). From then on input itself captures that session's
//! typed/raw/paste input (it never reaches the child) and asks the session controller
//! to render a fixed prompt built only from the descriptor (`TrustPrompt` /
//! `TrustPromptEnd` control events, see `api::services::session::event_kind`). Only the
//! registering VFS principal collects the decision (`CollectTrustDecision`).
//!
//! Every record here is a request or result *body* (the 24-byte header is separate).
//! Decoders require the exact length; text fields are length-prefixed (`u16`) graphic
//! ASCII (0x21..=0x7e), so no control byte can ever reach the rendered prompt.

use super::{InputOp, ObjectRefV1, PrincipalV1, Reader, Status, Writer};

pub const SEL_REGISTER_TRUST_CHALLENGE: u16 = InputOp::RegisterTrustChallenge as u16;
pub const SEL_BEGIN_TRUST_CONFIRMATION: u16 = InputOp::BeginTrustConfirmation as u16;
pub const SEL_COLLECT_TRUST_DECISION: u16 = InputOp::CollectTrustDecision as u16;

/// Configured host identifier bound (abi-vfs-telemetry §4).
pub const HOST_MAX: usize = 253;
pub const ALGORITHM_MAX: usize = 64;
/// Displayed fingerprint, e.g. `SHA256:<base64>`.
pub const FINGERPRINT_MAX: usize = 64;
/// `expiry_ns` is absolute receiver-monotonic time, at most this far ahead of the
/// owner's clock at registration. It is never extended.
pub const EXPIRY_MAX_NS: u64 = 60_000_000_000;

/// Undecided (pending) challenges per confirming target principal.
pub const MAX_PER_TARGET: usize = 1;
/// Uncollected challenges per registering principal.
pub const MAX_PER_REGISTRAR: usize = 4;
/// Uncollected challenges system-wide. An uncollected record is released by the owner
/// [`EXPIRY_MAX_NS`] after its deadline.
pub const MAX_TOTAL: usize = 32;

/// `CollectTrustDecision` decisions. Only `PENDING` is not final.
pub const DECISION_PENDING: u32 = 0;
pub const DECISION_ACCEPT: u32 = 1;
pub const DECISION_REJECT: u32 = 2;
pub const DECISION_EXPIRED: u32 = 3;
pub const DECISION_CANCELLED: u32 = 4;

fn text(bytes: &[u8], max: usize) -> Result<&str, Status> {
    if bytes.is_empty() || bytes.len() > max || !bytes.iter().all(u8::is_ascii_graphic) { return Err(Status::Malformed); }
    core::str::from_utf8(bytes).map_err(|_| Status::Malformed)
}
fn field<'a>(r: &mut Reader<'a>, max: usize) -> Result<&'a str, Status> {
    let len = r.u16()? as usize;
    text(r.take(len)?, max)
}
fn put(w: &mut Writer<'_>, value: &str, max: usize) -> Result<(), Status> {
    text(value.as_bytes(), max)?;
    w.u16(value.len() as u16)?; w.bytes(value.as_bytes())
}

/// The displayed part of a descriptor and the exact payload of the `TrustPrompt`
/// control event: `port:u16, host_len:u16, host, algorithm_len:u16, algorithm,
/// fingerprint_len:u16, fingerprint`. `port` is nonzero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustPromptV1<'a> { pub port: u16, pub host: &'a str, pub algorithm: &'a str, pub fingerprint: &'a str }
impl<'a> TrustPromptV1<'a> {
    pub const MAX_LEN: usize = 2 + 2 + HOST_MAX + 2 + ALGORITHM_MAX + 2 + FINGERPRINT_MAX;
    pub fn len(&self) -> usize { 8 + self.host.len() + self.algorithm.len() + self.fingerprint.len() }
    pub fn is_empty(&self) -> bool { false }
    fn write(&self, w: &mut Writer<'_>) -> Result<(), Status> {
        if self.port == 0 { return Err(Status::Malformed); }
        w.u16(self.port)?; put(w, self.host, HOST_MAX)?; put(w, self.algorithm, ALGORITHM_MAX)?; put(w, self.fingerprint, FINGERPRINT_MAX)
    }
    fn read(r: &mut Reader<'a>) -> Result<Self, Status> {
        let port = r.u16()?; if port == 0 { return Err(Status::Malformed); }
        Ok(Self { port, host: field(r, HOST_MAX)?, algorithm: field(r, ALGORITHM_MAX)?, fingerprint: field(r, FINGERPRINT_MAX)? })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Status> {
        let mut w = Writer::new(out); self.write(&mut w)?; Ok(w.position())
    }
    pub fn decode(payload: &'a [u8]) -> Result<Self, Status> {
        let mut r = Reader::new(payload); let value = Self::read(&mut r)?; r.finish()?; Ok(value)
    }
}

/// Selector 29 request (the canonical descriptor bytes): `challenge:Ref` (VFS-issued,
/// nonzero), `target:Principal` (the client Cell that will confirm), `expiry_ns:u64`,
/// then [`TrustPromptV1`]. Result: `confirmation:Ref` (input-issued, unique).
/// `CollectTrustDecision` returns the SHA-256 of exactly these bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustChallengeV1<'a> { pub challenge: ObjectRefV1, pub target: PrincipalV1, pub expiry_ns: u64, pub prompt: TrustPromptV1<'a> }
impl<'a> TrustChallengeV1<'a> {
    pub const MAX_LEN: usize = 40 + TrustPromptV1::MAX_LEN;
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Status> {
        if self.challenge.is_zero() || self.target.generation == 0 || self.expiry_ns == 0 { return Err(Status::Malformed); }
        let mut w = Writer::new(out);
        w.reference(self.challenge)?; w.principal(self.target)?; w.u64(self.expiry_ns)?; self.prompt.write(&mut w)?;
        Ok(w.position())
    }
    pub fn decode(body: &'a [u8]) -> Result<Self, Status> {
        let mut r = Reader::new(body);
        let value = Self { challenge: r.reference()?, target: r.principal()?, expiry_ns: r.u64()?, prompt: TrustPromptV1::read(&mut r)? };
        r.finish()?;
        if value.challenge.is_zero() || value.target.generation == 0 || value.expiry_ns == 0 { return Err(Status::Malformed); }
        Ok(value)
    }
}

/// Selector 30 request: `session:Ref, lease:Ref, epoch:u64, confirmation:Ref`.
/// Result: empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeginTrustConfirmationV1 { pub session: ObjectRefV1, pub lease: ObjectRefV1, pub epoch: u64, pub confirmation: ObjectRefV1 }
impl BeginTrustConfirmationV1 {
    pub const LEN: usize = 56;
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Status> {
        let mut w = Writer::new(out.get_mut(..Self::LEN).ok_or(Status::BufferTooSmall)?);
        w.reference(self.session)?; w.reference(self.lease)?; w.u64(self.epoch)?; w.reference(self.confirmation)?;
        Ok(Self::LEN)
    }
    pub fn decode(body: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(body);
        let value = Self { session: r.reference()?, lease: r.reference()?, epoch: r.u64()?, confirmation: r.reference()? };
        r.finish()?;
        Ok(value)
    }
}

/// Selector 31 result (request: `confirmation:Ref`): `decision:u32, challenge:Ref,
/// target:Principal, session:Ref, epoch:u64, descriptor_sha256:[u8;32]`. `session` and
/// `epoch` are the confirmation's binding (zero before `BeginTrustConfirmation`).
/// A final decision is returned once; a later collect is `Stale`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustDecisionV1 {
    pub decision: u32, pub challenge: ObjectRefV1, pub target: PrincipalV1,
    pub session: ObjectRefV1, pub epoch: u64, pub descriptor_sha256: [u8; 32],
}
impl TrustDecisionV1 {
    pub const LEN: usize = 92;
    pub fn is_final(&self) -> bool { self.decision != DECISION_PENDING }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Status> {
        if self.decision > DECISION_CANCELLED { return Err(Status::Malformed); }
        let mut w = Writer::new(out.get_mut(..Self::LEN).ok_or(Status::BufferTooSmall)?);
        w.u32(self.decision)?; w.reference(self.challenge)?; w.principal(self.target)?;
        w.reference(self.session)?; w.u64(self.epoch)?; w.bytes(&self.descriptor_sha256)?;
        Ok(Self::LEN)
    }
    pub fn decode(body: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(body);
        let decision = r.u32()?;
        let value = Self {
            decision, challenge: r.reference()?, target: r.principal()?, session: r.reference()?, epoch: r.u64()?,
            descriptor_sha256: r.take(32)?.try_into().map_err(|_| Status::Malformed)?,
        };
        r.finish()?;
        if decision > DECISION_CANCELLED { return Err(Status::Malformed); }
        Ok(value)
    }
}

/// `TrustPromptEnd` control payload: `accepted:u32` (0 or 1).
pub const PROMPT_END_LEN: usize = 4;
pub fn decode_prompt_end(payload: &[u8]) -> Result<bool, Status> {
    let mut r = Reader::new(payload);
    let accepted = r.u32()?; r.finish()?;
    match accepted { 0 => Ok(false), 1 => Ok(true), _ => Err(Status::Malformed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROMPT: TrustPromptV1<'static> = TrustPromptV1 { port: 22, host: "gateway", algorithm: "ssh-ed25519", fingerprint: "SHA256:abc" };

    #[test]
    fn descriptor_rejects_control_bytes_spaces_and_bounds_before_any_byte_is_trusted() {
        let mut out = [0u8; TrustChallengeV1::MAX_LEN];
        let challenge = TrustChallengeV1 { challenge: ObjectRefV1 { id: 1, generation: 1 }, target: PrincipalV1 { cell_id: 7, generation: 2 }, expiry_ns: 9, prompt: PROMPT };
        let len = challenge.encode(&mut out).unwrap();
        assert_eq!(TrustChallengeV1::decode(&out[..len]), Ok(challenge));
        // A trailing byte, a truncated field or a changed length prefix never decodes.
        assert_eq!(TrustChallengeV1::decode(&out[..len - 1]), Err(Status::Malformed));
        let mut long = out; long[len] = b'x';
        assert_eq!(TrustChallengeV1::decode(&long[..len + 1]), Err(Status::Malformed));
        // Every text field refuses ESC, CR/LF, space and DEL, and empty or oversized values.
        for bad in ["gate\u{1b}[2J", "gate\r\nway", "gate way", "gate\u{7f}", ""] {
            for slot in 0..3 {
                let mut prompt = PROMPT;
                match slot { 0 => prompt.host = bad, 1 => prompt.algorithm = bad, _ => prompt.fingerprint = bad }
                assert_eq!(TrustChallengeV1 { prompt, ..challenge }.encode(&mut out), Err(Status::Malformed), "{bad:?} in {slot}");
            }
        }
        let host = "a".repeat(HOST_MAX + 1);
        assert_eq!(TrustChallengeV1 { prompt: TrustPromptV1 { host: &host, ..PROMPT }, ..challenge }.encode(&mut out), Err(Status::Malformed));
        assert_eq!(TrustChallengeV1 { prompt: TrustPromptV1 { port: 0, ..PROMPT }, ..challenge }.encode(&mut out), Err(Status::Malformed));
        // A forged wire record with a control byte inside an in-bounds field is refused too.
        let mut forged = out; let len = challenge.encode(&mut forged).unwrap();
        let at = 40 + 2 + 2; forged[at] = 0x1b;
        assert_eq!(TrustChallengeV1::decode(&forged[..len]), Err(Status::Malformed));
    }
}
