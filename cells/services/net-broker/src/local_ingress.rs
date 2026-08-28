use api::ipc::IPC_BUF_SIZE;

pub const LOCAL_PAYLOAD_CAP: usize = IPC_BUF_SIZE - api::caller_identity::CALLER_IDENTITY_LEN;
pub const REQUEST_HEADER_LEN: usize = 10;
pub const RESPONSE_HEADER_LEN: usize = 20;
pub const MAX_REQUEST_BODY: usize = LOCAL_PAYLOAD_CAP - REQUEST_HEADER_LEN;
pub const MAX_REPLY_BODY: usize = IPC_BUF_SIZE - RESPONSE_HEADER_LEN;
pub const STATUS_TAG: u8 = 0x7F;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyStatus {
    Success = 0,
    Busy = 1,
    Indeterminate = 2,
    NotSupported = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    ShortBuffer,
    TruncatedPayload,
    PayloadTooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedLocalRequest {
    pub client_sequence: u64,
    pub payload_len: usize,
    pub payload: [u8; MAX_REQUEST_BODY],
}

impl ParsedLocalRequest {
    pub fn zero() -> Self {
        Self {
            client_sequence: 0,
            payload_len: 0,
            payload: [0; MAX_REQUEST_BODY],
        }
    }
}

pub fn parse_request(buf: &[u8]) -> Result<ParsedLocalRequest, ParseError> {
    if buf.len() < REQUEST_HEADER_LEN {
        return Err(ParseError::ShortBuffer);
    }
    let mut seq = [0u8; 8];
    seq.copy_from_slice(&buf[..8]);
    let mut len = [0u8; 2];
    len.copy_from_slice(&buf[8..10]);
    let payload_len = u16::from_le_bytes(len) as usize;
    if payload_len > MAX_REQUEST_BODY {
        return Err(ParseError::PayloadTooLarge);
    }
    let mut payload = [0u8; MAX_REQUEST_BODY];
    let end = REQUEST_HEADER_LEN + payload_len;
    if end > buf.len() {
        return Err(ParseError::TruncatedPayload);
    }
    payload[..payload_len].copy_from_slice(&buf[REQUEST_HEADER_LEN..end]);
    Ok(ParsedLocalRequest {
        client_sequence: u64::from_le_bytes(seq),
        payload_len,
        payload,
    })
}

pub fn encode_reply(
    status: ReplyStatus,
    request_id: u64,
    client_sequence: u64,
    payload: &[u8],
    out: &mut [u8; IPC_BUF_SIZE],
) -> usize {
    let payload_len = payload.len().min(MAX_REPLY_BODY);
    out.fill(0);
    out[0] = STATUS_TAG;
    out[1] = status as u8;
    out[2..10].copy_from_slice(&request_id.to_le_bytes());
    out[10..18].copy_from_slice(&client_sequence.to_le_bytes());
    out[18..20].copy_from_slice(&(payload_len as u16).to_le_bytes());
    out[20..20 + payload_len].copy_from_slice(&payload[..payload_len]);
    RESPONSE_HEADER_LEN + payload_len
}

#[cfg(test)]
mod tests;
