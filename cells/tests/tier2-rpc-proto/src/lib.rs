//! Wire contract for the Phase-02 cross-tier fixture (`/bin/tier2-rpc-provider`
//! ↔ `/bin/tier2-rpc-driver`).
//!
//! One postcard record each way, so both cells cannot drift from one shape. The
//! fixture is deliberately **tid-addressed**: a private-root Cell cannot register a
//! service today (`RegisterService` stays `SpawnCap`-gated), so `init` hands the
//! driver the provider's tid through the reviewed argv stash and no registry entry
//! exists for [`SERVICE_ID`]. That id is carried only so the two cells name the
//! same number, and the driver witnesses that it resolves to nothing.
//!
//! Not a public ABI: nothing outside the fixture's two cells links this crate.

#![no_std]

use serde::{Deserialize, Serialize};

/// The fixture's service id. **Not registered** — see the module docs.
pub const SERVICE_ID: u16 = 0x7A01;

/// Served method: echo the request body and report what the Tier-2 cell saw when it
/// called the named Tier-1 VFS service.
pub const METHOD_ECHO: u8 = 1;

/// A method the provider deliberately does not authorize. It must answer with a
/// refusal and leave no side effect.
pub const METHOD_UNAUTHORIZED: u8 = 9;

/// Request the driver sends and the provider serves.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct EchoRequest<'a> {
    /// One of [`METHOD_ECHO`] / [`METHOD_UNAUTHORIZED`].
    pub method: u8,
    /// Caller-chosen nonce, echoed so a stale or reordered reply cannot pass.
    pub nonce: u64,
    /// Nontrivial payload: the provider echoes its length and checksum.
    pub payload: &'a [u8],
}

/// What the provider answers.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct EchoResponse<'a> {
    /// The method this answer belongs to.
    pub method: u8,
    /// Echo of the request nonce.
    pub nonce: u64,
    /// Length and checksum of the payload the provider actually received.
    pub payload_len: u32,
    pub payload_checksum: u32,
    /// Whether the Tier-2 cell's own typed call to the named Tier-1 VFS service
    /// completed, and what that service said about the root directory.
    pub vfs_stat_ok: bool,
    pub vfs_root_is_dir: bool,
    /// Non-empty only when the provider refused the request.
    pub error: Option<&'a str>,
}

/// The refusal a provider returns for a method it does not authorize.
pub const UNAUTHORIZED: &str = "unauthorized-method";

/// FNV-1a over the payload: any truncation, reorder or substitution in the copied
/// body changes it, so the driver can tell an echo from a plausible-looking reply.
pub fn checksum(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in bytes {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Encode a request into `out`; `None` when it does not fit.
pub fn encode_request(request: &EchoRequest<'_>, out: &mut [u8]) -> Option<usize> {
    postcard::to_slice(request, out).ok().map(|used| used.len())
}

/// Decode a request from `bytes`; `None` when the bytes are not this record.
pub fn decode_request(bytes: &[u8]) -> Option<EchoRequest<'_>> {
    postcard::from_bytes(bytes).ok()
}

/// Encode a response into `out`; `None` when it does not fit.
pub fn encode_response(response: &EchoResponse<'_>, out: &mut [u8]) -> Option<usize> {
    postcard::to_slice(response, out)
        .ok()
        .map(|used| used.len())
}

/// Decode a response from `bytes`; `None` when the bytes are not this record.
pub fn decode_response(bytes: &[u8]) -> Option<EchoResponse<'_>> {
    postcard::from_bytes(bytes).ok()
}
