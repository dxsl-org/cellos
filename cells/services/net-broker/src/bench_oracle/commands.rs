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
}

pub const OP_ECHO: u8 = 0x01;
pub const OP_SNAPSHOT: u8 = 0x02;
pub const OP_HOLD: u8 = 0x03;
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
