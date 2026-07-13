//! Internal attestation token — a fixed-width, VPOL-shaped, verify-then-parse
//! blob (dossier-4 Decision 2). Native format for G1/G2 fleet-internal use; a
//! COSE_Sign1/CBOR interop adapter for external RATS/Veraison verifiers is a
//! deferred, separate phase (P06) layered on top, not the wire format itself.
//!
//! Layout (little-endian, all fixed-width — no variable-length fields):
//! ```text
//!   offset   0..4  : magic    [u8;4]  = b"ATT1"
//!   offset   4     : version  u8      = 1
//!   offset   5..37 : node_id               [u8;32]
//!   offset  37..69 : measurement_aggregate [u8;32]
//!   offset  69..134: alias_pubkey          [u8;65]  (SEC1 uncompressed P-256 point)
//!   offset 134..150: nonce                 [u8;16]
//!   offset 150..214: sig                   [u8;64]  (raw P-256 r‖s, NOT DER)
//! ```
//!
//! Signature convention: the signed bytes are `sha256(header || body)` (a
//! **prehash**), matching Silo's established `sign_prehash` convention
//! (`cells/guests/silo-guest/src/crypto.rs`) exactly — so `encode`'s `sign_fn`
//! seam accepts the real Silo signer unchanged when P02/P03 wire it in. This
//! module never signs on its own; P00 defines the format + a verify helper only.
//!
//! CDI values are secret and MUST NEVER appear in a token — only the *derived*
//! `alias_pubkey` does. The aggregate is a hash of public ELF bytes and the
//! pubkey is public, so a valid token is safe to log or transmit.

use p256::ecdsa::signature::hazmat::PrehashVerifier;
use p256::ecdsa::{Signature, VerifyingKey};

use super::sha256::sha256;

pub const TOKEN_MAGIC: [u8; 4] = *b"ATT1";
pub const TOKEN_VERSION: u8 = 1;

const NODE_ID_LEN: usize = 32;
const AGGREGATE_LEN: usize = 32;
const ALIAS_PUBKEY_LEN: usize = 65;
const NONCE_LEN: usize = 16;
const HEADER_LEN: usize = 5; // magic(4) + version(1)
const BODY_LEN: usize = NODE_ID_LEN + AGGREGATE_LEN + ALIAS_PUBKEY_LEN + NONCE_LEN; // 145
const SIG_LEN: usize = 64;
/// Total encoded token length: header(5) + body(145) + sig(64) = 214 bytes.
pub const TOKEN_LEN: usize = HEADER_LEN + BODY_LEN + SIG_LEN;

/// The token's structured payload (everything the signature covers except the
/// header). No secret ever lives here — see the module doc.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AttestBody {
    /// The attesting node's identity (e.g. a K2/K3 public key or node hash).
    pub node_id: [u8; NODE_ID_LEN],
    /// The DICE measurement aggregate this token attests to (public: a hash of
    /// public ELF bytes, never the CDI itself).
    pub measurement_aggregate: [u8; AGGREGATE_LEN],
    /// The alias key derived from the final CDI — SEC1 uncompressed P-256 point
    /// (`04 || X || Y`), the same 65-byte shape `SiloHandle` already produces.
    pub alias_pubkey: [u8; ALIAS_PUBKEY_LEN],
    /// Anti-replay / freshness nonce (caller-supplied; not interpreted here).
    pub nonce: [u8; NONCE_LEN],
}

impl AttestBody {
    fn write_to(&self, buf: &mut [u8]) {
        let mut off = 0;
        buf[off..off + NODE_ID_LEN].copy_from_slice(&self.node_id);
        off += NODE_ID_LEN;
        buf[off..off + AGGREGATE_LEN].copy_from_slice(&self.measurement_aggregate);
        off += AGGREGATE_LEN;
        buf[off..off + ALIAS_PUBKEY_LEN].copy_from_slice(&self.alias_pubkey);
        off += ALIAS_PUBKEY_LEN;
        buf[off..off + NONCE_LEN].copy_from_slice(&self.nonce);
    }

