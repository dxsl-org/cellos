#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OracleCommand<'a> {
    Echo(&'a [u8]),
    Snapshot,
    Hold { work_turns: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OracleError {
    Empty,
    UnknownOpcode,
    TruncatedHold,
    HoldTooLarge,
    OutputTooSmall,
    PayloadTooLarge,
    InvalidReplyTag,
    InvalidReplyStatus,
    TruncatedReply,
    SnapshotVersionMismatch,
    SnapshotSizeMismatch,
    EchoPayloadMismatch,
}

pub const OP_ECHO: u8 = 0x01;
pub const OP_SNAPSHOT: u8 = 0x02;
pub const OP_HOLD: u8 = 0x03;
#[cfg(feature = "restart-oracle")]
pub const OP_RESTART: u8 = 0x7E;
pub const OP_TIMED_ECHO_REPLY: u8 = 0x81;
pub const TIMED_ECHO_TRAILER_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedEchoTimestamps {
    pub worker_done_ticks: u64,
    pub reply_send_ticks: u64,
}
// Keep saturation long enough to fill ingress without exceeding the broker's
// role heartbeat window on a cooperative single-hart scheduler.
pub const MAX_HOLD_TURNS: u16 = 512;

pub fn encode_echo_command(payload: &[u8], out: &mut [u8]) -> Result<usize, OracleError> {
    if out.len() < payload.len() + 1 {
        return Err(OracleError::OutputTooSmall);
    }
    out[0] = OP_ECHO;
    out[1..1 + payload.len()].copy_from_slice(payload);
    Ok(payload.len() + 1)
}

pub fn encode_timed_echo_reply(
    payload: &[u8],
    worker_done_ticks: u64,
    out: &mut [u8],
) -> Result<usize, OracleError> {
    let command_len = 1 + payload.len();
    let reply_len = command_len + TIMED_ECHO_TRAILER_BYTES;
    if out.len() < reply_len {
        return Err(OracleError::OutputTooSmall);
    }
    out[0] = OP_TIMED_ECHO_REPLY;
    out[1..command_len].copy_from_slice(payload);
    out[command_len..command_len + 8].copy_from_slice(&worker_done_ticks.to_le_bytes());
    out[command_len + 8..reply_len].fill(0);
    Ok(reply_len)
}

pub fn decode_timed_echo_reply(
    payload: &[u8],
    expected_body: &[u8],
) -> Result<TimedEchoTimestamps, OracleError> {
    let expected_len = 1 + expected_body.len() + TIMED_ECHO_TRAILER_BYTES;
    if payload.len() != expected_len || payload.first().copied() != Some(OP_TIMED_ECHO_REPLY) {
        return Err(OracleError::TruncatedReply);
    }
    if payload.get(1..1 + expected_body.len()) != Some(expected_body) {
        return Err(OracleError::EchoPayloadMismatch);
    }
    let trailer: [u8; TIMED_ECHO_TRAILER_BYTES] = payload
        .get(1 + expected_body.len()..expected_len)
        .ok_or(OracleError::TruncatedReply)?
        .try_into()
        .map_err(|_| OracleError::TruncatedReply)?;
    let worker_done = trailer[..8]
        .try_into()
        .map_err(|_| OracleError::TruncatedReply)?;
    let reply_send = trailer[8..]
        .try_into()
        .map_err(|_| OracleError::TruncatedReply)?;
    Ok(TimedEchoTimestamps {
        worker_done_ticks: u64::from_le_bytes(worker_done),
        reply_send_ticks: u64::from_le_bytes(reply_send),
    })
}

pub fn stamp_timed_echo_reply(
    payload: &mut [u8],
    reply_send_ticks: u64,
) -> Result<(), OracleError> {
    if payload.len() < 1 + TIMED_ECHO_TRAILER_BYTES
        || payload.first().copied() != Some(OP_TIMED_ECHO_REPLY)
    {
        return Err(OracleError::TruncatedReply);
    }
    let stamp_offset = payload.len() - 8;
    payload[stamp_offset..].copy_from_slice(&reply_send_ticks.to_le_bytes());
    Ok(())
}

pub fn encode_snapshot_command(out: &mut [u8]) -> Result<usize, OracleError> {
    if out.is_empty() {
        return Err(OracleError::OutputTooSmall);
    }
    out[0] = OP_SNAPSHOT;
    Ok(1)
}

pub fn encode_hold_command(work_turns: u16, out: &mut [u8]) -> Result<usize, OracleError> {
    if work_turns > MAX_HOLD_TURNS {
        return Err(OracleError::HoldTooLarge);
    }
    if out.len() < 3 {
        return Err(OracleError::OutputTooSmall);
    }
    out[0] = OP_HOLD;
    out[1..3].copy_from_slice(&work_turns.to_le_bytes());
    Ok(3)
}

pub fn parse_command(payload: &[u8]) -> Result<OracleCommand<'_>, OracleError> {
    let Some((&opcode, body)) = payload.split_first() else {
        return Err(OracleError::Empty);
    };
    match opcode {
        OP_ECHO => Ok(OracleCommand::Echo(body)),
        OP_SNAPSHOT => Ok(OracleCommand::Snapshot),
        OP_HOLD => {
            let bytes = body.get(..2).ok_or(OracleError::TruncatedHold)?;
            let work_turns = u16::from_le_bytes([bytes[0], bytes[1]]);
            if work_turns > MAX_HOLD_TURNS {
                return Err(OracleError::HoldTooLarge);
            }
            Ok(OracleCommand::Hold { work_turns })
        }
        _ => Err(OracleError::UnknownOpcode),
    }
}
