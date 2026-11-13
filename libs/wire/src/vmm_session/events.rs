// SPDX-License-Identifier: Apache-2.0
//! Launcher -> hypervisor resize request and hypervisor -> launcher events.
//!
//! Events are posted (never sent): a launcher that stops receiving must not
//! park the hypervisor's vCPU loop. Every frame starts with its opcode byte.

use types::{ViError, ViResult};

use super::VMM_OP_RESIZE_APP;

/// The guest scanout Grant changed: map `grant` (shared read-only, owned by the
/// hypervisor) as a `width`x`height` BGRA8888 image with a `width * 4` stride.
pub const VMM_EVT_SCANOUT: u8 = 0x81;
/// Pixels in a rectangle of the current scanout Grant changed.
pub const VMM_EVT_DAMAGE: u8 = 0x82;
/// The guest stopped; the hypervisor exits after posting this.
pub const VMM_EVT_EXITED: u8 = 0x83;

/// Largest display edge a launcher may request (a 4K scanout).
pub const MAX_DISPLAY_EDGE: u32 = 4096;
/// Smallest display edge a launcher may request.
pub const MIN_DISPLAY_EDGE: u32 = 64;

/// Whether `width`x`height` is a display size the hypervisor accepts.
pub fn display_size_valid(width: u32, height: u32) -> bool {
    (MIN_DISPLAY_EDGE..=MAX_DISPLAY_EDGE).contains(&width)
        && (MIN_DISPLAY_EDGE..=MAX_DISPLAY_EDGE).contains(&height)
}

fn le32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

/// Ask the guest to switch its display to a new size (a window resize).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmmResizeRequest {
    pub session_id: u32,
    pub width: u32,
    pub height: u32,
}

impl VmmResizeRequest {
    pub const LEN: usize = 16;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = VMM_OP_RESIZE_APP;
        out[4..8].copy_from_slice(&self.session_id.to_le_bytes());
        out[8..12].copy_from_slice(&self.width.to_le_bytes());
        out[12..16].copy_from_slice(&self.height.to_le_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> ViResult<Self> {
        if buf.len() < Self::LEN || buf[0] != VMM_OP_RESIZE_APP {
            return Err(ViError::InvalidInput);
        }
        Ok(Self {
            session_id: le32(buf, 4),
            width: le32(buf, 8),
            height: le32(buf, 12),
        })
    }
}

/// A hypervisor -> launcher event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmmEvent {
    Scanout {
        grant: u32,
        width: u32,
        height: u32,
    },
    Damage {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    Exited {
        code: u32,
    },
}

impl VmmEvent {
    pub const LEN: usize = 20;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        let (opcode, words) = match *self {
            Self::Scanout {
                grant,
                width,
                height,
            } => (VMM_EVT_SCANOUT, [grant, width, height, 0]),
            Self::Damage {
                x,
                y,
                width,
                height,
            } => (VMM_EVT_DAMAGE, [x, y, width, height]),
            Self::Exited { code } => (VMM_EVT_EXITED, [code, 0, 0, 0]),
        };
        out[0] = opcode;
        for (index, word) in words.iter().enumerate() {
            out[4 + index * 4..8 + index * 4].copy_from_slice(&word.to_le_bytes());
        }
        out
    }

    /// Decode an event frame; `None` for anything that is not one.
    pub fn decode(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::LEN {
            return None;
        }
        let word = |index: usize| le32(buf, 4 + index * 4);
        match buf[0] {
            VMM_EVT_SCANOUT => Some(Self::Scanout {
                grant: word(0),
                width: word(1),
                height: word(2),
            }),
            VMM_EVT_DAMAGE => Some(Self::Damage {
                x: word(0),
                y: word(1),
                width: word(2),
                height: word(3),
            }),
            VMM_EVT_EXITED => Some(Self::Exited { code: word(0) }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip_and_other_frames_are_not_events() {
        for event in [
            VmmEvent::Scanout {
                grant: 7,
                width: 1280,
                height: 720,
            },
            VmmEvent::Damage {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
            },
            VmmEvent::Exited { code: 2 },
        ] {
            assert_eq!(VmmEvent::decode(&event.encode()), Some(event));
        }
        let mut frame = [0u8; VmmEvent::LEN];
        frame[0] = super::super::VMM_OP_LAUNCH_APP;
        assert_eq!(VmmEvent::decode(&frame), None);
        assert_eq!(VmmEvent::decode(&frame[..VmmEvent::LEN - 1]), None);
    }

    #[test]
    fn resize_round_trips_and_sizes_are_bounded() {
        let request = VmmResizeRequest {
            session_id: 1,
            width: 1024,
            height: 640,
        };
        assert_eq!(VmmResizeRequest::decode(&request.encode()), Ok(request));
        assert!(display_size_valid(64, 4096));
        assert!(!display_size_valid(63, 600));
        assert!(!display_size_valid(800, 4097));
    }
}
