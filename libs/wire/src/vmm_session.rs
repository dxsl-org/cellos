// SPDX-License-Identifier: Apache-2.0
//! Tier 3 Linux MicroVM App Session IPC contract.
//!
//! Governs communication between a user-facing app launcher (`cells/apps/chrome`)
//! and the hypervisor Cell it spawned (`/bin/hypervisor app`).
//!
//! The launcher owns the window. It sends [`VmmLaunchRequest`] with the window's
//! content size; the hypervisor binds the session to that sender, boots the guest
//! at that display size, and from then on posts [`events`] to the launcher: the
//! guest scanout Grant (shared read-only), damaged rectangles, and the guest's exit.
//! The launcher forwards its window's input with [`VmmInputForwardRequest`], asks
//! for a new display size with [`VmmResizeRequest`], and ends the guest with
//! `VMM_OP_TERMINATE_APP`. Only the bound launcher may do any of these.

use types::{ViError, ViResult};

pub mod clipboard;
pub mod events;
pub use clipboard::*;
pub use events::*;

pub const VMM_OP_LAUNCH_APP: u8 = 1;
pub const VMM_OP_RESIZE_APP: u8 = 2;
pub const VMM_OP_FORWARD_INPUT: u8 = 3;
pub const VMM_OP_QUERY_STATUS: u8 = 4;
pub const VMM_OP_TERMINATE_APP: u8 = 5;
/// [`VmmClipboardRequest`]: the host's clipboard text for the guest.
pub const VMM_OP_CLIPBOARD: u8 = 6;

pub const VMM_STATUS_OK: u32 = 0;
pub const VMM_STATUS_INVALID_ARG: u32 = 1;
pub const VMM_STATUS_NOT_FOUND: u32 = 2;
pub const VMM_STATUS_BUSY: u32 = 3;
pub const VMM_STATUS_FAILED: u32 = 4;
pub const VMM_STATUS_UNSUPPORTED: u32 = 5;

/// Linux evdev event types and codes carried by [`VmmInputForwardRequest`].
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const EV_ABS: u16 = 0x03;
pub const REL_X: u16 = 0x00;
pub const REL_Y: u16 = 0x01;
pub const REL_WHEEL: u16 = 0x08;
pub const ABS_X: u16 = 0x00;
pub const ABS_Y: u16 = 0x01;
/// The guest pointer's absolute axis range: `0..=ABS_MAX` spans the display.
pub const ABS_MAX: i32 = 0x7fff;

/// `pos` within `0..extent` pixels as a guest absolute-axis value, clamped to
/// the axis; an empty extent maps everything to 0.
pub fn abs_position(pos: i32, extent: u32) -> i32 {
    if extent <= 1 {
        return 0;
    }
    let pos = i64::from(pos.clamp(0, (extent - 1) as i32));
    (pos * i64::from(ABS_MAX) / i64::from(extent - 1)) as i32
}
pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;
pub const BTN_SIDE: u16 = 0x113;
pub const BTN_EXTRA: u16 = 0x114;

/// Request payload sent by app runner to start a single-app guest container.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmmLaunchRequest {
    pub opcode: u8,
    pub reserved: [u8; 3],
    pub width: u32,
    pub height: u32,
    pub app_name: [u8; 32],
}

impl VmmLaunchRequest {
    pub const LEN: usize = 44;

    /// Build a launch request; `name` is truncated to 31 bytes on a UTF-8
    /// boundary so the NUL-terminated field always decodes as text.
    pub fn new(name: &str, width: u32, height: u32) -> Self {
        let mut app_name = [0u8; 32];
        let mut copy_len = name.len().min(31);
        while !name.is_char_boundary(copy_len) {
            copy_len -= 1;
        }
        app_name[..copy_len].copy_from_slice(&name.as_bytes()[..copy_len]);

        Self {
            opcode: VMM_OP_LAUNCH_APP,
            reserved: [0; 3],
            width,
            height,
            app_name,
        }
    }

    pub fn encode(&self, out: &mut [u8]) -> ViResult<usize> {
        if out.len() < Self::LEN {
            return Err(ViError::InvalidArgument);
        }
        out[0] = self.opcode;
        out[1..4].copy_from_slice(&self.reserved);
        out[4..8].copy_from_slice(&self.width.to_le_bytes());
        out[8..12].copy_from_slice(&self.height.to_le_bytes());
        out[12..44].copy_from_slice(&self.app_name);
        Ok(Self::LEN)
    }

    pub fn decode(buf: &[u8]) -> ViResult<Self> {
        if buf.len() < Self::LEN || buf[0] != VMM_OP_LAUNCH_APP {
            return Err(ViError::InvalidInput);
        }
        let width = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let height = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let mut app_name = [0u8; 32];
        app_name.copy_from_slice(&buf[12..44]);

        Ok(Self {
            opcode: buf[0],
            reserved: [buf[1], buf[2], buf[3]],
            width,
            height,
            app_name,
        })
    }
}

/// Reply payload returned by the hypervisor upon app launch. The display size
/// the guest boots with; the scanout Grant follows as a [`VmmScanoutEvent`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmmLaunchReply {
    pub status: u32,
    pub session_id: u32,
    pub reserved: u32,
    pub scanout_width: u32,
    pub scanout_height: u32,
}

impl VmmLaunchReply {
    pub const LEN: usize = 20;

