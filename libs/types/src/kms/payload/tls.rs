use super::{put_u64, read_32, read_u64};

/// Purpose-specific TLS 1.3 client CertificateVerify request.
///
/// KMS and its provider reconstruct the protocol message from these typed
/// fields. This ABI intentionally has no algorithm, key ID, raw-message, or
/// caller-computed prehash field.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tls13ClientCertificateVerifyRequestPayload {
    pub transcript_hash: [u8; 32],
    pub relay_generation: u64,
    pub active_profile_digest: [u8; 32],
    pub request_id: u64,
}

impl Tls13ClientCertificateVerifyRequestPayload {
    pub const LEN: usize = 80;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[..32].copy_from_slice(&self.transcript_hash);
        put_u64(&mut out, 32, self.relay_generation);
        out[40..72].copy_from_slice(&self.active_profile_digest);
        put_u64(&mut out, 72, self.request_id);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        Some(Self {
            transcript_hash: read_32(bytes, 0),
            relay_generation: read_u64(bytes, 32),
            active_profile_digest: read_32(bytes, 40),
            request_id: read_u64(bytes, 72),
        })
    }
}

/// Successful TLS signing response: canonical big-endian, low-S `r || s`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tls13ClientCertificateVerifyResponsePayload {
    pub signature: [u8; 64],
}

impl Tls13ClientCertificateVerifyResponsePayload {
    pub const LEN: usize = 64;

    pub fn encode(&self) -> [u8; Self::LEN] {
        self.signature
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::LEN {
            return None;
        }
        let mut signature = [0u8; 64];
        signature.copy_from_slice(bytes);
        Some(Self { signature })
    }
}

const _: () = assert!(core::mem::size_of::<Tls13ClientCertificateVerifyRequestPayload>() == 80);
const _: () = assert!(core::mem::size_of::<Tls13ClientCertificateVerifyResponsePayload>() == 64);
