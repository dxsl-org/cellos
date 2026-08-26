//! Stable fail-closed faults. Values are wire ABI and never carry text.

/// Closed authority failure set.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityFault {
    Malformed = 1,
    UnsupportedVersion = 2,
    InvalidLength = 3,
    InvalidState = 4,
    IdentityMismatch = 5,
    ChallengeMismatch = 6,
    StaleRequest = 7,
    Replay = 8,
    Regression = 9,
    TimeInvalid = 10,
    TimeUnavailable = 11,
    ProfileRejected = 12,
    ReceiptAbsent = 13,
    ReceiptConsumed = 14,
    ProviderSplitBrain = 15,
    PersistenceFailure = 16,
    Sealed = 17,
}

impl TryFrom<u16> for AuthorityFault {
    type Error = u16;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        use AuthorityFault::*;
        Ok(match value {
            1 => Malformed,
            2 => UnsupportedVersion,
            3 => InvalidLength,
            4 => InvalidState,
            5 => IdentityMismatch,
            6 => ChallengeMismatch,
            7 => StaleRequest,
            8 => Replay,
            9 => Regression,
            10 => TimeInvalid,
            11 => TimeUnavailable,
            12 => ProfileRejected,
            13 => ReceiptAbsent,
            14 => ReceiptConsumed,
            15 => ProviderSplitBrain,
            16 => PersistenceFailure,
            17 => Sealed,
            _ => return Err(value),
        })
    }
}
