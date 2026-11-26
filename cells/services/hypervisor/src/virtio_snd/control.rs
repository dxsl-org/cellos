//! virtio-sound control requests (virtio 1.2 §5.14.6) and the lifecycle of
//! the device's one output stream.
//!
//! Pure: no guest memory and no clock, so every length and every transition
//! is host-tested. A request is the device-readable part of a control chain
//! and must be exactly its structure's size; `room` is the device-writable
//! part, which the reply never exceeds.

extern crate alloc;

use alloc::vec::Vec;

use super::query::query;

pub(super) const R_JACK_INFO: u32 = 1;
pub(super) const R_PCM_INFO: u32 = 0x0100;
const R_PCM_SET_PARAMS: u32 = 0x0101;
const R_PCM_PREPARE: u32 = 0x0102;
const R_PCM_RELEASE: u32 = 0x0103;
const R_PCM_START: u32 = 0x0104;
const R_PCM_STOP: u32 = 0x0105;
const R_CHMAP_INFO: u32 = 0x0200;

pub const S_OK: u32 = 0x8000;
pub const S_BAD_MSG: u32 = 0x8001;
pub const S_NOT_SUPP: u32 = 0x8002;
pub const S_IO_ERR: u32 = 0x8003;

/// The configuration space: no jacks, one output stream, one stereo map.
pub const JACKS: u32 = 0;
pub const STREAMS: u32 = 1;
pub const CHMAPS: u32 = 1;

/// The only format: `wire::audio`'s S16LE, 48 kHz, stereo.
pub(super) const FMT_S16: u8 = 5;
pub(super) const RATE_48000: u8 = 7;
pub(super) const CHANNELS: u8 = wire::audio::CHANNELS as u8;
pub const FRAME_BYTES: u32 = wire::audio::FRAME_BYTES;
/// Largest ring buffer a driver may set up, and so the largest tx buffer.
pub const MAX_BUFFER_BYTES: u32 = 512 * 1024;

const PCM_HDR_LEN: usize = 8;
const SET_PARAMS_LEN: usize = 24;

/// Where the stream is in the spec's command lifecycle (§5.14.6.6.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Initial,
    ParamsSet,
    Prepared,
    Running,
    Stopped,
    Released,
}

/// What the device must do after answering a request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    None,
    Start,
    Stop,
    /// Complete every pending tx buffer before the reply is published.
    Release,
}

/// The output stream's control state.
pub struct Pcm {
    state: State,
}

/// The reply bytes (at most the request's `room`) and the device's effect.
pub struct Reply {
    pub bytes: Vec<u8>,
    pub effect: Effect,
}

impl Pcm {
    pub const fn new() -> Self {
        Self {
            state: State::Initial,
        }
    }

    #[cfg(test)]
    pub fn state(&self) -> State {
        self.state
    }

    /// Prepared, running or stopped: tx buffers queue (output pre-buffers
    /// while prepared). Otherwise they complete at once with an I/O error.
    pub fn accepts_io(&self) -> bool {
        matches!(
            self.state,
            State::Prepared | State::Running | State::Stopped
        )
    }

    pub fn playing(&self) -> bool {
        self.state == State::Running
    }

    pub fn reset(&mut self) {
        self.state = State::Initial;
    }

    /// Answer one control request.
    pub fn handle(&mut self, request: &[u8], room: usize) -> Reply {
        let (status, items, effect) = match le32(request, 0) {
            None => (S_BAD_MSG, Vec::new(), Effect::None),
            Some(code @ (R_JACK_INFO | R_PCM_INFO | R_CHMAP_INFO)) => {
                match query(code, request, room) {
                    Ok(items) => (S_OK, items, Effect::None),
                    Err(status) => (status, Vec::new(), Effect::None),
                }
            }
            Some(R_PCM_SET_PARAMS) => (self.set_params(request), Vec::new(), Effect::None),
            Some(code @ (R_PCM_PREPARE | R_PCM_RELEASE | R_PCM_START | R_PCM_STOP)) => {
                match self.command(code, request) {
                    Ok(effect) => (S_OK, Vec::new(), effect),
                    Err(status) => (status, Vec::new(), Effect::None),
                }
            }
            // JACK_REMAP included: no jack exists, none offers remapping.
            Some(_) => (S_NOT_SUPP, Vec::new(), Effect::None),
        };
        let mut bytes = Vec::new();
        if room >= 4 {
            bytes.extend_from_slice(&status.to_le_bytes());
            bytes.extend_from_slice(&items);
        }
        Reply { bytes, effect }
    }

    fn set_params(&mut self, request: &[u8]) -> u32 {
        if request.len() != SET_PARAMS_LEN || le32(request, 4) != Some(0) {
            return S_BAD_MSG;
        }
        if !matches!(
            self.state,
            State::Initial | State::ParamsSet | State::Prepared | State::Released
        ) {
            return S_BAD_MSG;
        }
        let field = |at| le32(request, at).unwrap_or(0);
        let (buffer, period, features) = (field(8), field(12), field(16));
        let (channels, format, rate) = (request[20], request[21], request[22]);
        if features != 0 || channels != CHANNELS || format != FMT_S16 || rate != RATE_48000 {
            return S_NOT_SUPP;
        }
        if period == 0
            || !period.is_multiple_of(FRAME_BYTES)
            || !buffer.is_multiple_of(period)
            || buffer > MAX_BUFFER_BYTES
        {
            return S_BAD_MSG;
        }
        // Once per parameter set: the buffer the guest chose is what an
        // underrunning stream is judged against.
        ostd::io::println(&alloc::format!(
            "[hv-x86] sound: params buffer {buffer} B, period {period} B"
        ));
        self.state = State::ParamsSet;
        S_OK
    }

    fn command(&mut self, code: u32, request: &[u8]) -> Result<Effect, u32> {
        if request.len() != PCM_HDR_LEN || le32(request, 4) != Some(0) {
            return Err(S_BAD_MSG);
        }
        let (next, effect) = match (code, self.state) {
            (R_PCM_PREPARE, State::ParamsSet | State::Prepared | State::Released) => {
                (State::Prepared, Effect::None)
            }
            (R_PCM_START, State::Prepared | State::Stopped) => (State::Running, Effect::Start),
            (R_PCM_STOP, State::Running) => (State::Stopped, Effect::Stop),
            (R_PCM_RELEASE, State::Prepared | State::Stopped) => (State::Released, Effect::Release),
            _ => return Err(S_BAD_MSG),
        };
        self.state = next;
        Ok(effect)
    }
}

pub(super) fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    let word = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
