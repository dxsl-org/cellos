// SPDX-License-Identifier: MPL-2.0
//! Shared Cell-to-Cell protocol value types.

/// Retry safety declared by a typed remote method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RetryClass {
    Idempotent = 1,
    Conditional = 2,
    Never = 3,
}

impl RetryClass {
    /// Decode the canonical V1 wire value.
    pub const fn from_wire(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Idempotent,
            2 => Self::Conditional,
            3 => Self::Never,
            _ => return None,
        })
    }
}

/// Nonzero identity of one live exported-server incarnation.
///
/// Phase 04 scopes this value to one net-broker incarnation. A successful
/// server restart receives a fresh value; a request carrying an older value
/// must become `Indeterminate` before deduplication or local dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ServerEpoch(u64);

impl ServerEpoch {
    /// Construct an epoch, rejecting the reserved zero value.
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Return the canonical nonzero wire value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Nonzero caller-supplied relative deadline in monotonic milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct RelativeDeadline(u32);

impl RelativeDeadline {
    /// Construct a deadline budget, rejecting the reserved zero value.
    pub const fn new(milliseconds: u32) -> Option<Self> {
        if milliseconds == 0 {
            None
        } else {
            Some(Self(milliseconds))
        }
    }

    /// Return the canonical nonzero wire value in milliseconds.
    pub const fn milliseconds(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_epoch_rejects_zero() {
        assert_eq!(ServerEpoch::new(0), None);
        assert_eq!(ServerEpoch::new(7).map(ServerEpoch::get), Some(7));
    }

    #[test]
    fn relative_deadline_rejects_zero() {
        assert_eq!(RelativeDeadline::new(0), None);
        assert_eq!(
            RelativeDeadline::new(9).map(RelativeDeadline::milliseconds),
            Some(9)
        );
    }

    #[test]
    fn retry_wire_values_are_canonical() {
        assert_eq!(RetryClass::from_wire(1), Some(RetryClass::Idempotent));
        assert_eq!(RetryClass::from_wire(2), Some(RetryClass::Conditional));
        assert_eq!(RetryClass::from_wire(3), Some(RetryClass::Never));
        assert_eq!(RetryClass::from_wire(0), None);
        assert_eq!(RetryClass::from_wire(4), None);
    }
}
