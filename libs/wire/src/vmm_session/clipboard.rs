// SPDX-License-Identifier: Apache-2.0
//! Clipboard text between the launcher and the guest.
//!
//! Launcher -> hypervisor: [`VmmClipboardRequest`] hands the guest the host's
//! clipboard text. Hypervisor -> launcher: `VmmEvent::Clipboard` carries what
//! the guest copied. Either way the text is UTF-8 at the start of a Grant the
//! sender owns and shares read-only with the receiver, which copies it with an
//! owner check before using it.

use types::{ViError, ViResult};

use super::VMM_OP_CLIPBOARD;

/// Largest clipboard text in bytes, either way.
pub const VMM_CLIPBOARD_MAX: u32 = crate::display::CLIPBOARD_MAX as u32;

/// LE layout: `opcode:u8, reserved:[u8;3], session_id:u32, grant:u32,
/// len:u32`, `len` 1..=[`VMM_CLIPBOARD_MAX`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmmClipboardRequest {
    pub session_id: u32,
    pub grant: u32,
    pub len: u32,
}

impl VmmClipboardRequest {
    pub const LEN: usize = 16;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = VMM_OP_CLIPBOARD;
        out[4..8].copy_from_slice(&self.session_id.to_le_bytes());
        out[8..12].copy_from_slice(&self.grant.to_le_bytes());
        out[12..16].copy_from_slice(&self.len.to_le_bytes());
        out
    }

    /// # Errors
    /// `InvalidInput` for a short frame, another opcode, nonzero reserved
    /// bytes or a length outside 1..=[`VMM_CLIPBOARD_MAX`].
    pub fn decode(buf: &[u8]) -> ViResult<Self> {
        let le32 = |at: usize| u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]);
        if buf.len() < Self::LEN || buf[0] != VMM_OP_CLIPBOARD || buf[1..4] != [0; 3] {
            return Err(ViError::InvalidInput);
        }
        let request = Self {
            session_id: le32(4),
            grant: le32(8),
            len: le32(12),
        };
        if !(1..=VMM_CLIPBOARD_MAX).contains(&request.len) {
            return Err(ViError::InvalidInput);
        }
        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_refuses_bad_lengths_and_reserved_bytes() {
        let request = VmmClipboardRequest {
            session_id: 1,
            grant: 0x4000,
            len: 12,
        };
        assert_eq!(VmmClipboardRequest::decode(&request.encode()), Ok(request));
        for len in [0, VMM_CLIPBOARD_MAX + 1] {
            let frame = VmmClipboardRequest { len, ..request }.encode();
            assert!(VmmClipboardRequest::decode(&frame).is_err());
        }
        let mut frame = request.encode();
        frame[3] = 1;
        assert!(VmmClipboardRequest::decode(&frame).is_err());
        assert!(VmmClipboardRequest::decode(&request.encode()[..15]).is_err());
    }
}
