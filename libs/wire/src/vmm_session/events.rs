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
/// The guest's cursor image changed: `width`x`height` BGRA8888 pixels, rows
/// packed, at the start of `grant` (shared read-only, owned by the
/// hypervisor, at least `MAX_POINTER_EDGE`² pixels), hot spot `(hot_x,
/// hot_y)`; or the guest hid its cursor.
pub const VMM_EVT_POINTER: u8 = 0x84;
/// The guest copied text: `len` bytes of UTF-8 (1..=`VMM_CLIPBOARD_MAX`) at
/// the start of `grant` (shared read-only, owned by the hypervisor).
pub const VMM_EVT_CLIPBOARD: u8 = 0x85;

const POINTER_IMAGE: u8 = 0;
const POINTER_HIDDEN: u8 = 1;

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
    /// Layout: byte 1 = 0, word 0 `grant`, word 1 `width | height << 16`,
    /// word 2 `hot_x | hot_y << 16`. Edges are 1..=`MAX_POINTER_EDGE` and the
    /// hot spot lies inside the image, or the frame is not an event.
    Pointer {
        grant: u32,
        width: u16,
        height: u16,
        hot_x: u16,
        hot_y: u16,
    },
    /// Layout: byte 1 = 1, every word 0.
    PointerHidden,
    /// Layout: word 0 `grant`, word 1 `len`, the rest 0.
    Clipboard {
        grant: u32,
        len: u32,
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
            Self::Pointer {
                grant,
                width,
                height,
                hot_x,
                hot_y,
            } => (
                VMM_EVT_POINTER,
                [
                    grant,
                    u32::from(width) | u32::from(height) << 16,
                    u32::from(hot_x) | u32::from(hot_y) << 16,
                    0,
                ],
            ),
            Self::PointerHidden => {
                out[1] = POINTER_HIDDEN;
                (VMM_EVT_POINTER, [0; 4])
            }
            Self::Clipboard { grant, len } => (VMM_EVT_CLIPBOARD, [grant, len, 0, 0]),
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
            VMM_EVT_POINTER => match buf[1] {
                POINTER_HIDDEN if buf[2..Self::LEN].iter().all(|byte| *byte == 0) => {
                    Some(Self::PointerHidden)
                }
                POINTER_IMAGE => {
                    let (width, height) = (word(1) as u16, (word(1) >> 16) as u16);
                    let (hot_x, hot_y) = (word(2) as u16, (word(2) >> 16) as u16);
                    let edge = 1..=crate::display::MAX_POINTER_EDGE;
                    (edge.contains(&width)
                        && edge.contains(&height)
                        && hot_x < width
                        && hot_y < height)
                        .then_some(Self::Pointer {
                            grant: word(0),
                            width,
                            height,
                            hot_x,
                            hot_y,
                        })
                }
                _ => None,
            },
            VMM_EVT_CLIPBOARD => {
                let len = word(1);
                (buf[1..4] == [0; 3]
                    && (1..=super::VMM_CLIPBOARD_MAX).contains(&len)
                    && word(2) == 0
                    && word(3) == 0)
                    .then_some(Self::Clipboard {
                        grant: word(0),
                        len,
                    })
            }
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
            VmmEvent::Pointer {
                grant: 9,
                width: 24,
                height: 32,
                hot_x: 5,
                hot_y: 31,
            },
            VmmEvent::PointerHidden,
            VmmEvent::Clipboard {
                grant: 11,
                len: 65536,
            },
        ] {
            assert_eq!(VmmEvent::decode(&event.encode()), Some(event));
        }
        let mut frame = [0u8; VmmEvent::LEN];
        frame[0] = super::super::VMM_OP_LAUNCH_APP;
        assert_eq!(VmmEvent::decode(&frame), None);
        assert_eq!(VmmEvent::decode(&frame[..VmmEvent::LEN - 1]), None);
        // A cursor larger than the hardware plane, or a hot spot outside it.
        for (size, hot) in [(65 | 8 << 16, 0), (8 | 8 << 16, 8)] {
            let mut frame = VmmEvent::PointerHidden.encode();
            frame[1] = 0;
            frame[8..12].copy_from_slice(&(size as u32).to_le_bytes());
            frame[12..16].copy_from_slice(&(hot as u32).to_le_bytes());
            assert_eq!(VmmEvent::decode(&frame), None);
        }
        // Clipboard text that is empty or over the limit.
        for len in [0, super::super::VMM_CLIPBOARD_MAX + 1] {
            let mut frame = VmmEvent::Clipboard { grant: 1, len: 1 }.encode();
            frame[8..12].copy_from_slice(&len.to_le_bytes());
            assert_eq!(VmmEvent::decode(&frame), None);
        }
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