    /// Read a body from an already-verified, exactly-`BODY_LEN`-byte slice.
    /// Never called on unverified bytes — see `parse_and_verify`.
    fn read_from(buf: &[u8; BODY_LEN]) -> Self {
        let mut node_id = [0u8; NODE_ID_LEN];
        let mut measurement_aggregate = [0u8; AGGREGATE_LEN];
        let mut alias_pubkey = [0u8; ALIAS_PUBKEY_LEN];
        let mut nonce = [0u8; NONCE_LEN];
        let mut off = 0;
        node_id.copy_from_slice(&buf[off..off + NODE_ID_LEN]);
        off += NODE_ID_LEN;
        measurement_aggregate.copy_from_slice(&buf[off..off + AGGREGATE_LEN]);
        off += AGGREGATE_LEN;
        alias_pubkey.copy_from_slice(&buf[off..off + ALIAS_PUBKEY_LEN]);
        off += ALIAS_PUBKEY_LEN;
        nonce.copy_from_slice(&buf[off..off + NONCE_LEN]);
        Self { node_id, measurement_aggregate, alias_pubkey, nonce }
    }
}

/// Why a token blob failed to parse or verify. Never leaks which byte differed
/// (a single "malformed" outcome per stage, matching `policy.rs`'s discipline).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AttestError {
    /// Blob length is not exactly `TOKEN_LEN`.
    BadLength,
    /// `magic` does not equal `TOKEN_MAGIC`.
    BadMagic,
    /// `version` does not equal `TOKEN_VERSION`.
    BadVersion,
    /// Raw signature bytes are not a well-formed P-256 signature, or the
    /// signature does not verify against `header || body`.
    BadSignature,
}

/// Encode a token: writes `magic || version || body`, hashes it with SHA-256
/// (a prehash, matching Silo's `sign_prehash` convention), and calls `sign_fn`
/// with that digest to obtain the raw 64-byte P-256 signature. P00 does not sign
/// on its own — later phases pass `|digest| silo_handle.sign(digest)` here
/// unchanged (the seam this phase exists to define).
pub fn encode(body: &AttestBody, sign_fn: impl FnOnce(&[u8; 32]) -> [u8; SIG_LEN]) -> [u8; TOKEN_LEN] {
    let mut buf = [0u8; TOKEN_LEN];
    buf[0..4].copy_from_slice(&TOKEN_MAGIC);
    buf[4] = TOKEN_VERSION;
    body.write_to(&mut buf[HEADER_LEN..HEADER_LEN + BODY_LEN]);

    let digest = sha256(&buf[..HEADER_LEN + BODY_LEN]);
    let sig = sign_fn(&digest);
    buf[HEADER_LEN + BODY_LEN..].copy_from_slice(&sig);
    buf
}

/// Verify-then-parse a token blob (VPOL invariant, `kernel/src/policy.rs`
/// mirrored exactly): the signature is checked over `header || body` BEFORE any
/// field is interpreted as structured data. Fail-closed and panic-free — a
/// malformed or truncated blob returns `Err`, never panics.
pub fn parse_and_verify(blob: &[u8], verifying_key: &VerifyingKey) -> Result<AttestBody, AttestError> {
    if blob.len() != TOKEN_LEN {
        return Err(AttestError::BadLength);
    }
    if blob[0..4] != TOKEN_MAGIC {
        return Err(AttestError::BadMagic);
    }
    if blob[4] != TOKEN_VERSION {
        return Err(AttestError::BadVersion);
    }

    let signed = &blob[..HEADER_LEN + BODY_LEN];
    let sig_bytes = &blob[HEADER_LEN + BODY_LEN..];
    let sig = Signature::from_slice(sig_bytes).map_err(|_| AttestError::BadSignature)?;

    let digest = sha256(signed);
    verifying_key
        .verify_prehash(&digest, &sig)
        .map_err(|_| AttestError::BadSignature)?;

    // Parser runs ONLY after verification succeeds — never on unverified bytes.
    let mut body_buf = [0u8; BODY_LEN];
    body_buf.copy_from_slice(&blob[HEADER_LEN..HEADER_LEN + BODY_LEN]);
    Ok(AttestBody::read_from(&body_buf))
}

// Tests live in the sibling `token_tests.rs` (Law: files under 200 LOC).
#[cfg(test)]
#[path = "token_tests.rs"]
mod tests;
