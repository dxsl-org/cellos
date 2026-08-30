#![no_std]
#![forbid(unsafe_code)]
//! Closed private AP↔STM32 protocol for the terminal `DEV_REFERENCE` lane.
//!
//! The carrier is untrusted. Exactly twelve typed operations exist; runtime
//! provisioning, unseal, reset, arbitrary execution, and caller-selected keys
//! are intentionally unrepresentable.

mod fault;
mod message;
mod state;
mod validation;
mod wire;
use sha2::{Digest, Sha256};

pub use fault::AuthorityFault;
pub use message::*;
pub use state::{
    verify_protected_record, verify_protected_successor, AuthorityMode, AuthorityState,
    AuthorityStateConfig, BootState, CsrChunkIntent, EnrollmentIntent, OpenedBootFact,
    PendingTimeChallenge, PreparedCommitIntent, ProtectedAuthorityRecord, ProtectedRecordBindings,
    ProtectedRecordVerifier, ProtectedStore, ProtectedTimeFloors, RelayIntent, RelayProfileState,
    TimeChallengeSource, TimePurpose, TimeState, TlsSignatureIntent, TrustedClock,
    VerifiedProtectedRecord, PROTECTED_RECORD_MAX,
};
pub use validation::{
    is_strict_p256_der_signature, verify_boot_measurement, verify_provider_cas_receipt,
    verify_root_profile, verify_signed_time, BootMeasurementVerifier, ProviderCasReceipt,
    ProviderCasVerifier, RootProfileVerifier, RootValidatedProfile, SignedTimeVerifier,
    VerifiedBootMeasurement, VerifiedProviderCasReceipt, VerifiedSignedTime,
};
pub use wire::{
    decode_fault, decode_fault_frame, decode_typed_request, decode_typed_response,
    encode_fault_frame, encode_typed_request, encode_typed_response, verify_typed_response,
    FrameClass, FrameHeader, FrameHeaderLayout, LaneTag, MessageKind, Operation, TypedRequest,
    TypedResponse, ValidatedResponse,
};

pub const FRAME_MAGIC: [u8; 4] = *b"AUTH";
pub const PROTOCOL_VERSION: u8 = 1;
pub const LANE_DEV_REFERENCE: u8 = 1;
pub const FRAME_HEADER_LEN: usize = 36;
pub const FRAME_MAX_PAYLOAD: usize = 1200;

/// Structural wire failures in deterministic precedence order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    Truncated,
    BadMagic,
    UnsupportedVersion,
    UnknownLaneTag,
    UnknownMessageKind,
    UnknownOperation,
    UnknownFault,
    NonZeroReserved,
    InvalidLength,
    OversizePayload,
    TrailingBytes,
    BufferTooSmall,
}

/// Fixed-capacity, non-allocating byte sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounded<const N: usize> {
    len: u16,
    buf: [u8; N],
}

impl<const N: usize> Bounded<N> {
    /// Copy `src` in full; return `None` instead of truncating when oversized.
    pub fn from_slice(src: &[u8]) -> Option<Self> {
        if src.len() > N || src.len() > u16::MAX as usize {
            return None;
        }
        let mut buf = [0u8; N];
        buf[..src.len()].copy_from_slice(src);
        Some(Self {
            len: src.len() as u16,
            buf,
        })
    }

    /// Return the occupied prefix only.
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len as usize]
    }

    /// Return the occupied byte count.
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Report whether no bytes are occupied.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Constant-time equality for fixed authentication values.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for index in 0..a.len() {
        diff |= a[index] ^ b[index];
    }
    diff == 0
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

const _: () = assert!(FRAME_HEADER_LEN == 36);
const _: () = assert!(FRAME_MAX_PAYLOAD <= u16::MAX as usize);
