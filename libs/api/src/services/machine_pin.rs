// SPDX-License-Identifier: Apache-2.0
//! Machine pin frame: cellbox ↔ init over `service::MACHINE_PIN` (ADR-0029 §3).
//!
//! Class C record (`docs/abi-boundary-records.md`). Fixed little-endian
//! frames, identified by magic, version exactly [`FRAME_VERSION`]; unknown
//! operations, kinds, states and codes, non-zero reserved bytes and non-zero
//! bytes past the frame are refused before any effect. A request fits init's
//! 64-byte receive payload; caller identity never comes from the frame but
//! from the kernel's attested receive trailer.
//!
//! Request ([`REQUEST_BYTES`]): 0 magic `MACHPINQ` · 8 version · 9 op (1
//! Status, 2 Confirm) · 10 reserved [6] · 16 digest [32] (zero for Status).
//! Response ([`RESPONSE_BYTES`]): 0 magic `MACHPINR` · 8 version · 9 kind (1
//! Status, 2 Recorded, 3 Refused) · 10 code ([`PinState`] for Status,
//! [`Refusal`] for Refused, zero for Recorded) · 11 reserved [5] · 16 digest
//! [32] (Status: current; Recorded: the new pin; Refused: zero) · 48 digest
//! [32] (Status: pinned, zero when none; otherwise zero).

/// Version byte of both frames.
pub const FRAME_VERSION: u8 = 1;
/// Exact request length.
pub const REQUEST_BYTES: usize = 48;
/// Exact response length.
pub const RESPONSE_BYTES: usize = 80;
const REQUEST_MAGIC: &[u8; 8] = b"MACHPINQ";
const RESPONSE_MAGIC: &[u8; 8] = b"MACHPINR";

/// What init asks or is asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachinePinRequest {
    /// Report this boot's gate outcome and both digests. Answered in every mode.
    Status,
    /// Re-pin the current facts. Honoured only in safe mode, only from the
    /// shell init spawned, and only when `digest` is the current one.
    Confirm { digest: [u8; 32] },
}

/// This boot's gate outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PinState {
    /// The stored record equals the current facts.
    Match = 1,
    /// No record existed; the current facts were recorded.
    FirstBootRecorded = 2,
    /// The stored record decodes but differs: safe mode.
    Mismatch = 3,
    /// The stored record does not decode or has an unknown version: safe mode.
    RecordUnreadable = 4,
    /// The pin store could not be read or written.
    StoreUnavailable = 5,
    /// The kernel gave no facts, or init could not register the endpoint.
    FactsUnavailable = 6,
}

/// Why a [`MachinePinRequest::Confirm`] was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Refusal {
    /// Init is not in safe mode.
    NotSafeMode = 1,
    /// The sender is not the live console shell init spawned.
    NotConsoleShell = 2,
    /// The digest is not the current facts' digest.
    DigestStale = 3,
    /// Writing or reading back the record failed.
    StoreWriteFailed = 4,
}

/// Init's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachinePinResponse {
    /// `pinned` is zero when no readable record exists.
    Status { state: PinState, current: [u8; 32], pinned: [u8; 32] },
    /// The current facts are now the pin; reboot to start services.
    Recorded { digest: [u8; 32] },
    /// Nothing was written.
    Refused(Refusal),
}

impl PinState {
    fn decode(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Match,
            2 => Self::FirstBootRecorded,
            3 => Self::Mismatch,
            4 => Self::RecordUnreadable,
            5 => Self::StoreUnavailable,
            6 => Self::FactsUnavailable,
            _ => return None,
        })
    }

    /// Lower-case name for console output.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Match => "match",
            Self::FirstBootRecorded => "first-boot-recorded",
            Self::Mismatch => "mismatch",
            Self::RecordUnreadable => "record-unreadable",
            Self::StoreUnavailable => "store-unavailable",
            Self::FactsUnavailable => "facts-unavailable",
        }
    }
}

impl Refusal {
    fn decode(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::NotSafeMode,
            2 => Self::NotConsoleShell,
            3 => Self::DigestStale,
            4 => Self::StoreWriteFailed,
            _ => return None,
        })
    }

    /// The variant name, for console output.
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotSafeMode => "NotSafeMode",
            Self::NotConsoleShell => "NotConsoleShell",
            Self::DigestStale => "DigestStale",
            Self::StoreWriteFailed => "StoreWriteFailed",
        }
    }
}

fn header_valid(frame: &[u8], magic: &[u8; 8], len: usize) -> bool {
    frame.len() >= len
        && &frame[..8] == magic
        && frame[8] == FRAME_VERSION
        && frame[len..].iter().all(|byte| *byte == 0)
}

fn digest_at(frame: &[u8], at: usize) -> [u8; 32] {
    let mut digest = [0; 32];
    digest.copy_from_slice(&frame[at..at + 32]);
    digest
}

fn zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

impl MachinePinRequest {
    /// Wire bytes.
    pub fn encode(self) -> [u8; REQUEST_BYTES] {
        let mut frame = [0; REQUEST_BYTES];
        frame[..8].copy_from_slice(REQUEST_MAGIC);
        frame[8] = FRAME_VERSION;
        match self {
            Self::Status => frame[9] = 1,
            Self::Confirm { digest } => {
                frame[9] = 2;
                frame[16..48].copy_from_slice(&digest);
            }
        }
        frame
    }

    /// The request in `frame`, which may be a longer zero-padded payload;
    /// `None` for anything else.
    pub fn decode(frame: &[u8]) -> Option<Self> {
        if !header_valid(frame, REQUEST_MAGIC, REQUEST_BYTES) || !zero(&frame[10..16]) {
            return None;
        }
        match frame[9] {
            1 if zero(&frame[16..48]) => Some(Self::Status),
            2 => Some(Self::Confirm { digest: digest_at(frame, 16) }),
            _ => None,
        }
    }

