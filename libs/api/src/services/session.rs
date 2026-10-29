// SPDX-License-Identifier: Apache-2.0
//! Shared borrowed session records and the frozen cellos-vt1 input namespace.
//! Authentication, sequencing, epoch checks and batch admission belong to owners.

pub use crate::cell_session::{
    ChildRefV1, InputOp, KernelOp, ObjectRefV1, PrincipalV1, Reader,
    RequestHeaderV1, ResultHeaderV1, Status, VfsOp, Writer, HEADER_LEN,
    MAX_SERVICE_RECORD_LEN, TRANSFER_MAX,
};

pub const EVENT_HEADER_LEN: usize = 24;
pub const EVENT_MAX: usize = EVENT_HEADER_LEN + TRANSFER_MAX;
pub const PASTE_MAX: usize = 65536;
pub const ESC_TIMEOUT_NS: u64 = 30_000_000;
pub const CONTROL_MAX: usize = 256;
pub mod event_kind {
    pub const KEY: u16 = 1;
    pub const TEXT: u16 = 2;
    pub const PASTE_BEGIN: u16 = 3;
    pub const PASTE_END: u16 = 4;
    pub const RESIZE: u16 = 5;
    pub const INTERRUPT: u16 = 6;
    pub const EOF: u16 = 7;
    pub const LOST: u16 = 8;
    /// Owner notice to the session *controller* (never the child, never submittable):
    /// render the fixed first-trust prompt. Payload `cell_session::trust::TrustPromptV1`.
    pub const TRUST_PROMPT: u16 = 9;
    /// Owner notice to the controller: the trust capture ended. Payload `accepted:u32` (0/1).
    pub const TRUST_PROMPT_END: u16 = 10;
}
pub mod modifiers {
    pub const SHIFT: u32 = 1;
    pub const CONTROL: u32 = 2;
    pub const ALT: u32 = 4;
    pub const ALL: u32 = SHIFT | CONTROL | ALT;
}
pub mod key_code {
    pub const ENTER: u32 = 0x110001;
    pub const ESCAPE: u32 = 0x110002;
    pub const BACKSPACE: u32 = 0x110003;
    pub const TAB: u32 = 0x110004;
    pub const DELETE: u32 = 0x110005;
    pub const INSERT: u32 = 0x110006;
    pub const UP: u32 = 0x110010;
    pub const DOWN: u32 = 0x110011;
    pub const LEFT: u32 = 0x110012;
    pub const RIGHT: u32 = 0x110013;
    pub const HOME: u32 = 0x110014;
    pub const END: u32 = 0x110015;
    pub const PAGE_UP: u32 = 0x110016;
    pub const PAGE_DOWN: u32 = 0x110017;
    pub const F1: u32 = 0x110020;
    pub const F12: u32 = 0x11002b;
}
pub mod mode {
    pub const RAW: u32 = 1;
    pub const ECHO: u32 = 2;
    pub const CURSOR_VISIBLE: u32 = 4;
    pub const ALTERNATE_SCREEN: u32 = 8;
    pub const BRACKETED_PASTE: u32 = 16;
    pub const ALL: u32 = 31;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventHeaderV1 {
    pub sequence: u64,
    pub epoch: u64,
    pub kind: u16,
    pub payload_len: u16,
    pub flags: u32,
}
impl EventHeaderV1 {
    /// Decode one complete frame, including canonical payload validation.
    pub fn decode(record: &[u8]) -> Result<Self, Status> {
        let mut r = Reader::new(record);
        let header = Self { sequence: r.u64()?, epoch: r.u64()?, kind: r.u16()?, payload_len: r.u16()?, flags: r.u32()? };
        if header.flags != 0 { return Err(Status::InvalidFlags); }
        validate_event(header.kind, r.take(header.payload_len as usize)?)?;
        r.finish()?;
        Ok(header)
    }
    /// Encode the fixed header only; payload is written separately.
    pub fn encode(&self, out: &mut [u8]) -> Result<(), Status> {
        if self.flags != 0 { return Err(Status::InvalidFlags); }
        if self.payload_len as usize > TRANSFER_MAX { return Err(Status::Malformed); }
        if !(event_kind::KEY..=event_kind::TRUST_PROMPT_END).contains(&self.kind) { return Err(Status::UnsupportedInput); }
        let mut w = Writer::new(out.get_mut(..EVENT_HEADER_LEN).ok_or(Status::BufferTooSmall)?);
        w.u64(self.sequence)?; w.u64(self.epoch)?; w.u16(self.kind)?;
        w.u16(self.payload_len)?; w.u32(0)
    }
}

pub fn validate_dimensions(rows: u16, cols: u16) -> Result<(), Status> {
    if rows == 0 || rows > 256 || cols == 0 || cols > 512
        || u32::from(rows).checked_mul(u32::from(cols)).ok_or(Status::Malformed)? > 131072 {
        Err(Status::Malformed)
    } else { Ok(()) }
}
fn valid_key(code: u32) -> bool {
    (code >= 0x20 && char::from_u32(code).is_some() && code != 0x7f)
        || matches!(code, key_code::ENTER | key_code::ESCAPE | key_code::BACKSPACE |
            key_code::TAB | key_code::DELETE | key_code::INSERT | key_code::UP |
            key_code::DOWN | key_code::LEFT | key_code::RIGHT | key_code::HOME |
            key_code::END | key_code::PAGE_UP | key_code::PAGE_DOWN)
        || (key_code::F1..=key_code::F12).contains(&code)
}
/// Validate one normalized event. Owners separately restrict which event kinds
/// a frontend may submit and assign all sequence/epoch values themselves.
pub fn validate_event(kind: u16, payload: &[u8]) -> Result<(), Status> {
    use event_kind::*;
    match kind {
        KEY => {
            let mut r = Reader::new(payload);
            let code = r.u32()?;
            let mods = r.u32()?;
            let pressed = r.u8()?;
            if !valid_key(code) || mods & !modifiers::ALL != 0 { return Err(Status::UnsupportedInput); }
            if pressed > 2 || r.take(7)?.iter().any(|v| *v != 0) { return Err(Status::Malformed); }
            r.finish()?;
            encode_key(code, mods, pressed, &mut [0; 16]).map(|_| ())
        }
        TEXT => {
            if payload.is_empty() || payload.len() > TRANSFER_MAX { return Err(Status::Malformed); }
            validate_paste(payload)
        }
        PASTE_BEGIN | PASTE_END | INTERRUPT | EOF => {
            if payload.is_empty() { Ok(()) } else { Err(Status::Malformed) }
        }
        RESIZE => {
            let mut r = Reader::new(payload);
            validate_dimensions(r.u16()?, r.u16()?)?;
            r.finish()
        }
        LOST => {
            // 1..=3 routine input loss; 4 FrontendLost and 5 FrontendAttached (S5-3 annex §3).
            let mut r = Reader::new(payload);
            let reason = r.u32()?;
            if !(1..=3).contains(&reason) && reason != crate::cell_session::frontend::LOST_FRONTEND
                && reason != crate::cell_session::frontend::FRONTEND_ATTACHED { return Err(Status::Malformed); }
            r.finish()
        }
        TRUST_PROMPT => crate::cell_session::trust::TrustPromptV1::decode(payload).map(|_| ()),
        TRUST_PROMPT_END => crate::cell_session::trust::decode_prompt_end(payload).map(|_| ()),
        _ => Err(Status::UnsupportedInput),
    }
}
/// Printable committed Text outside a paste batch.
pub fn validate_text(bytes: &[u8]) -> Result<(), Status> {
    if bytes.is_empty() || bytes.len() > TRANSFER_MAX { return Err(Status::Malformed); }
    let text = core::str::from_utf8(bytes).map_err(|_| Status::UnsupportedInput)?;
    if text.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}') {
        Err(Status::UnsupportedInput)
    } else { Ok(()) }
}
/// Whole paste admission precedes any PasteBegin/Text/PasteEnd delivery.
pub fn validate_paste(bytes: &[u8]) -> Result<(), Status> {
    if bytes.len() > PASTE_MAX { return Err(Status::QuotaExceeded); }
    let text = core::str::from_utf8(bytes).map_err(|_| Status::UnsupportedInput)?;
    if text.chars().any(|c| (c <= '\u{1f}' && !matches!(c, '\t' | '\r' | '\n')) || c == '\u{7f}') {
        Err(Status::UnsupportedInput)
    } else { Ok(()) }
}

