use super::{canonical_tail, get_u16, get_u32, put_u16, put_u32, validate_header};
use crate::kms::{KmsOpcode, KmsWireError, KMS_ABI_VERSION, KMS_MESSAGE_LEN, KMS_PAYLOAD_LEN};

/// Version-1 request envelope. Reserved bytes and unused payload bytes are zero.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KmsRequestV1 {
    pub abi_version: u8,
    pub opcode: u8,
    pub flags: u16,
    pub request_id: u32,
    pub payload_len: u16,
    pub reserved0: u16,
    pub reserved1: u32,
    pub payload: [u8; KMS_PAYLOAD_LEN],
}

impl KmsRequestV1 {
    /// Build a canonical request or reject a payload larger than 112 bytes.
    pub fn new(opcode: KmsOpcode, request_id: u32, payload: &[u8]) -> Result<Self, KmsWireError> {
        if payload.len() > KMS_PAYLOAD_LEN {
            return Err(KmsWireError::PayloadTooLong(payload.len() as u16));
        }
        let mut frame = Self {
            abi_version: KMS_ABI_VERSION,
            opcode: opcode as u8,
            flags: 0,
            request_id,
            payload_len: payload.len() as u16,
            reserved0: 0,
            reserved1: 0,
            payload: [0; KMS_PAYLOAD_LEN],
        };
        frame.payload[..payload.len()].copy_from_slice(payload);
        Ok(frame)
    }

    /// Decode and validate exactly one 128-byte request.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KmsWireError> {
        if bytes.len() != KMS_MESSAGE_LEN {
            return Err(KmsWireError::InvalidLength(bytes.len()));
        }
        let mut payload = [0; KMS_PAYLOAD_LEN];
        payload.copy_from_slice(&bytes[16..]);
        let frame = Self {
            abi_version: bytes[0],
            opcode: bytes[1],
            flags: get_u16(bytes, 2),
            request_id: get_u32(bytes, 4),
            payload_len: get_u16(bytes, 8),
            reserved0: get_u16(bytes, 10),
            reserved1: get_u32(bytes, 12),
            payload,
        };
        frame.validate()?;
        Ok(frame)
    }

    /// Encode the frame using the frozen little-endian wire layout.
    pub fn to_bytes(&self) -> [u8; KMS_MESSAGE_LEN] {
        let mut out = [0; KMS_MESSAGE_LEN];
        out[0] = self.abi_version;
        out[1] = self.opcode;
        put_u16(&mut out, 2, self.flags);
        put_u32(&mut out, 4, self.request_id);
        put_u16(&mut out, 8, self.payload_len);
        put_u16(&mut out, 10, self.reserved0);
        put_u32(&mut out, 12, self.reserved1);
        out[16..].copy_from_slice(&self.payload);
        out
    }

    pub fn opcode(&self) -> Result<KmsOpcode, KmsWireError> {
        KmsOpcode::try_from(self.opcode).map_err(KmsWireError::UnknownOpcode)
    }

    pub fn payload(&self) -> Result<&[u8], KmsWireError> {
        self.validate()?;
        Ok(&self.payload[..self.payload_len as usize])
    }

    fn validate(&self) -> Result<(), KmsWireError> {
        validate_header(self.abi_version, self.opcode, self.payload_len)?;
        if self.flags != 0 || self.reserved0 != 0 || self.reserved1 != 0 {
            return Err(KmsWireError::NonZeroReserved);
        }
        canonical_tail(&self.payload, self.payload_len)
    }
}

const _: () = assert!(core::mem::size_of::<KmsRequestV1>() == KMS_MESSAGE_LEN);
