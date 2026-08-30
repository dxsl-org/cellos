use crate::{Bounded, DIGEST_LEN, HOSTNAME_MAX, ID_LEN, PROFILE_CHUNK_MAX};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrollmentIntent {
    pub generation: u64,
    pub pending_slot: u8,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileUploadIntent {
    pub device_id: [u8; ID_LEN],
    pub authority_id: [u8; ID_LEN],
    pub authority_epoch: u64,
    pub boot_epoch: u64,
    pub generation: u64,
    pub csr_handle: u64,
    pub policy_epoch: u64,
    pub pending_slot: u8,
    pub pending_spki_digest: [u8; DIGEST_LEN],
    pub profile_digest: [u8; DIGEST_LEN],
    pub tpm_public_digest: [u8; DIGEST_LEN],
    pub upload_handle: u64,
    pub profile_len: u32,
    pub next_index: u8,
}

impl ProfileUploadIntent {
    pub const fn chunk_count(&self) -> u8 {
        let size = PROFILE_CHUNK_MAX as u32;
        self.profile_len.div_ceil(size) as u8
    }

    pub const fn complete(&self) -> bool {
        self.next_index == self.chunk_count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileChunkMode {
    Write,
    VerifyExisting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileChunkIntent {
    pub upload: ProfileUploadIntent,
    pub chunk_index: u8,
    pub chunk: Bounded<PROFILE_CHUNK_MAX>,
    pub mode: ProfileChunkMode,
}
