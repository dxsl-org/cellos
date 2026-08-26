use types::kms::{
    KmsOpcode, Tls13ClientCertificateVerifyRequestPayload,
    Tls13ClientCertificateVerifyResponsePayload,
};

use super::{KmsClient, KmsClientError};

impl KmsClient {
    /// Sign one TLS 1.3 client CertificateVerify transcript.
    ///
    /// There is deliberately no algorithm, key ID, raw-message, or prehash
    /// parameter.
    pub fn sign_tls13_client_certificate_verify(
        &self,
        transcript_hash: &[u8; 32],
        relay_generation: u64,
        active_profile_digest: &[u8; 32],
        request_id: u64,
    ) -> Result<[u8; 64], KmsClientError> {
        if relay_generation == 0
            || request_id == 0
            || active_profile_digest.iter().all(|byte| *byte == 0)
        {
            return Err(KmsClientError::InvalidPayload);
        }
        let payload = Tls13ClientCertificateVerifyRequestPayload {
            transcript_hash: *transcript_hash,
            relay_generation,
            active_profile_digest: *active_profile_digest,
            request_id,
        };
        let response = self.call_opcode(
            KmsOpcode::SignTls13ClientCertificateVerify,
            &payload.encode(),
        )?;
        let payload = self.decode_payload(
            &response,
            Tls13ClientCertificateVerifyResponsePayload::decode,
        )?;
        canonical_p256_signature(&payload.signature)
            .then_some(payload.signature)
            .ok_or(KmsClientError::InvalidPayload)
    }
}

fn canonical_p256_signature(signature: &[u8; 64]) -> bool {
    const ORDER: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63,
        0x25, 0x51,
    ];
    const HALF_ORDER: [u8; 32] = [
        0x7f, 0xff, 0xff, 0xff, 0x80, 0x00, 0x00, 0x00, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xde, 0x73, 0x7d, 0x56, 0xd3, 0x8b, 0xcf, 0x42, 0x79, 0xdc, 0xe5, 0x61, 0x7e, 0x31,
        0x92, 0xa8,
    ];
    let r = &signature[..32];
    let s = &signature[32..];
    r.iter().any(|byte| *byte != 0)
        && s.iter().any(|byte| *byte != 0)
        && r < ORDER.as_slice()
        && s < ORDER.as_slice()
        && s <= HALF_ORDER.as_slice()
}
