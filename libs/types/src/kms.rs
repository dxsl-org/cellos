// SPDX-License-Identifier: MPL-2.0
//! Fixed-layout IPC contract for the key-management service.
//!
//! KMS owns the C2C X25519 private key. The contract exposes only public
//! identity metadata, opaque handles, and policy-gated static-DH output.

mod frame;
mod model;
mod payload;
#[cfg(test)]
mod tests;

pub use frame::{KmsRequestV1, KmsResponseV1};
pub use model::*;
pub use payload::*;

/// Current KMS ABI version. Versions and opcodes are append-only.
pub const KMS_ABI_VERSION: u8 = 1;
/// Exact request and response frame size.
pub const KMS_MESSAGE_LEN: usize = 128;
/// Payload remaining after the 16-byte frame header.
pub const KMS_PAYLOAD_LEN: usize = 112;
/// Singleton key slot reserved for Cell-to-Cell Anywhere.
pub const KMS_NODE_KEY_ID_C2C: u16 = 1;