    /// Whether `frame` claims to be a request of this protocol (magic only).
    /// Init uses it to answer a malformed claim instead of treating it as an
    /// exit record.
    pub fn is_claimed(frame: &[u8]) -> bool {
        frame.len() >= 8 && &frame[..8] == REQUEST_MAGIC
    }
}

impl MachinePinResponse {
    /// Wire bytes.
    pub fn encode(self) -> [u8; RESPONSE_BYTES] {
        let mut frame = [0; RESPONSE_BYTES];
        frame[..8].copy_from_slice(RESPONSE_MAGIC);
        frame[8] = FRAME_VERSION;
        match self {
            Self::Status { state, current, pinned } => {
                frame[9] = 1;
                frame[10] = state as u8;
                frame[16..48].copy_from_slice(&current);
                frame[48..80].copy_from_slice(&pinned);
            }
            Self::Recorded { digest } => {
                frame[9] = 2;
                frame[16..48].copy_from_slice(&digest);
            }
            Self::Refused(refusal) => {
                frame[9] = 3;
                frame[10] = refusal as u8;
            }
        }
        frame
    }

    /// The response in `frame`; `None` for anything else.
    pub fn decode(frame: &[u8]) -> Option<Self> {
        if !header_valid(frame, RESPONSE_MAGIC, RESPONSE_BYTES) || !zero(&frame[11..16]) {
            return None;
        }
        match frame[9] {
            1 => Some(Self::Status {
                state: PinState::decode(frame[10])?,
                current: digest_at(frame, 16),
                pinned: digest_at(frame, 48),
            }),
            2 if frame[10] == 0 && zero(&frame[48..80]) => {
                Some(Self::Recorded { digest: digest_at(frame, 16) })
            }
            3 if zero(&frame[16..80]) => Some(Self::Refused(Refusal::decode(frame[10])?)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip_and_fit_inits_payload() {
        const { assert!(REQUEST_BYTES <= 64) };
        for request in [MachinePinRequest::Status, MachinePinRequest::Confirm { digest: [7; 32] }] {
            let frame = request.encode();
            assert_eq!(MachinePinRequest::decode(&frame), Some(request));
            let mut padded = [0u8; 64];
            padded[..REQUEST_BYTES].copy_from_slice(&frame);
            assert_eq!(MachinePinRequest::decode(&padded), Some(request));
            assert!(MachinePinRequest::is_claimed(&padded));
        }
    }

    #[test]
    fn malformed_requests_are_refused() {
        let base = MachinePinRequest::Confirm { digest: [7; 32] }.encode();
        let refuse = |mutate: &dyn Fn(&mut [u8; 64])| {
            let mut frame = [0u8; 64];
            frame[..REQUEST_BYTES].copy_from_slice(&base);
            mutate(&mut frame);
            assert_eq!(MachinePinRequest::decode(&frame), None);
        };
        refuse(&|f| f[0] = b'X');
        refuse(&|f| f[8] = 2);
        refuse(&|f| f[9] = 0);
        refuse(&|f| f[9] = 3);
        refuse(&|f| f[10] = 1);
        refuse(&|f| f[15] = 1);
        refuse(&|f| f[48] = 1);
        refuse(&|f| f[63] = 1);
        refuse(&|f| { f[9] = 1; });
        assert_eq!(MachinePinRequest::decode(&base[..47]), None);
        assert!(!MachinePinRequest::is_claimed(b"OCLSREQ1"));
    }

    #[test]
    fn responses_round_trip() {
        for response in [
            MachinePinResponse::Status { state: PinState::Mismatch, current: [1; 32], pinned: [2; 32] },
            MachinePinResponse::Status { state: PinState::StoreUnavailable, current: [1; 32], pinned: [0; 32] },
            MachinePinResponse::Recorded { digest: [3; 32] },
            MachinePinResponse::Refused(Refusal::DigestStale),
        ] {
            assert_eq!(MachinePinResponse::decode(&response.encode()), Some(response));
        }
    }

    #[test]
    fn malformed_responses_are_refused() {
        let status = MachinePinResponse::Status { state: PinState::Match, current: [1; 32], pinned: [1; 32] }.encode();
        let mut bad = status; bad[10] = 0; assert_eq!(MachinePinResponse::decode(&bad), None);
        let mut bad = status; bad[10] = 7; assert_eq!(MachinePinResponse::decode(&bad), None);
        let mut bad = status; bad[11] = 1; assert_eq!(MachinePinResponse::decode(&bad), None);
        let mut bad = status; bad[9] = 4; assert_eq!(MachinePinResponse::decode(&bad), None);
        let mut bad = status; bad[8] = 0; assert_eq!(MachinePinResponse::decode(&bad), None);
        let refused = MachinePinResponse::Refused(Refusal::NotSafeMode).encode();
        let mut bad = refused; bad[10] = 5; assert_eq!(MachinePinResponse::decode(&bad), None);
        let mut bad = refused; bad[20] = 1; assert_eq!(MachinePinResponse::decode(&bad), None);
        let recorded = MachinePinResponse::Recorded { digest: [3; 32] }.encode();
        let mut bad = recorded; bad[10] = 1; assert_eq!(MachinePinResponse::decode(&bad), None);
        let mut bad = recorded; bad[50] = 1; assert_eq!(MachinePinResponse::decode(&bad), None);
        assert_eq!(MachinePinResponse::decode(&status[..79]), None);
    }
}
