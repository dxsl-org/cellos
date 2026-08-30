use sha2::{Digest, Sha256};

use crate::{tpm, Error};
use stm32_authority_journal::PendingEnrollmentSnapshot;

/// Maximum accepted encoded `TPM2B_PUBLIC` size, including its two-byte length.
pub const MAX_TPM2B_PUBLIC: usize = 1_026;

/// Purpose-typed identity for a pending TPM public-area read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingPublicRequest {
    slot: u8,
    generation: u64,
    policy_epoch: u64,
    upload_handle: u64,
    csr_handle: u64,
    device_id: [u8; 32],
    authority_id: [u8; 32],
    authority_epoch: u64,
    boot_epoch: u64,
}

impl PendingPublicRequest {
    /// Returns the physical enrollment slot whose public area must be read.
    pub const fn slot(self) -> u8 {
        self.slot
    }

    /// Returns the enrollment generation whose public area must be read.
    pub const fn generation(self) -> u64 {
        self.generation
    }
    /// Returns the authenticated policy epoch for this read.
    pub const fn policy_epoch(self) -> u64 {
        self.policy_epoch
    }

    /// Returns the admitted nonzero upload handle for this read.
    pub const fn upload_handle(self) -> u64 {
        self.upload_handle
    }

    /// Returns the state-issued CSR handle bound to the pending key.
    pub const fn csr_handle(self) -> u64 {
        self.csr_handle
    }

    /// Returns the admitted device identity.
    pub const fn device_id(&self) -> &[u8; 32] {
        &self.device_id
    }

    /// Returns the admitted authority identity.
    pub const fn authority_id(&self) -> &[u8; 32] {
        &self.authority_id
    }

    /// Returns the state-derived authority epoch.
    pub const fn authority_epoch(self) -> u64 {
        self.authority_epoch
    }

    /// Returns the admitted boot epoch.
    pub const fn boot_epoch(self) -> u64 {
        self.boot_epoch
    }
}

/// Storage-level failure returned by [`PendingPublicReader`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicReadError {
    /// The pending public area could not be read exactly.
    Unavailable,
    /// The destination was too small for the stored public area.
    BufferTooSmall,
}

/// Reads the exact canonical `TPM2B_PUBLIC` associated with a pending enrollment.
pub trait PendingPublicReader {
    /// Copies one complete `TPM2B_PUBLIC` into `destination` and returns its byte count.
    /// The validator invokes this twice with the same purpose-typed request.
    fn read_public(
        &mut self,
        request: PendingPublicRequest,
        destination: &mut [u8],
    ) -> Result<usize, PublicReadError>;
}

pub(crate) fn verify<R: PendingPublicReader>(
    snapshot: &PendingEnrollmentSnapshot,
    reader: &mut R,
) -> Result<[u8; 91], Error> {
    let request = PendingPublicRequest {
        slot: snapshot.pending_slot(),
        generation: snapshot.generation(),
        policy_epoch: snapshot.policy_epoch(),
        upload_handle: snapshot.upload_handle(),
        csr_handle: snapshot.csr_handle(),
        device_id: *snapshot.device_id(),
        authority_id: *snapshot.authority_id(),
        authority_epoch: snapshot.authority_epoch(),
        boot_epoch: snapshot.boot_epoch(),
    };
    let mut first = [0u8; MAX_TPM2B_PUBLIC];
    let mut second = [0u8; MAX_TPM2B_PUBLIC];
    let first_len = reader
        .read_public(request, &mut first)
        .map_err(|_| Error::PendingPublicRead)?;
    let second_len = reader
        .read_public(request, &mut second)
        .map_err(|_| Error::PendingPublicRead)?;
    if first_len > first.len() || second_len > second.len() {
        return Err(Error::PendingPublicRead);
    }
    if first_len != second_len || first[..first_len] != second[..second_len] {
        return Err(Error::PendingPublicRace);
    }
    let encoded = &first[..first_len];
    let digest: [u8; 32] = Sha256::digest(encoded).into();
    if digest != *snapshot.tpm_public_digest() {
        return Err(Error::TpmPublicDigestMismatch);
    }
    let spki = tpm::parse(encoded)?;
    let spki_digest: [u8; 32] = Sha256::digest(spki).into();
    if snapshot.spki() != spki.as_slice() || *snapshot.spki_digest() != spki_digest {
        return Err(Error::SpkiMismatch);
    }
    Ok(spki)
}
