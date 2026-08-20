use super::{canonical_tail, get_u16, get_u32, put_u16, put_u32, validate_header};
use crate::kms::{
    KmsErrorCode, KmsOpcode, KmsResponseStatus, KmsWireError, KMS_ABI_VERSION, KMS_MESSAGE_LEN,
    KMS_PAYLOAD_LEN,
};

/// Version-1 response envelope. Error responses never carry free-form strings.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KmsResponseV1 {
    pub abi_version: u8,
    pub opcode: u8,
    pub status: u8,
    pub reserved0: u8,
    pub request_id: u32,
    pub code: u16,
    pub payload_len: u16,
    pub reserved1: u32,
    pub payload: [u8; KMS_PAYLOAD_LEN],
}

impl KmsResponseV1 {
    /// Build a successful response with a bounded payload.
    pub fn ok(opcode: KmsOpcode, request_id: u32, payload: &[u8]) -> Result<Self, KmsWireError> {
        Self::new(opcode, KmsResponseStatus::Ok, request_id, 0, payload)
    }

    /// Build a typed error response with an empty, log-safe payload.
    pub fn error(opcode: KmsOpcode, request_id: u32, code: KmsErrorCode) -> Self {
        Self::new(
            opcode,
            KmsResponseStatus::Error,
            request_id,
            code as u16,
            &[],
        )
        .expect("empty KMS error response")
    }

    /// Decode and validate exactly one 128-byte response.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KmsWireError> {
        if bytes.len() != KMS_MESSAGE_LEN {
            return Err(KmsWireError::InvalidLength(bytes.len()));
        }
        let mut payload = [0; KMS_PAYLOAD_LEN];
        payload.copy_from_slice(&bytes[16..]);
        let frame = Self {
            abi_version: bytes[0],
            opcode: bytes[1],
            status: bytes[2],
            reserved0: bytes[3],
            request_id: get_u32(bytes, 4),
            code: get_u16(bytes, 8),
            payload_len: get_u16(bytes, 10),
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
        out[2] = self.status;
        out[3] = self.reserved0;
        put_u32(&mut out, 4, self.request_id);
        put_u16(&mut out, 8, self.code);
        put_u16(&mut out, 10, self.payload_len);
        put_u32(&mut out, 12, self.reserved1);
        out[16..].copy_from_slice(&self.payload);
        out
    }

    pub fn opcode(&self) -> Result<KmsOpcode, KmsWireError> {
        KmsOpcode::try_from(self.opcode).map_err(KmsWireError::UnknownOpcode)
    }

    pub fn error_code(&self) -> Result<Option<KmsErrorCode>, KmsWireError> {
        match KmsResponseStatus::try_from(self.status).map_err(KmsWireError::UnknownStatus)? {
            KmsResponseStatus::Ok if self.code == 0 => Ok(None),
            KmsResponseStatus::Ok => Err(KmsWireError::UnexpectedErrorCode(self.code)),
            KmsResponseStatus::Error if self.code == 0 => Err(KmsWireError::MissingErrorCode),
            KmsResponseStatus::Error => KmsErrorCode::try_from(self.code)
                .map(Some)
                .map_err(KmsWireError::UnknownErrorCode),
        }
    }

    pub fn payload(&self) -> Result<&[u8], KmsWireError> {
        self.validate()?;
        Ok(&self.payload[..self.payload_len as usize])
    }

    fn new(
        opcode: KmsOpcode,
        status: KmsResponseStatus,
        request_id: u32,
        code: u16,
        payload: &[u8],
    ) -> Result<Self, KmsWireError> {
        if payload.len() > KMS_PAYLOAD_LEN {
            return Err(KmsWireError::PayloadTooLong(payload.len() as u16));
        }
        let mut frame = Self {
            abi_version: KMS_ABI_VERSION,
            opcode: opcode as u8,
            status: status as u8,
            reserved0: 0,
            request_id,
            code,
            payload_len: payload.len() as u16,
            reserved1: 0,
            payload: [0; KMS_PAYLOAD_LEN],
        };
        frame.payload[..payload.len()].copy_from_slice(payload);
        Ok(frame)
    }

    fn validate(&self) -> Result<(), KmsWireError> {
        validate_header(self.abi_version, self.opcode, self.payload_len)?;
        self.error_code()?;
        if self.reserved0 != 0 || self.reserved1 != 0 {
            return Err(KmsWireError::NonZeroReserved);
        }
        canonical_tail(&self.payload, self.payload_len)
    }
}

const _: () = assert!(core::mem::size_of::<KmsResponseV1>() == KMS_MESSAGE_LEN);
