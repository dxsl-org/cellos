// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The headless, bounded `cellos-vt1` logical input codec.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use crate::error::TerminalError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeySymbol {
    Character(char), Enter, Backspace, Tab, Escape, Up, Down, Right, Left,
    Home, End, Insert, Delete, PageUp, PageDown, F(u8),
}

impl fmt::Display for KeySymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Character(c) => write!(f, "{}", c),
            Self::F(n) => write!(f, "F{}", n),
            other => write!(f, "{:?}", other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers { pub shift: bool, pub alt: bool, pub control: bool }

impl Modifiers {
    pub const NONE: Self = Self { shift: false, alt: false, control: false };
    pub const SHIFT: Self = Self { shift: true, ..Self::NONE };
    pub const ALT: Self = Self { alt: true, ..Self::NONE };
    pub const CONTROL: Self = Self { control: true, ..Self::NONE };

    pub fn csi_parameter(&self) -> u8 {
        1 + self.shift as u8 + 2 * self.alt as u8 + 4 * self.control as u8
    }
    pub fn from_csi_parameter(m: u8) -> Result<Self, TerminalError> {
        if !(2..=8).contains(&m) { return Err(TerminalError::UnsupportedInput); }
        let n = m - 1;
        Ok(Self { shift: n & 1 != 0, alt: n & 2 != 0, control: n & 4 != 0 })
    }
    pub fn is_empty(&self) -> bool { !self.shift && !self.alt && !self.control }
    pub fn from_bits(bits: u32) -> Result<Self, TerminalError> {
        if bits & !7 != 0 { return Err(TerminalError::UnsupportedInput); }
        Ok(Self { shift: bits & 1 != 0, control: bits & 2 != 0, alt: bits & 4 != 0 })
    }
    pub fn bits(self) -> u32 {
        self.shift as u32 | ((self.control as u32) << 1) | ((self.alt as u32) << 2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyState { Release = 0, Press = 1, Repeat = 2 }

/// Text and Key are mutually exclusive for a single logical action. A Paste is
/// one completely validated batch, never an incrementally delivered prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    Text(String),
    Key { symbol: KeySymbol, modifiers: Modifiers, state: KeyState },
    Paste(String),
}

impl KeySymbol {
    pub fn from_code(code: u32) -> Result<Self, TerminalError> {
        Ok(match code {
            0x20..=0x10ffff if code != 0x7f => Self::Character(
                char::from_u32(code).ok_or(TerminalError::UnsupportedInput)?),
            0x110001 => Self::Enter, 0x110002 => Self::Escape,
            0x110003 => Self::Backspace, 0x110004 => Self::Tab,
            0x110005 => Self::Delete, 0x110006 => Self::Insert,
            0x110010 => Self::Up, 0x110011 => Self::Down,
            0x110012 => Self::Left, 0x110013 => Self::Right,
            0x110014 => Self::Home, 0x110015 => Self::End,
            0x110016 => Self::PageUp, 0x110017 => Self::PageDown,
            0x110020..=0x11002b => Self::F((code - 0x110020 + 1) as u8),
            _ => return Err(TerminalError::UnsupportedInput),
        })
    }
    pub fn code(self) -> Result<u32, TerminalError> {
        Ok(match self {
            Self::Character(c) if c >= ' ' && c != '\u{7f}' => c as u32,
            Self::Enter => 0x110001, Self::Escape => 0x110002,
            Self::Backspace => 0x110003, Self::Tab => 0x110004,
            Self::Delete => 0x110005, Self::Insert => 0x110006,
            Self::Up => 0x110010, Self::Down => 0x110011,
            Self::Left => 0x110012, Self::Right => 0x110013,
            Self::Home => 0x110014, Self::End => 0x110015,
            Self::PageUp => 0x110016, Self::PageDown => 0x110017,
            Self::F(n @ 1..=12) => 0x110020 + n as u32 - 1,
            _ => return Err(TerminalError::UnsupportedInput),
        })
    }
}

impl KeyAction {
    pub fn from_key(code: u32, modifiers: u32, pressed: u8) -> Result<Self, TerminalError> {
        let symbol = KeySymbol::from_code(code)?;
        let modifiers = Modifiers::from_bits(modifiers)?;
        let state = match pressed {
            0 => KeyState::Release, 1 => KeyState::Press, 2 => KeyState::Repeat,
            _ => return Err(TerminalError::UnsupportedInput),
        };
        validate_key(symbol, modifiers)?;
        Ok(Self::Key { symbol, modifiers, state })
    }
    pub fn key_fields(&self) -> Result<(u32, u32, u8), TerminalError> {
        match self {
            Self::Key { symbol, modifiers, state } => {
                validate_key(*symbol, *modifiers)?;
                Ok((symbol.code()?, modifiers.bits(), *state as u8))
            }
            _ => Err(TerminalError::UnsupportedInput),
        }
    }
}

pub fn validate_text(bytes: &[u8]) -> Result<&str, TerminalError> {
    api::services::session::validate_text(bytes).map_err(codec_error)?;
    core::str::from_utf8(bytes).map_err(|_| TerminalError::UnsupportedInput)
}

pub fn validate_paste(bytes: &[u8]) -> Result<&str, TerminalError> {
    api::services::session::validate_paste(bytes).map_err(codec_error)?;
    core::str::from_utf8(bytes).map_err(|_| TerminalError::UnsupportedInput)
}

fn codec_error(status: api::cell_session::Status) -> TerminalError {
    match status {
        api::cell_session::Status::QuotaExceeded => TerminalError::BufferOverflow,
        api::cell_session::Status::Malformed => TerminalError::InvalidInput,
        _ => TerminalError::UnsupportedInput,
    }
}

fn validate_key(symbol: KeySymbol, mods: Modifiers) -> Result<(), TerminalError> {
    api::services::session::encode_key(symbol.code()?, mods.bits(), 0, &mut [0; 16])
        .map_err(codec_error)?;
    Ok(())
}

/// Validate the entire action before appending anything. Paste is bracketed by default.
pub fn encode_key_action(action: &KeyAction, out: &mut Vec<u8>) -> Result<(), TerminalError> {
    encode_key_action_with_paste(action, true, out)
}

pub fn encode_key_action_with_paste(action: &KeyAction, bracketed: bool, out: &mut Vec<u8>) -> Result<(), TerminalError> {
    match action {
        KeyAction::Text(text) => { validate_text(text.as_bytes())?; out.extend_from_slice(text.as_bytes()); }
        KeyAction::Paste(text) => {
            validate_paste(text.as_bytes())?;
            if bracketed { out.extend_from_slice(b"\x1b[200~"); }
            out.extend_from_slice(text.as_bytes());
            if bracketed { out.extend_from_slice(b"\x1b[201~"); }
        }
        KeyAction::Key { symbol, modifiers, state } => {
            let mut bytes = ostd::session::Scrub::<16>::new();
            let count = api::services::session::encode_key(
                symbol.code()?, modifiers.bits(), *state as u8, &mut bytes,
            ).map_err(codec_error)?;
            out.extend_from_slice(&bytes[..count]);
        }
    }
    Ok(())
}

/// Receiver-monotonic wrapper around the one shared ABI grammar. The parser's
/// paste markers remain internal until the complete <=64KiB batch validates.
/// Call `expire` at `deadline_ns` even if the frontend has gone quiet.
#[derive(Debug)]
pub struct Decoder {
    inner: api::services::session::SerialDecoder,
    last_now: Option<u64>,
    in_paste: bool,
    paste: Vec<u8>,
}

impl Default for Decoder { fn default() -> Self { Self::new() } }

impl Decoder {
    pub const MAX_CONTROL_LEN: usize = 256;
    pub const MAX_UTF8_LEN: usize = 4;
    pub const MAX_PASTE_LEN: usize = 65536;
    pub const ESC_TIMEOUT_NS: u64 = 30_000_000;

    pub fn new() -> Self {
        Self { inner: api::services::session::SerialDecoder::new(),
            last_now: None, in_paste: false, paste: Vec::new() }
    }
    pub fn deadline_ns(&self) -> Option<u64> { self.inner.deadline_ns() }
    fn clock(&mut self, now: u64) -> Result<(), TerminalError> {
        if self.last_now.is_some_and(|last| now < last) { return Err(TerminalError::InvalidInput); }
        self.last_now = Some(now);
        Ok(())
    }
    fn reset(&mut self) {
        // A fresh decoder (not `reset`) also overwrites the pending-sequence bytes.
        self.inner = api::services::session::SerialDecoder::new(); self.in_paste = false;
        ostd::session::zeroize_vec(&mut self.paste);
    }
    pub fn feed(&mut self, input: &[u8], now_ns: u64, out: &mut Vec<KeyAction>) -> Result<(), TerminalError> {
        self.expire(now_ns, out)?;
        let mut frame = ostd::session::Scrub::<{ api::services::session::EVENT_MAX }>::new();
        for &byte in input {
            let result = self.inner.feed(byte, now_ns, &mut frame).map_err(codec_error)
                .and_then(|count| self.frame(&frame[..count], out));
            if let Err(error) = result { self.reset(); return Err(error); }
        }
        Ok(())
    }
    pub fn expire(&mut self, now_ns: u64, out: &mut Vec<KeyAction>) -> Result<(), TerminalError> {
        self.clock(now_ns)?;
        let mut frame = ostd::session::Scrub::<{ api::services::session::EVENT_MAX }>::new();
        let result = self.inner.expire(now_ns, &mut frame).map_err(codec_error)
            .and_then(|count| self.frame(&frame[..count], out));
        if result.is_err() { self.reset(); }
        result
    }
    /// EOF never completes a missing scalar, control or paste terminator.
    pub fn flush(&mut self, out: &mut Vec<KeyAction>) -> Result<(), TerminalError> {
        let mut frame = ostd::session::Scrub::<{ api::services::session::EVENT_MAX }>::new();
        let result = self.inner.finish(&mut frame).map_err(codec_error)
            .and_then(|count| self.frame(&frame[..count], out))
            .and_then(|()| if self.in_paste { Err(TerminalError::UnsupportedInput) } else { Ok(()) });
        if result.is_err() { self.reset(); }
        result
    }
    fn frame(&mut self, mut frames: &[u8], out: &mut Vec<KeyAction>) -> Result<(), TerminalError> {
        while !frames.is_empty() {
            let header = frames.get(..24).ok_or(TerminalError::InvalidInput)?;
            let payload_len = u16::from_le_bytes([header[18], header[19]]) as usize;
            let length = 24 + payload_len;
            let frame = frames.get(..length).ok_or(TerminalError::InvalidInput)?;
            self.one_frame(frame, out)?;
            frames = &frames[length..];
        }
        Ok(())
    }
    fn one_frame(&mut self, frame: &[u8], out: &mut Vec<KeyAction>) -> Result<(), TerminalError> {
        use api::services::session::event_kind;
        // Internal decoder frames have no owner sequence/epoch. Paste text may
        // contain HT/CR/LF, unlike ordinary TEXT; validate it as a whole batch.
        let mut reader = api::cell_session::Reader::new(frame);
        let _sequence = reader.u64().map_err(codec_error)?;
        let _epoch = reader.u64().map_err(codec_error)?;
        let kind = reader.u16().map_err(codec_error)?;
        let length = reader.u16().map_err(codec_error)? as usize;
        if reader.u32().map_err(codec_error)? != 0 { return Err(TerminalError::InvalidInput); }
        let payload = reader.take(length).map_err(codec_error)?;
        reader.finish().map_err(codec_error)?;
        match kind {
            event_kind::KEY if !self.in_paste => {
                let mut key = api::cell_session::Reader::new(payload);
                let code = key.u32().map_err(codec_error)?;
                let modifiers = key.u32().map_err(codec_error)?;
                let pressed = key.u8().map_err(codec_error)?;
                if key.take(7).map_err(codec_error)?.iter().any(|&b| b != 0) {
                    return Err(TerminalError::InvalidInput);
                }
                key.finish().map_err(codec_error)?;
                out.push(KeyAction::from_key(code, modifiers, pressed)?);
            }
            event_kind::TEXT if self.in_paste => {
                let next = self.paste.len().checked_add(payload.len()).ok_or(TerminalError::BufferOverflow)?;
                if next > Self::MAX_PASTE_LEN { return Err(TerminalError::BufferOverflow); }
                self.paste.extend_from_slice(payload);
            }
            event_kind::TEXT => {
                out.push(KeyAction::Text(String::from(validate_text(payload)?)));
            }
            event_kind::PASTE_BEGIN if !self.in_paste && payload.is_empty() => {
                self.in_paste = true;
            }
            event_kind::PASTE_END if self.in_paste && payload.is_empty() => {
                validate_paste(&self.paste)?;
                let text = match String::from_utf8(core::mem::take(&mut self.paste)) {
                    Ok(text) => text,
                    Err(error) => {
                        let mut bytes = error.into_bytes();
                        ostd::session::zeroize_vec(&mut bytes);
                        return Err(TerminalError::UnsupportedInput);
                    }
                };
                self.in_paste = false;
                out.push(KeyAction::Paste(text));
            }
            _ => return Err(TerminalError::UnsupportedInput),
        }
        Ok(())
    }
}
impl Drop for Decoder { fn drop(&mut self) { self.reset(); } }

impl KeyAction {
    /// Wipe the text of a `Text`/`Paste` action in place once it has been consumed.
    pub fn scrub(&mut self) {
        if let Self::Text(text) | Self::Paste(text) = self { ostd::session::zeroize_string(text); }
    }
}
