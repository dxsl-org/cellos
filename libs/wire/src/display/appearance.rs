//! System appearance: the shell tells the compositor which palette to use
//! ([`SetAppearance`]); the compositor tells every surface owner
//! ([`AppearanceChanged`]) so toolkits draw with the same palette.

use super::{compositor_events, compositor_ops, DisplayProtocolError};

/// Exact wire size of [`SetAppearance`].
pub const SET_APPEARANCE_WIRE_SIZE: usize = 12;

/// System palette of compositor-drawn chrome. Values match
/// `viui::theme::SystemPalette`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppearancePalette {
    #[default]
    Light = 0,
    Dark = 1,
    Paper = 2,
}

impl AppearancePalette {
    fn decode(value: u8) -> Result<Self, DisplayProtocolError> {
        match value {
            0 => Ok(Self::Light),
            1 => Ok(Self::Dark),
            2 => Ok(Self::Paper),
            _ => Err(DisplayProtocolError::InvalidDiscriminant),
        }
    }
}

/// Fixed 12-byte request that sets the chrome palette.
///
/// LE layout: `opcode:u8, palette:u8, reserved:[u8;2], cap:u32, revision:u32`.
/// Accepted only when `cap` is a Panel surface owned by the sender (the system
/// shell). Revisions are scoped to that owner: a request from the same owner
/// must carry a revision greater than the last accepted one; the first request
/// from a new Panel owner is accepted whatever its revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetAppearance {
    pub palette: AppearancePalette,
    /// Panel surface capability owned by the sender.
    pub cap: u32,
    pub revision: u32,
}

impl SetAppearance {
    pub const fn new(cap: u32, palette: AppearancePalette, revision: u32) -> Self {
        Self {
            palette,
            cap,
            revision,
        }
    }

    /// Encode into the exact 12-byte little-endian wire frame.
    pub fn encode(&self) -> [u8; SET_APPEARANCE_WIRE_SIZE] {
        let mut bytes = [0; SET_APPEARANCE_WIRE_SIZE];
        bytes[0] = compositor_ops::SET_APPEARANCE;
        bytes[1] = self.palette as u8;
        bytes[4..8].copy_from_slice(&self.cap.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.revision.to_le_bytes());
        bytes
    }

    /// Decode an exact 12-byte frame.
    ///
    /// # Errors
    /// Wrong length or opcode, nonzero reserved bytes, or an unknown palette.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        if bytes.len() != SET_APPEARANCE_WIRE_SIZE {
            return Err(DisplayProtocolError::InvalidLength);
        }
        if bytes[0] != compositor_ops::SET_APPEARANCE {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if bytes[2..4] != [0, 0] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        Ok(Self {
            palette: AppearancePalette::decode(bytes[1])?,
            cap: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            revision: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        })
    }
}

/// Exact wire size of [`AppearanceChanged`].
pub const APPEARANCE_CHANGED_WIRE_SIZE: usize = 8;

/// Fixed 8-byte compositor event announcing the system palette.
///
/// LE layout: `opcode:u8 (0xA3), palette:u8, reserved:[u8;2], generation:u32`.
/// Sent to every surface owner when the palette changes, and to a client right
/// after each successful `ATTACH_GRANT` reply so a new window starts in the
/// current palette. Frames from one compositor arrive in order, so the latest
/// frame wins. `generation` counts compositor palette changes from 1 and is
/// never 0: a `CREATE_SURFACE` reply (`cap:u32` then four zero bytes) can
/// therefore never be mistaken for this event, whatever the cap value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppearanceChanged {
    pub palette: AppearancePalette,
    pub generation: u32,
}

impl AppearanceChanged {
    pub fn encode(&self) -> [u8; APPEARANCE_CHANGED_WIRE_SIZE] {
        let mut bytes = [0; APPEARANCE_CHANGED_WIRE_SIZE];
        bytes[0] = compositor_events::APPEARANCE_CHANGED;
        bytes[1] = self.palette as u8;
        bytes[4..8].copy_from_slice(&self.generation.to_le_bytes());
        bytes
    }

    /// Decode the first 8 bytes of `bytes` (receive buffers are longer).
    ///
    /// # Errors
    /// Short buffer, wrong opcode, nonzero reserved bytes, generation 0 or an
    /// unknown palette.
    pub fn decode(bytes: &[u8]) -> Result<Self, DisplayProtocolError> {
        let Some(bytes) = bytes.get(..APPEARANCE_CHANGED_WIRE_SIZE) else {
            return Err(DisplayProtocolError::InvalidLength);
        };
        if bytes[0] != compositor_events::APPEARANCE_CHANGED {
            return Err(DisplayProtocolError::InvalidOpcode);
        }
        if bytes[2..4] != [0, 0] {
            return Err(DisplayProtocolError::NonZeroReserved);
        }
        let generation = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if generation == 0 {
            return Err(DisplayProtocolError::InvalidDiscriminant);
        }
        Ok(Self {
            palette: AppearancePalette::decode(bytes[1])?,
            generation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trips_and_refuses_malformed_bytes() {
        let frame = SetAppearance::new(7, AppearancePalette::Paper, 0x0102_0304).encode();
        assert_eq!(frame, [0x12, 2, 0, 0, 7, 0, 0, 0, 4, 3, 2, 1]);
        assert_eq!(
            SetAppearance::decode(&frame),
            Ok(SetAppearance::new(7, AppearancePalette::Paper, 0x0102_0304))
        );

        let refused = |mutate: fn(&mut [u8; 12])| {
            let mut bytes = frame;
            mutate(&mut bytes);
            SetAppearance::decode(&bytes)
        };
        assert_eq!(
            refused(|b| b[0] = 0x0A),
            Err(DisplayProtocolError::InvalidOpcode)
        );
        assert_eq!(
            refused(|b| b[1] = 3),
            Err(DisplayProtocolError::InvalidDiscriminant)
        );
        assert_eq!(
            refused(|b| b[3] = 1),
            Err(DisplayProtocolError::NonZeroReserved)
        );
        assert_eq!(
            SetAppearance::decode(&frame[..11]),
            Err(DisplayProtocolError::InvalidLength)
        );
    }

    #[test]
    fn appearance_event_never_reads_as_a_create_reply() {
        let event = AppearanceChanged {
            palette: AppearancePalette::Dark,
            generation: 3,
        };
        let frame = event.encode();
        assert_eq!(frame, [0xA3, 1, 0, 0, 3, 0, 0, 0]);
        assert_eq!(AppearanceChanged::decode(&frame), Ok(event));
        // CREATE_SURFACE reply for cap 0xA3 with Dark-looking second byte: cap:u32 + 4 zero bytes.
        assert_eq!(
            AppearanceChanged::decode(&[0xA3, 1, 0, 0, 0, 0, 0, 0]),
            Err(DisplayProtocolError::InvalidDiscriminant)
        );
        assert_eq!(
            AppearanceChanged::decode(&[0xA3, 9, 0, 0, 1, 0, 0, 0]),
            Err(DisplayProtocolError::InvalidDiscriminant)
        );
        assert_eq!(
            AppearanceChanged::decode(&frame[..7]),
            Err(DisplayProtocolError::InvalidLength)
        );
    }
}
