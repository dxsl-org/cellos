mod request;
mod response;

pub use request::KmsRequestV1;
pub use response::KmsResponseV1;

use super::{KmsOpcode, KmsWireError, KMS_ABI_VERSION, KMS_PAYLOAD_LEN};

fn validate_header(version: u8, opcode: u8, payload_len: u16) -> Result<(), KmsWireError> {
    if version != KMS_ABI_VERSION {
        return Err(KmsWireError::UnsupportedVersion(version));
    }
    KmsOpcode::try_from(opcode).map_err(KmsWireError::UnknownOpcode)?;
    if payload_len as usize > KMS_PAYLOAD_LEN {
        return Err(KmsWireError::PayloadTooLong(payload_len));
    }
    Ok(())
}

fn canonical_tail(payload: &[u8; KMS_PAYLOAD_LEN], len: u16) -> Result<(), KmsWireError> {
    if payload[len as usize..].iter().any(|byte| *byte != 0) {
        Err(KmsWireError::NonCanonicalPayload)
    } else {
        Ok(())
    }
}

fn get_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn get_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn put_u16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
