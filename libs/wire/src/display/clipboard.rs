//! The text clipboard the compositor holds for every app.
//!
//! Only the owner of the surface with keyboard focus may set it or ask for it,
//! so an app in the background can neither read what the user copied nor plant
//! text. Text travels in Grants: a request frame is 512 bytes at most.
//!
//! - [`ClipboardSet`]: the sender's Grant holds `len` bytes of UTF-8 text; the
//!   compositor copies them on receipt (owner-checked).
//! - [`ClipboardGet`]: ask for the current text; answered with
//!   [`ClipboardChanged`] when `cap` is the sender's and has focus, otherwise
//!   not at all.
//! - [`ClipboardChanged`]: the compositor's Grant (shared read-only with the
//!   receiver) holds the current text. Sent to the focused surface's owner when
//!   the text changed since that surface last saw it, and in answer to a get.

use super::{compositor_events, compositor_ops, DisplayProtocolError};

/// Largest clipboard text in bytes (the Terminal's paste limit).
pub const CLIPBOARD_MAX: usize = crate::session::PASTE_MAX;
/// Exact wire size of [`ClipboardSet`].
pub const CLIPBOARD_SET_WIRE_SIZE: usize = 16;
/// Exact wire size of [`ClipboardGet`].
pub const CLIPBOARD_GET_WIRE_SIZE: usize = 8;
/// Exact wire size of [`ClipboardChanged`].
pub const CLIPBOARD_CHANGED_WIRE_SIZE: usize = 20;

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// The first `size` bytes of `bytes`, which must start with `opcode` and have
/// zero reserved bytes 1..4.
fn frame(bytes: &[u8], size: usize, opcode: u8) -> Result<&[u8], DisplayProtocolError> {
    let bytes = bytes
        .get(..size)
        .ok_or(DisplayProtocolError::InvalidLength)?;
    if bytes[0] != opcode {
        return Err(DisplayProtocolError::InvalidOpcode);
    }
    if bytes[1..4] != [0; 3] {
        return Err(DisplayProtocolError::NonZeroReserved);
    }
    Ok(bytes)
}

/// LE layout: `opcode:u8, reserved:[u8;3], cap:u32, grant:u32, len:u32`.
/// `len` is 1..=[`CLIPBOARD_MAX`]; the text is at the start of `grant`, a
/// Grant the sender owns and shares with the compositor. No reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardSet {
    pub cap: u32,
    pub grant: u32,
    pub len: u32,
}

impl ClipboardSet {
    pub fn encode(&self) -> [u8; CLIPBOARD_SET_WIRE_SIZE] {
        let mut bytes = [0; CLIPBOARD_SET_WIRE_SIZE];
        bytes[0] = compositor_ops::CLIPBOARD_SET;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.grant.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.len.to_le_bytes());
        bytes
    }

    /// # Errors
    /// Short buffer, wrong opcode, nonzero reserved bytes, or `len` outside
    /// 1..=[`CLIPBOARD_MAX`] (`OutOfRange`).
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        let bytes = frame(
            bytes,
            CLIPBOARD_SET_WIRE_SIZE,
            compositor_ops::CLIPBOARD_SET,
        )?;
        let request = Self {
            cap: le32(bytes, 4),
            grant: le32(bytes, 8),
            len: le32(bytes, 12),
        };
        if !(1..=CLIPBOARD_MAX).contains(&(request.len as usize)) {
            return Err(DisplayProtocolError::OutOfRange);
        }
        Ok(request)
    }
}

/// LE layout: `opcode:u8, reserved:[u8;3], cap:u32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardGet {
    pub cap: u32,
}

impl ClipboardGet {
    pub fn encode(&self) -> [u8; CLIPBOARD_GET_WIRE_SIZE] {
        let mut bytes = [0; CLIPBOARD_GET_WIRE_SIZE];
        bytes[0] = compositor_ops::CLIPBOARD_GET;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes
    }

