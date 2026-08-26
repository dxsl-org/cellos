use crate::{Bounded, DIGEST_LEN, HOSTNAME_MAX};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrollmentIntent {
    pub generation: u64,
    pub hostname: Bounded<HOSTNAME_MAX>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsrChunkIntent {
    pub generation: u64,
    pub csr_handle: u64,
    pub chunk_index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TlsSignatureIntent {
    pub relay_generation: u64,
    pub transcript_hash: [u8; DIGEST_LEN],
    pub active_profile_digest: [u8; DIGEST_LEN],
    pub public_request_id: u64,
}
