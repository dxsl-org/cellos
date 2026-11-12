// SPDX-License-Identifier: Apache-2.0
//! Config service errors carried in `ConfigResponse::Err(u8)`; the requests
//! are the postcard `ConfigRequest`/`ConfigResponse` in `ipc.rs`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConfigError {
    /// Unattested caller, missing `CALLER_FLAG_CONFIG_WRITE`, or a key outside
    /// the shell namespace; the store is untouched. Same byte as
    /// [`super::net::NetError::Denied`].
    Denied = 0xFC,
    /// The request did not decode.
    Malformed = 0xFF,
}
