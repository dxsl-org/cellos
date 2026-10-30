//! The one stored identity (LR-1 = Y) and its public status facts.

use core::fmt;

use sha2::{Digest, Sha256};
use types::kms::{
    KmsCapabilityReadiness, KmsCustodyPosture, KmsKeyAlgorithm, KmsProviderKind, KmsSshKeyCustody,
    SshKeyStatusPayload, KMS_SSH_KEY_NAME_MAX, KMS_SSH_PUBLIC_KEY_LEN,
};
use zeroize::Zeroizing;

use super::parser::{KDF_SALT_MAX, STORE_WRAPPED_CT_MAX};

/// What is kept for the key. A plain key stores only the 32-byte seed (the
/// comment and the redundant public half are dropped); a wrapped key stores the
/// KDF parameters and ciphertext verbatim, which are passphrase-protected.
#[allow(clippy::large_enum_variant)] // no heap in this cell: the wrapped record is inline
pub(crate) enum Secret {
    Plain(Zeroizing<[u8; 32]>),
    Wrapped {
        salt: [u8; KDF_SALT_MAX],
        salt_len: u8,
        rounds: u32,
        ciphertext: [u8; STORE_WRAPPED_CT_MAX],
        ciphertext_len: u16,
    },
}

pub(crate) struct StoredIdentity {
    name: [u8; KMS_SSH_KEY_NAME_MAX],
    name_len: u8,
    public_key: [u8; KMS_SSH_PUBLIC_KEY_LEN],
    secret: Secret,
}

/// Borrowed view of one stored record, handed to the persistence seam. The
/// fields are read by a durable persister (K4b) and by the test fakes.
#[allow(dead_code)]
pub(crate) struct RecordView<'a> {
    pub revision: u64,
    pub name: &'a [u8],
    pub public_key: &'a [u8; KMS_SSH_PUBLIC_KEY_LEN],
    pub secret: SecretView<'a>,
}

#[allow(dead_code)]
pub(crate) enum SecretView<'a> {
    Plain(&'a [u8; 32]),
    Wrapped {
        salt: &'a [u8],
        rounds: u32,
        ciphertext: &'a [u8],
    },
}

impl fmt::Debug for RecordView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecordView(<redacted>)")
    }
}

impl StoredIdentity {
    pub(crate) fn plain(name: &[u8], public_key: [u8; 32], seed: &[u8; 32]) -> Option<Self> {
        Self::empty(name, public_key, Secret::Plain(Zeroizing::new(*seed)))
    }

    pub(crate) fn wrapped(
        name: &[u8],
        public_key: [u8; 32],
        salt: &[u8],
        rounds: u32,
        ciphertext: &[u8],
    ) -> Option<Self> {
        if salt.len() > KDF_SALT_MAX || ciphertext.len() > STORE_WRAPPED_CT_MAX {
            return None;
        }
        let mut salt_buf = [0u8; KDF_SALT_MAX];
        salt_buf.get_mut(..salt.len())?.copy_from_slice(salt);
        let mut ct_buf = [0u8; STORE_WRAPPED_CT_MAX];
        ct_buf
            .get_mut(..ciphertext.len())?
            .copy_from_slice(ciphertext);
        Self::empty(
            name,
            public_key,
            Secret::Wrapped {
                salt: salt_buf,
                salt_len: salt.len() as u8,
                rounds,
                ciphertext: ct_buf,
                ciphertext_len: ciphertext.len() as u16,
            },
        )
    }

    fn empty(name: &[u8], public_key: [u8; 32], secret: Secret) -> Option<Self> {
        let mut buf = [0u8; KMS_SSH_KEY_NAME_MAX];
        buf.get_mut(..name.len())?.copy_from_slice(name);
        Some(Self {
            name: buf,
            name_len: name.len() as u8,
            public_key,
            secret,
        })
    }

    pub(crate) fn name(&self) -> &[u8] {
        self.name.get(..usize::from(self.name_len)).unwrap_or(&[])
    }

    pub(crate) fn public_key(&self) -> &[u8; KMS_SSH_PUBLIC_KEY_LEN] {
        &self.public_key
    }

    pub(crate) fn secret(&self) -> &Secret {
        &self.secret
    }

    pub(crate) fn custody(&self) -> KmsSshKeyCustody {
        match self.secret {
            Secret::Plain(_) => KmsSshKeyCustody::Plain,
            Secret::Wrapped { .. } => KmsSshKeyCustody::PassphraseWrapped,
        }
    }

    pub(crate) fn record_view(&self, revision: u64) -> RecordView<'_> {
        let secret = match &self.secret {
            Secret::Plain(seed) => SecretView::Plain(seed),
            Secret::Wrapped {
                salt,
                salt_len,
                rounds,
                ciphertext,
                ciphertext_len,
            } => SecretView::Wrapped {
                salt: salt.get(..usize::from(*salt_len)).unwrap_or(&[]),
                rounds: *rounds,
                ciphertext: ciphertext
                    .get(..usize::from(*ciphertext_len))
                    .unwrap_or(&[]),
            },
        };
        RecordView {
            revision,
            name: self.name(),
            public_key: &self.public_key,
            secret,
        }
    }
}

impl fmt::Debug for StoredIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredIdentity")
            .field("custody", &self.custody())
            .field("secret", &"<redacted>")
            .finish_non_exhaustive()
    }
}

/// SHA-256 of the 51-byte SSH wire public key blob: the value `ssh-keygen -lf`
/// prints (base64) for an ed25519 key.
pub(crate) fn fingerprint(public_key: &[u8; KMS_SSH_PUBLIC_KEY_LEN]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(11u32.to_be_bytes());
    hash.update(b"ssh-ed25519");
    hash.update(32u32.to_be_bytes());
    hash.update(public_key);
    hash.finalize().into()
}

/// Leaf status payload (`name_len == 0` query): no key facts.
pub(crate) fn leaf_status(
    readiness: KmsCapabilityReadiness,
    posture: KmsCustodyPosture,
    key_count: u16,
    revision: u64,
) -> SshKeyStatusPayload {
    SshKeyStatusPayload {
        algorithm: KmsKeyAlgorithm::SshEd25519,
        readiness,
        provider: KmsProviderKind::None,
        posture,
        key_present: 0,
        custody: KmsSshKeyCustody::None,
        key_count,
        store_revision: revision,
        public_key: [0; KMS_SSH_PUBLIC_KEY_LEN],
        fingerprint_sha256: [0; 32],
    }
}

/// Named-key status payload for the stored identity.
pub(crate) fn key_status(
    identity: &StoredIdentity,
    readiness: KmsCapabilityReadiness,
    posture: KmsCustodyPosture,
    revision: u64,
) -> SshKeyStatusPayload {
    SshKeyStatusPayload {
        algorithm: KmsKeyAlgorithm::SshEd25519,
        readiness,
        provider: KmsProviderKind::None,
        posture,
        key_present: 1,
        custody: identity.custody(),
        key_count: 1,
        store_revision: revision,
        public_key: identity.public_key,
        fingerprint_sha256: fingerprint(&identity.public_key),
    }
}
