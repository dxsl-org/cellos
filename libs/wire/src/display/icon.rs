//! Window icon request: the app names the icon its Cell Tab shows.

use super::{compositor_ops, DisplayProtocolError};

/// Exact wire size of [`SetIcon`].
pub const SET_ICON_WIRE_SIZE: usize = 8;

/// Fixed 8-byte owner request that sets the Cell Tab icon of a surface.
///
/// LE layout: `opcode:u8, icon:u8, reserved:[u8;2], cap:u32`. `icon` is a
/// `ostd::display::linear::icons::LineIcon` wire value; the compositor refuses
/// a value it does not know and a `cap` the sender does not own. A surface that
/// never sends one shows the generic application icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetIcon {
    pub icon: u8,
    pub cap: u32,
}

impl SetIcon {
    pub fn encode(&self) -> [u8; SET_ICON_WIRE_SIZE] {
        let mut bytes = [0; SET_ICON_WIRE_SIZE];
        bytes[0] = compositor_ops::SET_ICON;
        bytes[1] = self.icon;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes
    }

    /// Decode the first 8 bytes of `bytes`.
    ///
    /// # Errors
    /// Short buffer, wrong opcode or nonzero reserved bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        let Some(bytes) = bytes.get(..SET_ICON_WIRE_SIZE) else {
            return Err(DisplayProtocolError::InvalidLength);
        };
        if bytes[0] != compositor_ops::SET_ICON {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if bytes[2..4] != [0, 0] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        Ok(Self {
            icon: bytes[1],
            cap: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trips_and_refuses_malformed_bytes() {
        let frame = SetIcon {
            icon: 12,
            cap: 0x0102_0304,
        }
        .encode();
        assert_eq!(frame, [0x13, 12, 0, 0, 4, 3, 2, 1]);
        assert_eq!(
            SetIcon::decode(&frame),
            Ok(SetIcon {
                icon: 12,
                cap: 0x0102_0304
            })
        );
        assert_eq!(
            SetIcon::decode(&frame[..7]),
            Err(DisplayProtocolError::InvalidLength)
        );
        let mut bad = frame;
        bad[2] = 1;
        assert_eq!(
            SetIcon::decode(&bad),
            Err(DisplayProtocolError::NonZeroReserved)
        );
        bad = frame;
        bad[0] = 0x0A;
        assert_eq!(
            SetIcon::decode(&bad),
            Err(DisplayProtocolError::InvalidOpcode)
        );
    }
}