/// Encode one frozen logical Key into raw bytes. Release emits zero bytes;
/// unsupported combinations never write a prefix to the caller's buffer.
pub fn encode_key(code: u32, mods: u32, pressed: u8, out: &mut [u8]) -> Result<usize, Status> {
    use key_code::*;
    if !valid_key(code) || mods & !modifiers::ALL != 0 { return Err(Status::UnsupportedInput); }
    if pressed > 2 { return Err(Status::Malformed); }
    let mut bytes = [0u8; 16];
    let mut w = Writer::new(&mut bytes);
    let alt = mods & modifiers::ALT != 0;
    let ctrl = mods & modifiers::CONTROL != 0;
    let shift = mods & modifiers::SHIFT != 0;
    if code <= 0x10ffff {
        if !alt && !ctrl { return Err(Status::UnsupportedInput); }
        if alt { w.u8(0x1b)?; }
        if ctrl {
            let byte = match code {
                0x61..=0x7a => (code - 0x60) as u8,
                0x41..=0x5a => (code - 0x40) as u8,
                0x20 | 0x40 => 0, 0x5b => 0x1b, 0x5c => 0x1c,
                0x5d => 0x1d, 0x5e => 0x1e, 0x5f => 0x1f, 0x3f => 0x7f,
                _ => return Err(Status::UnsupportedInput),
            };
            w.u8(byte)?;
        } else {
            let mut utf8 = [0; 4];
            w.bytes(char::from_u32(code).ok_or(Status::UnsupportedInput)?.encode_utf8(&mut utf8).as_bytes())?;
        }
    } else if matches!(code, ENTER | BACKSPACE | ESCAPE | TAB) {
        if ctrl { return Err(Status::UnsupportedInput); }
        if alt { w.u8(0x1b)?; }
        match code {
            ENTER => w.u8(b'\r')?, BACKSPACE => w.u8(0x7f)?, ESCAPE => w.u8(0x1b)?,
            TAB if shift => w.bytes(b"\x1b[Z")?, TAB => w.u8(b'\t')?, _ => unreachable!(),
        }
    } else {
        let parameter = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
        let final_byte = match code {
            UP => Some(b'A'), DOWN => Some(b'B'), RIGHT => Some(b'C'), LEFT => Some(b'D'),
            HOME => Some(b'H'), END => Some(b'F'), F1 => Some(b'P'),
            0x110021 => Some(b'Q'), 0x110022 => Some(b'R'), 0x110023 => Some(b'S'), _ => None,
        };
        if let Some(final_byte) = final_byte {
            if parameter == 1 && (F1..=F1 + 3).contains(&code) { w.bytes(b"\x1bO")?; }
            else { w.bytes(b"\x1b[")?; }
            if parameter != 1 { w.bytes(&[b'1', b';', b'0' + parameter])?; }
            w.u8(final_byte)?;
        } else {
            let number: u8 = match code {
                INSERT => 2, DELETE => 3, PAGE_UP => 5, PAGE_DOWN => 6,
                0x110024 => 15, 0x110025 => 17, 0x110026 => 18, 0x110027 => 19,
                0x110028 => 20, 0x110029 => 21, 0x11002a => 23, F12 => 24,
                _ => return Err(Status::UnsupportedInput),
            };
            w.bytes(b"\x1b[")?;
            if number >= 10 { w.u8(b'0' + number / 10)?; }
            w.u8(b'0' + number % 10)?;
            if parameter != 1 { w.bytes(&[b';', b'0' + parameter])?; }
            w.u8(b'~')?;
        }
    }
    let len = w.position();
    if pressed == 0 { return Ok(0); }
    out.get_mut(..len).ok_or(Status::BufferTooSmall)?.copy_from_slice(&bytes[..len]);
    Ok(len)
}