    pub fn encode(&self, out: &mut [u8]) -> ViResult<usize> {
        if out.len() < Self::LEN {
            return Err(ViError::InvalidArgument);
        }
        out[0..4].copy_from_slice(&self.status.to_le_bytes());
        out[4..8].copy_from_slice(&self.session_id.to_le_bytes());
        out[8..12].copy_from_slice(&self.reserved.to_le_bytes());
        out[12..16].copy_from_slice(&self.scanout_width.to_le_bytes());
        out[16..20].copy_from_slice(&self.scanout_height.to_le_bytes());
        Ok(Self::LEN)
    }

    pub fn decode(buf: &[u8]) -> ViResult<Self> {
        if buf.len() < Self::LEN {
            return Err(ViError::InvalidInput);
        }
        Ok(Self {
            status: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            session_id: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            reserved: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            scanout_width: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
            scanout_height: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
        })
    }
}

/// Request to forward a native host input event into guest VirtIO-Input.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmmInputForwardRequest {
    pub opcode: u8,
    pub reserved: u8,
    pub event_type: u16,
    pub session_id: u32,
    pub code: u16,
    pub pad: u16,
    pub value: i32,
}

impl VmmInputForwardRequest {
    pub const LEN: usize = 16;

    pub fn new(session_id: u32, event_type: u16, code: u16, value: i32) -> Self {
        Self {
            opcode: VMM_OP_FORWARD_INPUT,
            reserved: 0,
            event_type,
            session_id,
            code,
            pad: 0,
            value,
        }
    }

    pub fn encode(&self, out: &mut [u8]) -> ViResult<usize> {
        if out.len() < Self::LEN {
            return Err(ViError::InvalidArgument);
        }
        out[0] = self.opcode;
        out[1] = self.reserved;
        out[2..4].copy_from_slice(&self.event_type.to_le_bytes());
        out[4..8].copy_from_slice(&self.session_id.to_le_bytes());
        out[8..10].copy_from_slice(&self.code.to_le_bytes());
        out[10..12].copy_from_slice(&self.pad.to_le_bytes());
        out[12..16].copy_from_slice(&self.value.to_le_bytes());
        Ok(Self::LEN)
    }

    pub fn decode(buf: &[u8]) -> ViResult<Self> {
        if buf.len() < Self::LEN || buf[0] != VMM_OP_FORWARD_INPUT {
            return Err(ViError::InvalidInput);
        }
        Ok(Self {
            opcode: buf[0],
            reserved: buf[1],
            event_type: u16::from_le_bytes([buf[2], buf[3]]),
            session_id: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            code: u16::from_le_bytes([buf[8], buf[9]]),
            pad: u16::from_le_bytes([buf[10], buf[11]]),
            value: i32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_request_encode_decode() {
        let req = VmmLaunchRequest::new("chrome", 1280, 800);
        let mut buf = [0u8; 64];
        let len = req.encode(&mut buf).unwrap();
        assert_eq!(len, VmmLaunchRequest::LEN);

        let decoded = VmmLaunchRequest::decode(&buf[..len]).unwrap();
        assert_eq!(decoded.opcode, VMM_OP_LAUNCH_APP);
        assert_eq!(decoded.width, 1280);
        assert_eq!(decoded.height, 800);
        assert_eq!(&decoded.app_name[..6], b"chrome");
        assert_eq!(decoded.app_name[6], 0);
    }

    #[test]
    fn launch_reply_encode_decode() {
        let reply = VmmLaunchReply {
            status: VMM_STATUS_OK,
            session_id: 42,
            reserved: 0,
            scanout_width: 1280,
            scanout_height: 800,
        };
        let mut buf = [0u8; 32];
        let len = reply.encode(&mut buf).unwrap();
        assert_eq!(len, VmmLaunchReply::LEN);

        let decoded = VmmLaunchReply::decode(&buf[..len]).unwrap();
        assert_eq!(decoded, reply);
    }

    #[test]
    fn input_forward_request_encode_decode() {
        let req = VmmInputForwardRequest::new(42, 1, 30, 1); // EV_KEY, KEY_A, Pressed
        let mut buf = [0u8; 32];
        let len = req.encode(&mut buf).unwrap();
        assert_eq!(len, VmmInputForwardRequest::LEN);

        let decoded = VmmInputForwardRequest::decode(&buf[..len]).unwrap();
        assert_eq!(decoded, req);
    }

    #[test]
    fn absolute_positions_span_the_axis_and_clamp() {
        assert_eq!(abs_position(0, 1280), 0);
        assert_eq!(abs_position(1279, 1280), ABS_MAX);
        assert_eq!(abs_position(640, 1281), ABS_MAX / 2);
        assert_eq!(abs_position(-5, 1280), 0);
        assert_eq!(abs_position(5000, 1280), ABS_MAX);
        assert_eq!(abs_position(3, 0), 0);
    }

    #[test]
    fn launch_request_truncates_name_on_char_boundary() {
        // 16 two-byte chars = 32 bytes; a byte cut at 31 would split the last one.
        let req = VmmLaunchRequest::new("éééééééééééééééé", 1, 1);
        let len = req.app_name.iter().position(|&b| b == 0).unwrap();
        assert_eq!(len, 30);
        assert!(core::str::from_utf8(&req.app_name[..len]).is_ok());
    }
}
