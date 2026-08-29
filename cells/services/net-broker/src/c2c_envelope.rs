// SPDX-License-Identifier: Apache-2.0
//! Canonical bounded V1 envelope for authenticated C2C transport records.

use api::services::cluster::{CellNetId, ClusterId};
pub use types::c2c::{RelativeDeadline, RetryClass, ServerEpoch};

pub const C2C_VERSION: u8 = 1;
pub const C2C_HEADER_LEN: usize = 112;
// V1 forbids streaming. The frame must fit both the attested local request body
// and one conservative net-cell TcpSend after Noise adds its AEAD tag.
pub const NOISE_TAG_LEN: usize = 16;
const NOISE_PLAINTEXT_CAP: usize = api::ipc::NET_TCP_INLINE_DATA_MAX - NOISE_TAG_LEN;
const NOISE_BODY_CAP: usize = NOISE_PLAINTEXT_CAP - C2C_HEADER_LEN;
pub const MAX_C2C_PAYLOAD: usize = if crate::local_ingress::MAX_REQUEST_BODY < NOISE_BODY_CAP {
    crate::local_ingress::MAX_REQUEST_BODY
} else {
    NOISE_BODY_CAP
};
pub const MAX_C2C_FRAME: usize = C2C_HEADER_LEN + MAX_C2C_PAYLOAD;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EnvelopeKind {
    Lookup = 1,
    Request = 2,
    Response = 3,
    Busy = 4,
    Indeterminate = 5,
    Cancel = 6,
    Heartbeat = 7,
}

impl EnvelopeKind {
    fn decode(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Lookup,
            2 => Self::Request,
            3 => Self::Response,
            4 => Self::Busy,
            5 => Self::Indeterminate,
            6 => Self::Cancel,
            7 => Self::Heartbeat,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeError {
    ShortBuffer,
    UnsupportedVersion,
    UnknownKind,
    UnknownRetryClass,
    NonCanonical,
    InvalidIdentity,
    PayloadTooLarge,
    LengthMismatch,
}

/// Borrowed decoded envelope. Identity is trusted only after Noise authenticates the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct C2cEnvelope<'a> {
    pub kind: EnvelopeKind,
    pub retry_class: RetryClass,
    pub request_id: u64,
    pub src_node: CellNetId,
    pub dst_node: CellNetId,
    pub src_boot_epoch: u64,
    pub dst_server_epoch: ServerEpoch,
    pub cluster_id: ClusterId,
    pub service_id: u16,
    pub export_id: u16,
    pub relative_deadline: RelativeDeadline,
    pub payload: &'a [u8],
}

impl C2cEnvelope<'_> {
    /// Encode one canonical frame into `out`.
    ///
    /// Returns the encoded length or a deterministic validation error.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, EnvelopeError> {
        validate(self)?;
        let len = C2C_HEADER_LEN + self.payload.len();
        if out.len() < len {
            return Err(EnvelopeError::ShortBuffer);
        }
        out[..len].fill(0);
        out[0] = C2C_VERSION;
        out[1] = self.kind as u8;
        out[2] = self.retry_class as u8;
        put_u16(out, 4, self.payload.len() as u16);
        put_u64(out, 8, self.request_id);
        out[16..48].copy_from_slice(&self.src_node.0);
        out[48..80].copy_from_slice(&self.dst_node.0);
        put_u64(out, 80, self.src_boot_epoch);
        put_u64(out, 88, self.dst_server_epoch.get());
        put_u64(out, 96, self.cluster_id.0);
        put_u16(out, 104, self.service_id);
        put_u16(out, 106, self.export_id);
        out[108..112].copy_from_slice(&self.relative_deadline.milliseconds().to_le_bytes());
        out[C2C_HEADER_LEN..len].copy_from_slice(self.payload);
        Ok(len)
    }
}

/// Decode one exact canonical V1 frame without allocation.
pub fn decode(bytes: &[u8]) -> Result<C2cEnvelope<'_>, EnvelopeError> {
    if bytes.len() < C2C_HEADER_LEN {
        return Err(EnvelopeError::ShortBuffer);
    }
    if bytes[0] != C2C_VERSION {
        return Err(EnvelopeError::UnsupportedVersion);
    }
    let kind = EnvelopeKind::decode(bytes[1]).ok_or(EnvelopeError::UnknownKind)?;
    let retry_class = RetryClass::from_wire(bytes[2]).ok_or(EnvelopeError::UnknownRetryClass)?;
    if bytes[3] != 0 || bytes[6..8] != [0; 2] {
        return Err(EnvelopeError::NonCanonical);
    }
    let payload_len = read_u16(bytes, 4) as usize;
    if payload_len > MAX_C2C_PAYLOAD {
        return Err(EnvelopeError::PayloadTooLarge);
    }
    if bytes.len() != C2C_HEADER_LEN + payload_len {
        return Err(EnvelopeError::LengthMismatch);
    }
    let envelope = C2cEnvelope {
        kind,
        retry_class,
        request_id: read_u64(bytes, 8),
        src_node: CellNetId(bytes[16..48].try_into().expect("fixed range")),
        dst_node: CellNetId(bytes[48..80].try_into().expect("fixed range")),
        src_boot_epoch: read_u64(bytes, 80),
        dst_server_epoch: ServerEpoch::new(read_u64(bytes, 88))
            .ok_or(EnvelopeError::InvalidIdentity)?,
        cluster_id: ClusterId(read_u64(bytes, 96)),
        service_id: read_u16(bytes, 104),
        export_id: read_u16(bytes, 106),
        relative_deadline: RelativeDeadline::new(u32::from_le_bytes(
            bytes[108..112].try_into().expect("fixed range"),
        ))
        .ok_or(EnvelopeError::InvalidIdentity)?,
        payload: &bytes[C2C_HEADER_LEN..],
    };
    validate(&envelope)?;
    Ok(envelope)
}

fn validate(envelope: &C2cEnvelope<'_>) -> Result<(), EnvelopeError> {
    if envelope.payload.len() > MAX_C2C_PAYLOAD {
        return Err(EnvelopeError::PayloadTooLarge);
    }
    if envelope.request_id == 0
        || envelope.src_node.0.iter().all(|byte| *byte == 0)
        || envelope.dst_node.0.iter().all(|byte| *byte == 0)
        || envelope.src_boot_epoch == 0
        || envelope.cluster_id.0 == 0
        || envelope.service_id == 0
        || envelope.export_id == 0
    {
        return Err(EnvelopeError::InvalidIdentity);
    }
    Ok(())
}

fn put_u16(out: &mut [u8], offset: usize, value: u16) {
    out[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut [u8], offset: usize, value: u64) {
    out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("fixed range"))
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("fixed range"))
}

#[cfg(test)]
mod tests;
