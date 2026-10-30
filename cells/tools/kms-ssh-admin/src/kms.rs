//! The key-service calls the admin cell makes: exactly opcodes 15-20. The trait is the seam
//! between the commands and the K6 [`KmsClient`]; host tests implement it with a scripted
//! fake and with a bridge to the real service state machine.
//!
//! There is deliberately no method for opcodes 21-24 (sign and unlock): the admin right is
//! never held together with the sign right, and nothing in this crate can ask for either.

use ostd::clients::kms::{
    KmsClient, KmsClientError, SshChunkAckPayload, SshKeyStatusPayload,
    SshTransferBeginResponsePayload, KMS_SSH_PUBLIC_KEY_LEN,
};

pub trait Kms {
    /// Op 19: leaf status for an empty `name`, else one key's facts.
    fn status(&self, name: &[u8]) -> Result<SshKeyStatusPayload, KmsClientError>;
    /// Op 15.
    fn import_begin(
        &self,
        name: &[u8],
        total_len: u32,
        blob_sha256: &[u8; 32],
    ) -> Result<SshTransferBeginResponsePayload, KmsClientError>;
    /// Op 16.
    fn import_chunk(&self, handle: u64, index: u32, data: &[u8]) -> Result<SshChunkAckPayload, KmsClientError>;
    /// Op 17.
    fn import_commit(
        &self,
        handle: u64,
        chunk_count: u32,
        blob_sha256: &[u8; 32],
    ) -> Result<SshKeyStatusPayload, KmsClientError>;
    /// Op 18.
    fn import_abort(&self, handle: u64) -> Result<(), KmsClientError>;
    /// Op 20.
    fn delete(&self, name: &[u8], public_key: &[u8; KMS_SSH_PUBLIC_KEY_LEN]) -> Result<(), KmsClientError>;
}

impl Kms for KmsClient {
    fn status(&self, name: &[u8]) -> Result<SshKeyStatusPayload, KmsClientError> {
        self.ssh_key_status(name)
    }

    fn import_begin(
        &self,
        name: &[u8],
        total_len: u32,
        blob_sha256: &[u8; 32],
    ) -> Result<SshTransferBeginResponsePayload, KmsClientError> {
        self.ssh_import_begin(name, total_len, blob_sha256)
    }

    fn import_chunk(&self, handle: u64, index: u32, data: &[u8]) -> Result<SshChunkAckPayload, KmsClientError> {
        self.ssh_import_chunk(handle, index, data)
    }

    fn import_commit(
        &self,
        handle: u64,
        chunk_count: u32,
        blob_sha256: &[u8; 32],
    ) -> Result<SshKeyStatusPayload, KmsClientError> {
        self.ssh_import_commit(handle, chunk_count, blob_sha256)
    }

    fn import_abort(&self, handle: u64) -> Result<(), KmsClientError> {
        self.ssh_import_abort(handle)
    }

    fn delete(&self, name: &[u8], public_key: &[u8; KMS_SSH_PUBLIC_KEY_LEN]) -> Result<(), KmsClientError> {
        self.ssh_delete(name, public_key)
    }
}