fn event(kind: u16, payload: &[u8], out: &mut [u8]) -> Result<usize, Status> {
    let len = EVENT_HEADER_LEN + payload.len();
    let out = out.get_mut(..len).ok_or(Status::BufferTooSmall)?;
    EventHeaderV1 { sequence: 0, epoch: 0, kind, payload_len: payload.len() as u16, flags: 0 }.encode(out)?;
    out[EVENT_HEADER_LEN..].copy_from_slice(payload);
    Ok(len)
}
fn key_event(code: u32, mods: u32, out: &mut [u8]) -> Result<usize, Status> {
    let mut payload = [0; 16];
    let mut w = Writer::new(&mut payload);
    w.u32(code)?; w.u32(mods)?; w.u8(1)?; w.zero(7)?;
    event(event_kind::KEY, &payload, out)
}
fn scalar_event(bytes: &[u8], alt: bool, out: &mut [u8]) -> Result<usize, Status> {
    if bytes.len() == 1 {
        let (code, mods) = match bytes[0] {
            0 => (0x20, modifiers::CONTROL),
            b'\t' => (key_code::TAB, 0), b'\r' => (key_code::ENTER, 0),
            0x1b => (key_code::ESCAPE, 0), 0x7f => (key_code::BACKSPACE, 0),
            1..=26 => (u32::from(bytes[0]) + 0x60, modifiers::CONTROL),
            0x1c..=0x1f => (u32::from(bytes[0]) + 0x40, modifiers::CONTROL),
            _ => (u32::from(bytes[0]), 0),
        };
        if mods != 0 || alt || bytes[0] < 0x20 || bytes[0] == 0x7f {
            return key_event(code, mods | if alt { modifiers::ALT } else { 0 }, out);
        }
    }
    let text = core::str::from_utf8(bytes).map_err(|_| Status::UnsupportedInput)?;
    let code = text.chars().next().ok_or(Status::UnsupportedInput)? as u32;
    if alt { key_event(code, modifiers::ALT, out) }
    else { validate_text(bytes)?; event(event_kind::TEXT, bytes, out) }
}

