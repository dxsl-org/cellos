/// A closed-profile parsing, verification, bounds, or output failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    OutputTooSmall,
    ScratchTooSmall,
    Truncated,
    TrailingData,
    WrongType,
    NonCanonical,
    WrongSchema,
    WrongLane,
    WrongIdentity,
    WrongFreshness,
    UnknownKey,
    WrongComponent,
    ZeroLength,
    Overflow,
    LimitExceeded,
    RangeOverlap,
    WrongEntry,
    WrongRegionLength,
    InvalidCose,
    WrongAlgorithm,
    WrongKeyId,
    InvalidPublicKey,
    InvalidSeed,
    Signature,
    DigestMismatch,
    InvalidFrame,
    InvalidBlock,
    InvalidCrc,
    InvalidPadding,
    MissingEot,
    InvalidStaging,
}

/// Result type used by every bounded operation in this crate.
pub type Result<T> = core::result::Result<T, Error>;
