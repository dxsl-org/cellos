// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Error definitions for the Cellos Terminal SDK.

use core::fmt;

/// Top-level error type for terminal operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalError {
    /// Invalid or malformed byte stream / argument.
    InvalidInput,
    /// Unrecognized or unsupported input sequence (e.g. unknown escape sequence).
    UnsupportedInput,
    /// Unsupported screen control command.
    UnsupportedControl,
    /// Bounded buffer or queue capacity exceeded.
    BufferOverflow,
    /// Requested session is closed.
    SessionClosed,
    /// Requested session ID does not exist.
    SessionNotFound,
    /// Caller is not the active foreground input reader.
    NotForegroundReader,
    /// Session foreground epoch has advanced (stale reader lease).
    EpochMismatch,
    /// Underlying stream or endpoint error.
    Stream(StreamError),
}

impl fmt::Display for TerminalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput => write!(f, "invalid input"),
            Self::UnsupportedInput => write!(f, "unsupported input sequence"),
            Self::UnsupportedControl => write!(f, "unsupported control"),
            Self::BufferOverflow => write!(f, "buffer capacity exceeded"),
            Self::SessionClosed => write!(f, "session closed"),
            Self::SessionNotFound => write!(f, "session not found"),
            Self::NotForegroundReader => write!(f, "caller is not foreground reader"),
            Self::EpochMismatch => write!(f, "foreground epoch mismatch"),
            Self::Stream(err) => write!(f, "stream error: {}", err),
        }
    }
}

/// Errors occurring on streams and generational endpoint grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamError {
    /// Write failed because all reader endpoints have closed.
    BrokenPipe,
    /// Read returned zero bytes and no authorized writers remain.
    Eof,
    /// Nonblocking read/write cannot proceed immediately.
    WouldBlock,
    /// Deadline elapsed before the requested I/O could complete.
    TimedOut,
    /// Operation was explicitly cancelled; descendant subtree revoked.
    Cancelled,
    /// Endpoint grant was revoked by owner or ancestor.
    Revoked,
    /// Principal or caller lacks permission for the requested operation.
    Denied,
    /// Endpoint generation does not match the active descriptor (stale handle).
    StaleGeneration,
    /// Creating cell's allocation or endpoint quota exceeded.
    QuotaExceeded,
    /// Capacity outside admitted range [64, 65536].
    CapacityOutOfRange,
    /// Invalid endpoint handle or descriptor ID.
    InvalidHandle,
    /// Invalid operation reference or parentage.
    InvalidOperation,
    /// Rights attenuation violation (granting rights not held by parent).
    InvalidRights,
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BrokenPipe => write!(f, "broken pipe"),
            Self::Eof => write!(f, "end of file"),
            Self::WouldBlock => write!(f, "operation would block"),
            Self::TimedOut => write!(f, "operation timed out"),
            Self::Cancelled => write!(f, "operation cancelled"),
            Self::Revoked => write!(f, "endpoint grant revoked"),
            Self::Denied => write!(f, "permission denied"),
            Self::StaleGeneration => write!(f, "stale endpoint generation"),
            Self::QuotaExceeded => write!(f, "quota exceeded"),
            Self::CapacityOutOfRange => write!(f, "capacity out of range [64, 65536]"),
            Self::InvalidHandle => write!(f, "invalid handle"),
            Self::InvalidOperation => write!(f, "invalid operation"),
            Self::InvalidRights => write!(f, "invalid rights attenuation"),
        }
    }
}

impl From<StreamError> for TerminalError {
    fn from(err: StreamError) -> Self {
        Self::Stream(err)
    }
}