/// Incremental bounded serial/SSH grammar. Returns complete normalized frames
/// (sequence/epoch zero). Paste markers are framing: the owner must stage and
/// validate the entire paste before delivery. All timestamps are nanoseconds.
#[derive(Debug)]
pub struct SerialDecoder {
    pending: [u8; CONTROL_MAX],
    len: usize,
    esc_started: Option<u64>,
    paste: bool,
    last_now: Option<u64>,
}
impl Default for SerialDecoder { fn default() -> Self { Self::new() } }
impl SerialDecoder {
    pub const fn new() -> Self { Self { pending: [0; CONTROL_MAX], len: 0, esc_started: None, paste: false, last_now: None } }
    pub fn reset(&mut self) { self.wipe_pending(); self.esc_started = None; self.paste = false; self.last_now = None; }
    /// Zero every pending byte (volatile: partial sequences may be pasted secrets), then forget them.
    fn wipe_pending(&mut self) {
        // Safe Rust only (this crate denies `unsafe`): write zeros, then make the
        // buffer observable so the stores cannot be elided as dead.
        self.pending.fill(0);
        core::hint::black_box(&mut self.pending);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        self.len = 0;
    }
    /// Every completed or aborted sequence ends here: no partial-sequence byte survives it.
    /// The paste flag is framing state across many sequences and is left to PasteEnd/reset/finish.
    fn clear_action(&mut self) { self.wipe_pending(); self.esc_started = None; }
    pub fn deadline_ns(&self) -> Option<u64> {
        self.esc_started.and_then(|start| start.checked_add(ESC_TIMEOUT_NS))
    }
    pub fn feed(&mut self, byte: u8, now_ns: u64, out: &mut [u8]) -> Result<usize, Status> {
        let prefix = self.expire(now_ns, out)?;
        let out = out.get_mut(prefix..).ok_or(Status::BufferTooSmall)?;
        // A second ESC may be the Alt prefix of Shift+Tab.
        if self.len >= 2 && self.pending[..2] == [0x1b, 0x1b] {
            if self.len == 2 && byte == b'[' {
                self.pending[2] = byte;
                self.len = 3;
                return Ok(prefix);
            }
            if self.len == 3 && byte == b'Z' {
                let n = key_event(key_code::TAB, modifiers::ALT | modifiers::SHIFT, out)?;
                self.clear_action();
                return Ok(prefix + n);
            }
            let had_bracket = self.len == 3;
            let mut n = key_event(key_code::ESCAPE, modifiers::ALT, out)?;
            self.clear_action();
            if had_bracket { n += scalar_event(b"[", false, &mut out[n..])?; }
            n += self.feed(byte, now_ns, &mut out[n..])?;
            return Ok(prefix + n);
        }
        if self.len == CONTROL_MAX { self.clear_action(); return Err(Status::UnsupportedInput); }
        self.pending[self.len] = byte;
        self.len += 1;
        if self.len == 1 && byte == 0x1b {
            if now_ns.checked_add(ESC_TIMEOUT_NS).is_none() {
                self.clear_action();
                return Err(Status::Malformed);
            }
            self.esc_started = Some(now_ns);
            return Ok(prefix);
        }
        let result = self.decode_pending(out);
        if !matches!(result, Ok(0) | Err(Status::BufferTooSmall)) { self.clear_action(); }
        result.map(|n| prefix + n)
    }
    fn decode_pending(&mut self, out: &mut [u8]) -> Result<usize, Status> {
        let bytes = &self.pending[..self.len];
        if bytes[0] != 0x1b {
            if self.paste {
                match core::str::from_utf8(bytes) {
                    Ok(_) => { validate_paste(bytes)?; return event(event_kind::TEXT, bytes, out); }
                    Err(error) if error.error_len().is_none() && bytes.len() < 4 => return Ok(0),
                    Err(_) => return Err(Status::UnsupportedInput),
                }
            }
            return match core::str::from_utf8(bytes) {
                Ok(_) => scalar_event(bytes, false, out),
                Err(error) if error.error_len().is_none() && bytes.len() < 4 => Ok(0),
                Err(_) => Err(Status::UnsupportedInput),
            };
        }
        if self.len == 1 { return Ok(0); }
        if self.paste {
            if b"\x1b[201~".starts_with(bytes) {
                if self.len < 6 { return Ok(0); }
                let len = event(event_kind::PASTE_END, &[], out)?;
                self.paste = false;
                return Ok(len);
            }
            return Err(Status::UnsupportedInput);
        }
        if bytes == b"\x1b\x1b" { return Ok(0); }
        if bytes == b"\x1b[200~" {
            let len = event(event_kind::PASTE_BEGIN, &[], out)?;
            self.paste = true;
            return Ok(len);
        }
        if bytes[1] != b'[' && bytes[1] != b'O' {
            return match core::str::from_utf8(&bytes[1..]) {
                Ok(_) => scalar_event(&bytes[1..], true, out),
                Err(error) if error.error_len().is_none() && self.len < 5 => Ok(0),
                Err(_) => Err(Status::UnsupportedInput),
            };
        }
        if self.len == 2 { return Ok(0); }
        if self.len == 3 && bytes[1] == b'O' {
            let code = match byte_final(bytes[2]) {
                Some(code) if (key_code::F1..=key_code::F1 + 3).contains(&code) => code,
                _ => return Err(Status::UnsupportedInput),
            };
            return key_event(code, 0, out);
        }
        let last = bytes[self.len - 1];
        if last.is_ascii_digit() || last == b';' { return Ok(0); }
        if bytes[1] != b'[' { return Err(Status::UnsupportedInput); }
        if bytes == b"\x1b[Z" { return key_event(key_code::TAB, modifiers::SHIFT, out); }
        let parameters = &bytes[2..self.len - 1];
        let (number, mods) = parse_parameters(parameters)?;
        let code = if last == b'~' {
            match number {
                2 => key_code::INSERT, 3 => key_code::DELETE,
                5 => key_code::PAGE_UP, 6 => key_code::PAGE_DOWN,
                15 => key_code::F1 + 4, 17 => key_code::F1 + 5,
                18 => key_code::F1 + 6, 19 => key_code::F1 + 7,
                20 => key_code::F1 + 8, 21 => key_code::F1 + 9,
                23 => key_code::F1 + 10, 24 => key_code::F12,
                _ => return Err(Status::UnsupportedInput),
            }
        } else {
            let code = byte_final(last).ok_or(Status::UnsupportedInput)?;
            if !parameters.is_empty() && (number != 1 || mods == 0) { return Err(Status::UnsupportedInput); }
            if parameters.is_empty() && (key_code::F1..=key_code::F1 + 3).contains(&code) { return Err(Status::UnsupportedInput); }
            code
        };
        key_event(code, mods, out)
    }
    /// Resolve an expired ESC prefix. Incomplete longer controls fail explicitly.
    pub fn expire(&mut self, now_ns: u64, out: &mut [u8]) -> Result<usize, Status> {
        if self.last_now.is_some_and(|last| now_ns < last) {
            self.reset();
            return Err(Status::Malformed);
        }
        self.last_now = Some(now_ns);
        let Some(deadline) = self.deadline_ns() else { return Ok(0); };
        if now_ns < deadline { return Ok(0); }
        self.finish_action(out)
    }
    fn finish_action(&mut self, out: &mut [u8]) -> Result<usize, Status> {
        let result = if self.paste { Err(Status::UnsupportedInput) }
            else if self.len == 1 { key_event(key_code::ESCAPE, 0, out) }
            else if self.len == 2 && matches!(self.pending[1], b'[' | b'O') { scalar_event(&self.pending[1..2], true, out) }
            else if self.len >= 2 && self.pending[..2] == [0x1b, 0x1b] {
                let n = key_event(key_code::ESCAPE, modifiers::ALT, out)?;
                if self.len == 3 { scalar_event(b"[", false, &mut out[n..]).map(|tail| n + tail) } else { Ok(n) }
            }
            else { Err(Status::UnsupportedInput) };
        if !matches!(result, Err(Status::BufferTooSmall)) { self.clear_action(); }
        result
    }
    /// End-of-source resolution; incomplete UTF-8/control/paste is unsupported.
    pub fn finish(&mut self, out: &mut [u8]) -> Result<usize, Status> {
        if self.paste { self.reset(); return Err(Status::UnsupportedInput); }
        if self.len == 0 { return Ok(0); }
        if self.esc_started.is_some() { self.finish_action(out) }
        else { self.clear_action(); Err(Status::UnsupportedInput) }
    }
}
fn byte_final(byte: u8) -> Option<u32> {
    use key_code::*;
    Some(match byte { b'A' => UP, b'B' => DOWN, b'C' => RIGHT, b'D' => LEFT,
        b'H' => HOME, b'F' => END, b'P' => F1, b'Q' => F1 + 1,
        b'R' => F1 + 2, b'S' => F1 + 3, _ => return None })
}
fn parse_parameters(bytes: &[u8]) -> Result<(u16, u32), Status> {
    if bytes.is_empty() { return Ok((1, 0)); }
    let mut parts = bytes.split(|byte| *byte == b';');
    let number_bytes = parts.next().ok_or(Status::UnsupportedInput)?;
    if number_bytes.is_empty() || number_bytes[0] == b'0' { return Err(Status::UnsupportedInput); }
    let mut number = 0u16;
    for byte in number_bytes {
        if !byte.is_ascii_digit() { return Err(Status::UnsupportedInput); }
        number = number.checked_mul(10).and_then(|n| n.checked_add(u16::from(*byte - b'0'))).ok_or(Status::UnsupportedInput)?;
    }
    let mods = if let Some(parameter) = parts.next() {
        if parameter.len() != 1 || !(b'2'..=b'8').contains(&parameter[0]) { return Err(Status::UnsupportedInput); }
        let value = parameter[0] - b'1';
        u32::from(value & 1) | if value & 2 != 0 { modifiers::ALT } else { 0 }
            | if value & 4 != 0 { modifiers::CONTROL } else { 0 }
    } else { 0 };
    if parts.next().is_some() { return Err(Status::UnsupportedInput); }
    Ok((number, mods))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_namespace_preserves_frozen_raw_bytes() {
        let mut out = [0; 16];
        for (code, mods, expected) in [
            (key_code::ENTER, 0, &b"\r"[..]),
            (key_code::BACKSPACE, 0, &b"\x7f"[..]),
            (key_code::UP, modifiers::CONTROL, &b"\x1b[1;5A"[..]),
            (key_code::LEFT, modifiers::ALT, &b"\x1b[1;3D"[..]),
            (b'c' as u32, modifiers::CONTROL, &b"\x03"[..]),
            (key_code::F12, modifiers::ALL, &b"\x1b[24;8~"[..]),
        ] {
            let n = encode_key(code, mods, 1, &mut out).unwrap();
            assert_eq!(&out[..n], expected);
            assert_eq!(encode_key(code, mods, 0, &mut out), Ok(0));
            let n = encode_key(code, mods, 2, &mut out).unwrap();
            assert_eq!(&out[..n], expected);
        }
        out.fill(0xaa);
        assert_eq!(encode_key(key_code::ENTER, modifiers::CONTROL, 1, &mut out), Err(Status::UnsupportedInput));
        assert_eq!(encode_key(key_code::UP, 8, 1, &mut out), Err(Status::UnsupportedInput));
        assert_eq!(out, [0xaa; 16]);
    }
    #[test]
    fn frames_reject_bad_flags_lengths_and_payloads() {
        let mut out = [0; EVENT_MAX];
        let n = key_event(key_code::ENTER, 0, &mut out).unwrap();
        assert_eq!(EventHeaderV1::decode(&out[..n]).unwrap().kind, event_kind::KEY);
        assert!(EventHeaderV1::decode(&out[..n - 1]).is_err());
        out[20] = 1;
        assert_eq!(EventHeaderV1::decode(&out[..n]), Err(Status::InvalidFlags));
        assert_eq!(validate_event(event_kind::TEXT, b"\x1b"), Err(Status::UnsupportedInput));
        assert_eq!(validate_paste(b"a\t\r\nb"), Ok(()));
        assert_eq!(validate_paste(b"a\x1bb"), Err(Status::UnsupportedInput));
        assert_eq!(validate_dimensions(256, 512), Ok(()));
        assert_eq!(validate_dimensions(257, 512), Err(Status::Malformed));
    }
    #[test]
    fn lost_reasons_include_frontend_detach_and_attach_only() {
        for reason in 1u32..=5 {
            assert_eq!(validate_event(event_kind::LOST, &reason.to_le_bytes()), Ok(()), "reason {reason}");
        }
        for reason in [0u32, 6, u32::MAX] {
            assert_eq!(validate_event(event_kind::LOST, &reason.to_le_bytes()), Err(Status::Malformed), "reason {reason}");
        }
        assert!(validate_event(event_kind::LOST, &[4, 0, 0]).is_err());
    }
    #[test]
    fn fragmented_serial_and_escape_deadlines_are_canonical() {
        let mut decoder = SerialDecoder::new();
        let mut out = [0; EVENT_MAX];
        for byte in b"\x1b[1;5" { assert_eq!(decoder.feed(*byte, 0, &mut out), Ok(0)); }
        let n = decoder.feed(b'A', 29_000_000, &mut out).unwrap();
        assert_eq!(EventHeaderV1::decode(&out[..n]).unwrap().kind, event_kind::KEY);
        let mut r = Reader::new(&out[24..n]);
        assert_eq!(r.u32(), Ok(key_code::UP));
        assert_eq!(r.u32(), Ok(modifiers::CONTROL));
        assert_eq!(decoder.feed(0x1b, 40_000_000, &mut out), Ok(0));
        assert_eq!(decoder.expire(69_999_999, &mut out), Ok(0));
        assert_eq!(decoder.expire(70_000_000, &mut out), Ok(40));
        assert_eq!(decoder.feed(0xc3, 71_000_000, &mut out), Ok(0));
        let n = decoder.feed(0xa9, 72_000_000, &mut out).unwrap();
        assert_eq!(&out[24..n], "é".as_bytes());
    }
    #[test]
    fn every_named_key_and_modifier_roundtrips_through_the_shared_grammar() {
        let named = [
            key_code::ENTER, key_code::ESCAPE, key_code::BACKSPACE, key_code::TAB,
            key_code::INSERT, key_code::DELETE, key_code::UP, key_code::DOWN,
            key_code::LEFT, key_code::RIGHT, key_code::HOME, key_code::END,
            key_code::PAGE_UP, key_code::PAGE_DOWN,
        ];
        for code in named.into_iter().chain(key_code::F1..=key_code::F12) {
            for mods in 0..=modifiers::ALL {
                let mut raw = [0; 16];
                let Ok(raw_len) = encode_key(code, mods, 1, &mut raw) else { continue; };
                let mut decoder = SerialDecoder::new();
                let mut frame = [0; EVENT_MAX];
                let mut len = 0;
                for byte in &raw[..raw_len] {
                    let n = decoder.feed(*byte, 0, &mut frame[len..]).unwrap();
                    len += n;
                }
                len += decoder.finish(&mut frame[len..]).unwrap();
                EventHeaderV1::decode(&frame[..len]).unwrap();
                let mut r = Reader::new(&frame[EVENT_HEADER_LEN..len]);
                assert_eq!(r.u32(), Ok(code));
                // Shift on Enter/Backspace/Escape is intentionally byte-neutral.
                let expected = if matches!(code, key_code::ENTER | key_code::BACKSPACE | key_code::ESCAPE) {
                    mods & !modifiers::SHIFT
                } else { mods };
                assert_eq!(r.u32(), Ok(expected));
            }
        }
    }

    #[test]
    fn malformed_controls_paste_and_source_end_do_not_guess_actions() {
        let mut frame = [0; EVENT_MAX];
        for bytes in [&b"\x1b[1A"[..], &b"\x1b[01;2A"[..], &b"\x1b[1;1A"[..]] {
            let mut decoder = SerialDecoder::new();
            let mut rejected = false;
            for byte in bytes {
                if decoder.feed(*byte, 0, &mut frame).is_err() { rejected = true; break; }
            }
            if !rejected { rejected = decoder.finish(&mut frame).is_err(); }
            assert!(rejected);
        }
        let mut decoder = SerialDecoder::new();
        for byte in b"\x1b[200~" { decoder.feed(*byte, 0, &mut frame).unwrap(); }
        assert_eq!(decoder.finish(&mut frame), Err(Status::UnsupportedInput));
        assert_eq!(decoder.feed(0xc3, 0, &mut frame), Ok(0));
        assert_eq!(decoder.finish(&mut frame), Err(Status::UnsupportedInput));
        assert_eq!(decoder.feed(0x1b, u64::MAX, &mut frame), Err(Status::Malformed));
        decoder.reset();
        assert_eq!(decoder.feed(0x1b, 10, &mut frame), Ok(0));
        assert_eq!(decoder.expire(9, &mut frame), Err(Status::Malformed));
    }

    fn assert_wiped(decoder: &SerialDecoder, what: &str) {
        assert_eq!(decoder.len, 0, "{what}: len");
        assert!(decoder.pending.iter().all(|byte| *byte == 0), "{what}: pending bytes survived");
    }
    fn feed_all(decoder: &mut SerialDecoder, bytes: &[u8], now: u64) -> Result<usize, Status> {
        let mut frame = [0; EVENT_MAX];
        let mut last = Ok(0);
        for byte in bytes { last = decoder.feed(*byte, now, &mut frame); if last.is_err() { break; } }
        last
    }

    #[test]
    fn raw_lf_is_control_j_and_cr_is_enter() {
        let mut decoder = SerialDecoder::new();
        let mut frame = [0; EVENT_MAX];
        for (byte, code, mods) in [(0x0au8, b'j' as u32, modifiers::CONTROL), (0x0d, key_code::ENTER, 0)] {
            let n = decoder.feed(byte, 0, &mut frame).unwrap();
            assert_eq!(EventHeaderV1::decode(&frame[..n]).unwrap().kind, event_kind::KEY);
            let mut r = Reader::new(&frame[EVENT_HEADER_LEN..n]);
            assert_eq!(r.u32(), Ok(code));
            assert_eq!(r.u32(), Ok(mods));
            assert_wiped(&decoder, "single-byte key");
        }
    }

    #[test]
    fn no_partial_sequence_byte_survives_a_completed_or_rejected_sequence() {
        let mut frame = [0; EVENT_MAX];
        // Incomplete sequences retain their bytes; completion wipes every one of them.
        for complete in [&b"\x1b[1;5A"[..], b"\x1b[24;8~", b"\x1bOP", b"\x1b[Z", b"\x1bx", b"\x1b\x1bx",
                         b"\x1b\x1b[Z", b"\x1b\x1b[x", "é".as_bytes(), "😀".as_bytes(), b"a"] {
            let mut decoder = SerialDecoder::new();
            let (last, head) = complete.split_last().unwrap();
            for byte in head { assert!(decoder.feed(*byte, 0, &mut frame).is_ok()); }
            if !head.is_empty() && head != b"\x1b\x1b" { assert!(decoder.pending[..head.len().min(CONTROL_MAX)].iter().any(|b| *b != 0)); }
            assert!(decoder.feed(*last, 0, &mut frame).unwrap() > 0, "{complete:?}");
            assert_wiped(&decoder, "completed");
            assert_eq!(decoder.deadline_ns(), None);
        }
        // Rejected sequences wipe too.
        for rejected in [&b"\x1b[1A"[..], b"\x1b[01;2A", b"\x1b[1;1A", b"\x1bOZ", b"\x1b[9~", b"\xff", b"\xc3("] {
            let mut decoder = SerialDecoder::new();
            assert!(feed_all(&mut decoder, rejected, 0).is_err(), "{rejected:?}");
            assert_wiped(&decoder, "rejected");
        }
        // Control overflow: the full pending buffer is wiped by the rejection.
        let mut decoder = SerialDecoder::new();
        assert_eq!(feed_all(&mut decoder, b"\x1b[", 0), Ok(0));
        for _ in 0..CONTROL_MAX - 2 { assert_eq!(decoder.feed(b'1', 0, &mut frame), Ok(0)); }
        assert!(decoder.pending.iter().all(|b| *b != 0));
        assert_eq!(decoder.feed(b'1', 0, &mut frame), Err(Status::UnsupportedInput));
        assert_wiped(&decoder, "overflow");
        // ESC deadline resolution (expire and finish) and reset.
        let mut decoder = SerialDecoder::new();
        assert_eq!(decoder.feed(0x1b, 0, &mut frame), Ok(0));
        assert_eq!(decoder.expire(ESC_TIMEOUT_NS, &mut frame), Ok(40));
        assert_wiped(&decoder, "expired ESC");
        assert_eq!(feed_all(&mut decoder, b"\x1b[1", ESC_TIMEOUT_NS), Ok(0));
        assert!(decoder.finish(&mut frame).is_err());
        assert_wiped(&decoder, "finish");
        assert_eq!(feed_all(&mut decoder, b"\xc3", ESC_TIMEOUT_NS), Ok(0));
        assert!(decoder.finish(&mut frame).is_err());
        assert_wiped(&decoder, "finish partial scalar");
        assert_eq!(feed_all(&mut decoder, b"\x1b[1;", ESC_TIMEOUT_NS), Ok(0));
        decoder.reset();
        assert_wiped(&decoder, "reset");
        assert_eq!(decoder.feed(0x1b, 10, &mut frame), Ok(0));
        assert_eq!(decoder.expire(9, &mut frame), Err(Status::Malformed));
        assert_wiped(&decoder, "backwards clock");
    }

    #[test]
    fn paste_text_and_aborted_paste_controls_are_wiped() {
        let mut frame = [0; EVENT_MAX];
        let mut decoder = SerialDecoder::new();
        assert_eq!(feed_all(&mut decoder, b"\x1b[200~", 0).map(|n| n > 0), Ok(true));
        assert_wiped(&decoder, "paste begin");
        for byte in b"hunter2" {
            assert!(decoder.feed(*byte, 0, &mut frame).unwrap() > 0);
            assert_wiped(&decoder, "paste text");
        }
        assert!(decoder.paste, "paste framing survives individual text sequences");
        // A control that is not PasteEnd inside a paste is rejected and wiped.
        assert_eq!(feed_all(&mut decoder, b"\x1b[2", 0), Ok(0));
        assert!(decoder.pending[..3].iter().all(|b| *b != 0));
        assert_eq!(decoder.feed(b'x', 0, &mut frame), Err(Status::UnsupportedInput));
        assert_wiped(&decoder, "aborted paste control");
        assert!(decoder.paste, "the owner discards the paste; framing ends at PasteEnd/reset");
        assert_eq!(feed_all(&mut decoder, b"\x1b[201~", 0).map(|n| n > 0), Ok(true));
        assert_wiped(&decoder, "paste end");
        assert!(!decoder.paste);
        // An incomplete scalar inside a paste dies with the source.
        feed_all(&mut decoder, b"\x1b[200~", 0).unwrap();
        assert_eq!(decoder.feed(0xc3, 0, &mut frame), Ok(0));
        assert_eq!(decoder.finish(&mut frame), Err(Status::UnsupportedInput));
        assert_wiped(&decoder, "paste finish");
        assert!(!decoder.paste);
    }
}
