use types::kms::{KmsErrorCode, KMS_PAYLOAD_LEN};

pub(crate) struct SuccessPayload {
    bytes: [u8; KMS_PAYLOAD_LEN],
    len: usize,
}

impl SuccessPayload {
    pub(crate) fn new(bytes: &[u8]) -> Result<Self, KmsErrorCode> {
        if bytes.len() > KMS_PAYLOAD_LEN {
            return Err(KmsErrorCode::ProviderFailure);
        }
        let mut payload = [0; KMS_PAYLOAD_LEN];
        payload[..bytes.len()].copy_from_slice(bytes);
        Ok(Self {
            bytes: payload,
            len: bytes.len(),
        })
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
