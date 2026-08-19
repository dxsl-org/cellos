use super::commands::{
    encode_echo_command, encode_hold_command, encode_snapshot_command, OracleCommand, OracleError,
};
use crate::local_ingress::{
    ReplyStatus, LOCAL_PAYLOAD_CAP, MAX_REQUEST_BODY, REQUEST_HEADER_LEN, RESPONSE_HEADER_LEN,
    STATUS_TAG,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodedReply<'a> {
    pub status: ReplyStatus,
    pub request_id: u64,
    pub client_sequence: u64,
    pub payload: &'a [u8],
}

pub fn encode_echo_request(
    client_sequence: u64,
    payload: &[u8],
    out: &mut [u8],
) -> Result<usize, OracleError> {
    encode_request(client_sequence, OracleCommand::Echo(payload), out)
}

pub fn encode_snapshot_request(client_sequence: u64, out: &mut [u8]) -> Result<usize, OracleError> {
    encode_request(client_sequence, OracleCommand::Snapshot, out)
}

pub fn encode_hold_request(
    client_sequence: u64,
    work_turns: u16,
    out: &mut [u8],
) -> Result<usize, OracleError> {
    encode_request(client_sequence, OracleCommand::Hold { work_turns }, out)
}

pub fn decode_reply_frame(buf: &[u8]) -> Result<DecodedReply<'_>, OracleError> {
    if buf.len() < RESPONSE_HEADER_LEN || buf.first().copied() != Some(STATUS_TAG) {
        return Err(OracleError::InvalidReplyTag);
    }
    let status = match buf[1] {
        0 => ReplyStatus::Success,
        1 => ReplyStatus::Busy,
        2 => ReplyStatus::Indeterminate,
        _ => return Err(OracleError::InvalidReplyStatus),
    };
    let request_id = u64::from_le_bytes(buf[2..10].try_into().unwrap_or([0; 8]));
    let client_sequence = u64::from_le_bytes(buf[10..18].try_into().unwrap_or([0; 8]));
    let payload_len = u16::from_le_bytes(buf[18..20].try_into().unwrap_or([0; 2])) as usize;
    if payload_len > crate::local_ingress::MAX_REPLY_BODY
        || RESPONSE_HEADER_LEN + payload_len > buf.len()
    {
        return Err(OracleError::TruncatedReply);
    }
    Ok(DecodedReply {
        status,
        request_id,
        client_sequence,
        payload: &buf[RESPONSE_HEADER_LEN..RESPONSE_HEADER_LEN + payload_len],
    })
}

fn encode_request(
    client_sequence: u64,
    command: OracleCommand<'_>,
    out: &mut [u8],
) -> Result<usize, OracleError> {
    if out.len() < REQUEST_HEADER_LEN + 1 {
        return Err(OracleError::OutputTooSmall);
    }
    let body_len = match command {
        OracleCommand::Echo(payload) => {
            if payload.len() + 1 > MAX_REQUEST_BODY {
                return Err(OracleError::PayloadTooLarge);
            }
            encode_echo_command(payload, &mut out[REQUEST_HEADER_LEN..])?
        }
        OracleCommand::Snapshot => encode_snapshot_command(&mut out[REQUEST_HEADER_LEN..])?,
        OracleCommand::Hold { work_turns } => {
            encode_hold_command(work_turns, &mut out[REQUEST_HEADER_LEN..])?
        }
    };
    let frame_len = REQUEST_HEADER_LEN + body_len;
    if frame_len > LOCAL_PAYLOAD_CAP {
        return Err(OracleError::PayloadTooLarge);
    }
    out[..8].copy_from_slice(&client_sequence.to_le_bytes());
    out[8..10].copy_from_slice(&(body_len as u16).to_le_bytes());
    Ok(frame_len)
}