    /// # Errors
    /// Short buffer, wrong opcode or nonzero reserved bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        let bytes = frame(
            bytes,
            CLIPBOARD_GET_WIRE_SIZE,
            compositor_ops::CLIPBOARD_GET,
        )?;
        Ok(Self {
            cap: le32(bytes, 4),
        })
    }
}

/// LE layout: `opcode:u8, reserved:[u8;3], cap:u32, grant:u32, len:u32,
/// serial:u32`. `cap` is the receiver's focused surface; the text is the first
/// `len` bytes of `grant` (owned by the compositor), `len` at most
/// [`CLIPBOARD_MAX`], and `grant` 0 exactly when `len` is 0 (nothing copied
/// yet). `serial` grows by one with each change of the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardChanged {
    pub cap: u32,
    pub grant: u32,
    pub len: u32,
    pub serial: u32,
}

impl ClipboardChanged {
    pub fn encode(&self) -> [u8; CLIPBOARD_CHANGED_WIRE_SIZE] {
        let mut bytes = [0; CLIPBOARD_CHANGED_WIRE_SIZE];
        bytes[0] = compositor_events::CLIPBOARD_CHANGED;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.grant.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.len.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.serial.to_le_bytes());
        bytes
    }

    /// # Errors
    /// Short buffer, wrong opcode, nonzero reserved bytes, a `len` over
    /// [`CLIPBOARD_MAX`] or a `grant` that disagrees with `len` (`OutOfRange`).
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        let bytes = frame(
            bytes,
            CLIPBOARD_CHANGED_WIRE_SIZE,
            compositor_events::CLIPBOARD_CHANGED,
        )?;
        let event = Self {
            cap: le32(bytes, 4),
            grant: le32(bytes, 8),
            len: le32(bytes, 12),
            serial: le32(bytes, 16),
        };
        if event.len as usize > CLIPBOARD_MAX || (event.len == 0) != (event.grant == 0) {
            return Err(DisplayProtocolError::OutOfRange);
        }
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let set = ClipboardSet {
            cap: 3,
            grant: 0x9000,
            len: 5,
        };
        assert_eq!(ClipboardSet::decode(&set.encode()), Ok(set));
        let get = ClipboardGet { cap: 3 };
        assert_eq!(ClipboardGet::decode(&get.encode()), Ok(get));
        for (grant, len) in [(0x9000, 70), (0, 0)] {
            let event = ClipboardChanged {
                cap: 3,
                grant,
                len,
                serial: 2,
            };
            assert_eq!(ClipboardChanged::decode(&event.encode()), Ok(event));
        }
    }

    #[test]
    fn lengths_outside_the_limit_and_reserved_bytes_are_refused() {
        for len in [0, CLIPBOARD_MAX as u32 + 1] {
            let set = ClipboardSet {
                cap: 1,
                grant: 1,
                len,
            };
            assert_eq!(
                ClipboardSet::decode(&set.encode()),
                Err(DisplayProtocolError::OutOfRange)
            );
        }
        let at_limit = ClipboardSet {
            cap: 1,
            grant: 1,
            len: CLIPBOARD_MAX as u32,
        };
        assert!(ClipboardSet::decode(&at_limit.encode()).is_ok());
        for (grant, len) in [(0, 4), (8, 0)] {
            let event = ClipboardChanged {
                cap: 1,
                grant,
                len,
                serial: 1,
            };
            assert_eq!(
                ClipboardChanged::decode(&event.encode()),
                Err(DisplayProtocolError::OutOfRange)
            );
        }
        let mut get = ClipboardGet { cap: 1 }.encode();
        get[2] = 1;
        assert_eq!(
            ClipboardGet::decode(&get),
            Err(DisplayProtocolError::NonZeroReserved)
        );
        assert_eq!(
            ClipboardGet::decode(&get[..7]),
            Err(DisplayProtocolError::InvalidLength)
        );
        assert_eq!(
            ClipboardSet::decode(&ClipboardGet { cap: 1 }.encode()),
            Err(DisplayProtocolError::InvalidLength)
        );
    }
}
