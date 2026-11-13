// SPDX-License-Identifier: Apache-2.0
//! VFS error bytes carried in `VfsResponse::Err(u8)`; the requests are the
//! postcard `VfsRequest`/`VfsResponse` in `ipc.rs`, which also owns the
//! `ssh_trust`, `save_refusal` and directory-page bytes.
//!
//! These codes are VFS-specific: 1/2/4 do not mean what the generic
//! `ViError` code map gives them.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VfsError {
    /// The path does not exist, the device failed, or the backend refused.
    Io = 1,
    Quota = 2,
    /// Same meaning as `ViError::PermissionDenied`.
    Denied = 3,
    /// Stale, unknown, or not-this-caller's handle.
    Handle = 4,
    /// The backend has no such operation.
    Unsupported = 6,
    /// The request did not decode.
    Malformed = 0xFF,
}

impl VfsError {
    pub const fn from_u8(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::Io,
            2 => Self::Quota,
            3 => Self::Denied,
            4 => Self::Handle,
            6 => Self::Unsupported,
            0xFF => Self::Malformed,
            _ => return None,
        })
    }
}
