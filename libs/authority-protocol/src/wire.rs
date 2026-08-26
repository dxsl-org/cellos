//! Manual fixed-header codec with exact-frame consumption.
mod common;
mod payload;
mod request;
mod response;
mod types;

pub(crate) use request::CanonicalBody;
pub use request::TypedRequest;
pub use response::{verify_typed_response, TypedResponse, ValidatedResponse};
pub use types::*;

use crate::{
    message::max_payload_len, AuthorityFault, WireError, FRAME_HEADER_LEN, FRAME_MAGIC,
    FRAME_MAX_PAYLOAD, LANE_DEV_REFERENCE, PROTOCOL_VERSION,
};

/// Decode exactly one complete frame; trailing bytes are rejected.
pub(crate) fn decode_frame(bytes: &[u8]) -> Result<DecodedFrame<'_>, WireError> {
    if bytes.len() < FRAME_HEADER_LEN {
        return Err(WireError::Truncated);
    }
    if bytes[..4] != FRAME_MAGIC {
        return Err(WireError::BadMagic);
    }
    if bytes[4] != PROTOCOL_VERSION {
        return Err(WireError::UnsupportedVersion);
    }
    if bytes[5] != LANE_DEV_REFERENCE {
        return Err(WireError::UnknownLaneTag);
    }
    let class = FrameClass::try_from(bytes[6]).map_err(|_| WireError::UnknownMessageKind)?;
    let operation = Operation::try_from(bytes[7]).map_err(|_| WireError::UnknownOperation)?;
    if bytes[10] != 0 || bytes[11] != 0 {
        return Err(WireError::NonZeroReserved);
    }
    let payload_len = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    validate_length(class, operation, payload_len)?;
    let total = FRAME_HEADER_LEN
        .checked_add(payload_len)
        .ok_or(WireError::OversizePayload)?;
    if bytes.len() < total {
        return Err(WireError::Truncated);
    }
    if bytes.len() > total {
        return Err(WireError::TrailingBytes);
    }
    let mut authenticator = [0u8; 16];
    authenticator.copy_from_slice(&bytes[20..36]);
    let request_id = u64::from_le_bytes(bytes[12..20].try_into().expect("fixed request id"));
    Ok(DecodedFrame {
        header: FrameHeader {
            class,
            operation,
            payload_len: payload_len as u16,
            request_id,
            authenticator,
        },
        payload: &bytes[36..],
    })
}

pub fn encode_typed_request(
    header: FrameHeader,
    request: &TypedRequest,
    output: &mut [u8],
) -> Result<usize, WireError> {
    if header.class != FrameClass::Request || header.operation != request.operation() {
        return Err(WireError::UnknownOperation);
    }
    encode_typed(header, output, |payload| request.encode_payload(payload))
}

pub fn encode_typed_response(
    header: FrameHeader,
    response: &TypedResponse,
    output: &mut [u8],
) -> Result<usize, WireError> {
    if header.class != FrameClass::Response || header.operation != response.operation() {
        return Err(WireError::UnknownOperation);
    }
    encode_typed(header, output, |payload| response.encode_payload(payload))
}

pub fn decode_typed_request(bytes: &[u8]) -> Result<(FrameHeader, TypedRequest), WireError> {
    let frame = decode_frame(bytes)?;
    if frame.header.class != FrameClass::Request {
        return Err(WireError::UnknownMessageKind);
    }
    let request = TypedRequest::decode_payload(frame.header.operation, frame.payload)?;
    let context = request.context();
    if context.request_id != frame.header.request_id
        || context.authenticator[..16] != frame.header.authenticator
    {
        return Err(WireError::InvalidLength);
    }
    Ok((frame.header, request))
}

pub fn decode_typed_response(bytes: &[u8]) -> Result<(FrameHeader, TypedResponse), WireError> {
    let frame = decode_frame(bytes)?;
    if frame.header.class != FrameClass::Response {
        return Err(WireError::UnknownMessageKind);
    }
    let response = TypedResponse::decode_payload(frame.header.operation, frame.payload)?;
    let binding = response.binding();
    if binding.request_id != frame.header.request_id
        || binding.authority_signature[..16] != frame.header.authenticator
    {
        return Err(WireError::InvalidLength);
    }
    Ok((frame.header, response))
}

pub fn encode_fault_frame(
    header: FrameHeader,
    fault: AuthorityFault,
    output: &mut [u8],
) -> Result<usize, WireError> {
    if header.class != FrameClass::Fault || header.payload_len != 2 {
        return Err(WireError::InvalidLength);
    }
    encode_typed(header, output, |payload| {
        let bytes = payload.get_mut(..2).ok_or(WireError::BufferTooSmall)?;
        bytes.copy_from_slice(&(fault as u16).to_le_bytes());
        Ok(2)
    })
}

pub fn decode_fault_frame(bytes: &[u8]) -> Result<(FrameHeader, AuthorityFault), WireError> {
    let frame = decode_frame(bytes)?;
    if frame.header.class != FrameClass::Fault {
        return Err(WireError::UnknownMessageKind);
    }
    Ok((frame.header, decode_fault(frame.payload)?))
}

fn encode_typed(
    header: FrameHeader,
    output: &mut [u8],
    encode: impl FnOnce(&mut [u8]) -> Result<usize, WireError>,
) -> Result<usize, WireError> {
    if output.len() < FRAME_HEADER_LEN {
        return Err(WireError::BufferTooSmall);
    }
    let payload_len = encode(&mut output[FRAME_HEADER_LEN..])?;
    if header.payload_len as usize != payload_len {
        return Err(WireError::InvalidLength);
    }
    validate_length(header.class, header.operation, payload_len)?;
    encode_header(header, output)?;
    Ok(FRAME_HEADER_LEN + payload_len)
}

fn encode_header(header: FrameHeader, output: &mut [u8]) -> Result<(), WireError> {
    let bytes = output
        .get_mut(..FRAME_HEADER_LEN)
        .ok_or(WireError::BufferTooSmall)?;
    bytes.fill(0);
    bytes[..4].copy_from_slice(&FRAME_MAGIC);
    bytes[4] = PROTOCOL_VERSION;
    bytes[5] = LANE_DEV_REFERENCE;
    bytes[6] = header.class as u8;
    bytes[7] = header.operation as u8;
    bytes[8..10].copy_from_slice(&header.payload_len.to_le_bytes());
    bytes[12..20].copy_from_slice(&header.request_id.to_le_bytes());
    bytes[20..36].copy_from_slice(&header.authenticator);
    Ok(())
}

fn validate_length(class: FrameClass, operation: Operation, len: usize) -> Result<(), WireError> {
    if len > FRAME_MAX_PAYLOAD || len > max_payload_len(operation) {
        return Err(WireError::OversizePayload);
    }
    if class == FrameClass::Fault && len != core::mem::size_of::<u16>() {
        return Err(WireError::InvalidLength);
    }
    Ok(())
}

/// Decode a two-byte typed fault payload.
pub fn decode_fault(payload: &[u8]) -> Result<AuthorityFault, WireError> {
    if payload.len() != 2 {
        return Err(WireError::InvalidLength);
    }
    AuthorityFault::try_from(u16::from_le_bytes([payload[0], payload[1]]))
        .map_err(|_| WireError::UnknownFault)
}
