pub(crate) struct SuccessPayload {
    bytes: [u8; 64],
    len: usize,
}

impl SuccessPayload {
    pub(crate) fn new(bytes: &[u8]) -> Self {
        let mut payload = [0; 64];
        payload[..bytes.len()].copy_from_slice(bytes);
        Self {
            bytes: payload,
            len: bytes.len(),
        }
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
