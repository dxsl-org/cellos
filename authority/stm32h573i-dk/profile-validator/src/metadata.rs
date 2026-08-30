/// Metadata produced only after the entire profile and pending TPM binding validate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatedProfileMetadata<'a> {
    pub(crate) slot: u8,
    pub(crate) generation: u64,
    pub(crate) profile_len: u32,
    pub(crate) profile_digest: [u8; 32],
    pub(crate) spki_digest: [u8; 32],
    pub(crate) node_id: [u8; 32],
    pub(crate) serial: &'a [u8],
    pub(crate) tpm_public_digest: [u8; 32],
}

impl<'a> ValidatedProfileMetadata<'a> {
    /// Returns the authenticated physical profile slot.
    pub const fn slot(&self) -> u8 {
        self.slot
    }

    /// Returns the authenticated monotonic profile generation.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the exact validated raw profile length.
    pub const fn profile_len(&self) -> u32 {
        self.profile_len
    }

    /// Returns SHA-256 of the complete validated raw profile.
    pub const fn profile_digest(&self) -> &[u8; 32] {
        &self.profile_digest
    }

    /// Returns SHA-256 of the leaf's complete DER `SubjectPublicKeyInfo`.
    pub const fn spki_digest(&self) -> &[u8; 32] {
        &self.spki_digest
    }

    /// Returns the validated raw NodeId extension.
    pub const fn node_id(&self) -> &[u8; 32] {
        &self.node_id
    }

    /// Returns the leaf's canonical unsigned positive serial bytes.
    pub const fn serial(&self) -> &'a [u8] {
        self.serial
    }

    /// Returns SHA-256 of the stable canonical `TPM2B_PUBLIC` read twice.
    pub const fn tpm_public_digest(&self) -> &[u8; 32] {
        &self.tpm_public_digest
    }
}
