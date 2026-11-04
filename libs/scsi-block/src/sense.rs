//! Sense data decoding (SPC-4 §4.5) and retry classification.

/// Sense key values (SPC-4 table 49); the key is the low nibble.
pub mod sense_key {
    /// No specific sense key information.
    pub const NO_SENSE: u8 = 0x0;
    /// Command completed after device-side recovery.
    pub const RECOVERED_ERROR: u8 = 0x1;
    /// Logical unit not accessible.
    pub const NOT_READY: u8 = 0x2;
    /// Unrecovered media flaw.
    pub const MEDIUM_ERROR: u8 = 0x3;
    /// Non-recoverable device failure.
    pub const HARDWARE_ERROR: u8 = 0x4;
    /// Bad CDB field, opcode, or LBA range.
    pub const ILLEGAL_REQUEST: u8 = 0x5;
    /// Reset, media change, or parameter change since the last command.
    pub const UNIT_ATTENTION: u8 = 0x6;
    /// Write-protected medium.
    pub const DATA_PROTECT: u8 = 0x7;
    /// Command aborted by the device server.
    pub const ABORTED_COMMAND: u8 = 0xB;
}

/// ASC 0x04 "logical unit not ready".
const ASC_NOT_READY: u8 = 0x04;
/// ASCQ 0x01 under ASC 0x04: "in process of becoming ready".
const ASCQ_BECOMING_READY: u8 = 0x01;

/// Decoded sense: the three fields the disk layer acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sense {
    /// Sense key (low nibble only; see [`sense_key`]).
    pub key: u8,
    /// Additional sense code; 0 when the device did not supply one.
    pub asc: u8,
    /// Additional sense code qualifier; 0 when the device did not supply one.
    pub ascq: u8,
}

/// How a caller should react to a CHECK CONDITION carrying a given [`Sense`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SenseClass {
    /// Transient: reissuing the same command may succeed (UNIT ATTENTION, or
    /// NOT READY / "becoming ready").
    Retry,
    /// Reissuing the same command will not help.
    Fatal,
}

impl Sense {
    /// Decodes fixed-format (response code 0x70/0x71) or descriptor-format
    /// (0x72/0x73) sense data.
    ///
    /// Returns `None` when the response code is unknown or `bytes` is too
    /// short to hold the sense key (3 bytes fixed, 2 bytes descriptor). In
    /// fixed format, ASC/ASCQ are taken only when both the buffer and the
    /// ADDITIONAL SENSE LENGTH (byte 7) cover bytes 12–13; otherwise they read
    /// as 0. Descriptor format needs 4 bytes for ASC/ASCQ, with the same
    /// fallback.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        match bytes.first()? & 0x7F {
            0x70 | 0x71 => Self::decode_fixed(bytes),
            0x72 | 0x73 => Self::decode_descriptor(bytes),
            _ => None,
        }
    }

    fn decode_fixed(bytes: &[u8]) -> Option<Self> {
        let key = bytes.get(2)? & 0x0F;
        // Bytes 0..8 are always present in a well-formed record; byte 7 counts
        // what follows it, so ASC/ASCQ (12, 13) need an additional length >= 6.
        let covered = bytes.get(7).is_some_and(|&add| usize::from(add) >= 6);
        let (asc, ascq) = match (covered, bytes.get(12), bytes.get(13)) {
            (true, Some(&asc), Some(&ascq)) => (asc, ascq),
            _ => (0, 0),
        };
        Some(Self { key, asc, ascq })
    }

    fn decode_descriptor(bytes: &[u8]) -> Option<Self> {
        let key = bytes.get(1)? & 0x0F;
        let (asc, ascq) = match (bytes.get(2), bytes.get(3)) {
            (Some(&asc), Some(&ascq)) => (asc, ascq),
            _ => (0, 0),
        };
        Some(Self { key, asc, ascq })
    }

    /// Classifies this sense for retry decisions.
    ///
    /// [`SenseClass::Retry`]: UNIT ATTENTION, and NOT READY with ASC/ASCQ
    /// 04h/01h ("becoming ready"). Everything else — including MEDIUM ERROR,
    /// HARDWARE ERROR, ILLEGAL REQUEST, DATA PROTECT, and NOT READY for any
    /// other reason (no medium, manual intervention) — is
    /// [`SenseClass::Fatal`].
    pub fn class(&self) -> SenseClass {
        match (self.key, self.asc, self.ascq) {
            (sense_key::UNIT_ATTENTION, _, _) => SenseClass::Retry,
            (sense_key::NOT_READY, ASC_NOT_READY, ASCQ_BECOMING_READY) => SenseClass::Retry,
            _ => SenseClass::Fatal,
        }
    }
}
