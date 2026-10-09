//! Bounded IPC frames between the DWC2 transport cell and the LAN9514 NIC cell.
//!
//! The host retains USB endpoint/DMA ownership. The NIC cell owns service
//! registration and protocol dispatch, so a NIC failure cannot take HID parsing
//! down with it.

pub const OP_ATTACH: u8 = 0xD0;
pub const OP_REQUEST: u8 = 0xD1;
pub const OP_RESPONSE: u8 = 0xD2;
const TID_BYTES: usize = 8;
const OP_BYTES: usize = 8;
const LEN_BYTES: usize = 2;

/// Largest encoded `OP_REQUEST`: the envelope plus the widest NIC command
/// (`OP_TX` header and a maximum-length Ethernet frame).
pub const MAX_REQUEST: usize =
    1 + TID_BYTES + OP_BYTES + LEN_BYTES + 3 + crate::dispatch::FRAME_BUF;

pub fn encode_attach(out: &mut [u8; 1]) {
    out[0] = OP_ATTACH;
}

pub fn is_attach(frame: &[u8]) -> bool {
    frame.len() == 1 && frame[0] == OP_ATTACH
}

pub fn encode_request(
    client_tid: usize,
    operation: Option<usize>,
    payload: &[u8],
    out: &mut [u8],
) -> Option<usize> {
    let start = 1 + TID_BYTES + OP_BYTES + LEN_BYTES;
    if payload.len() > u16::MAX as usize || out.len() < start + payload.len() {
        return None;
    }
    out[0] = OP_REQUEST;
    out[1..1 + TID_BYTES].copy_from_slice(&(client_tid as u64).to_le_bytes());
    out[1 + TID_BYTES..1 + TID_BYTES + OP_BYTES]
        .copy_from_slice(&(operation.unwrap_or(0) as u64).to_le_bytes());
    out[start - LEN_BYTES..start].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    out[start..start + payload.len()].copy_from_slice(payload);
    Some(start + payload.len())
}

pub fn decode_request(frame: &[u8]) -> Option<(usize, Option<usize>, &[u8])> {
    let start = 1 + TID_BYTES + OP_BYTES + LEN_BYTES;
    if frame.len() < start || frame[0] != OP_REQUEST {
        return None;
    }
    let tid = u64::from_le_bytes(frame[1..1 + TID_BYTES].try_into().ok()?) as usize;
    let operation = u64::from_le_bytes(
        frame[1 + TID_BYTES..1 + TID_BYTES + OP_BYTES].try_into().ok()?,
    ) as usize;
    let len = u16::from_le_bytes(frame[start - LEN_BYTES..start].try_into().ok()?) as usize;
    if frame.len() < start + len {
        return None;
    }
    Some((tid, (operation != 0).then_some(operation), &frame[start..start + len]))
}


#[cfg(test)]
mod padded_tests {
    use super::*;

    #[test]
    fn decode_uses_declared_payload_not_receive_padding() {
        let mut frame = [0u8; 64];
        let len = encode_request(42, Some(99), &[7, 8], &mut frame).expect("fits");
        assert_eq!(len, 21);
        assert_eq!(decode_request(&frame), Some((42, Some(99), &[7, 8][..])));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_only_the_embedded_client_payload() {
        let mut frame = [0u8; 32];
        let len = encode_request(42, None, &[7, 8], &mut frame).expect("fits");
        assert_eq!(decode_request(&frame[..len]), Some((42, None, &[7, 8][..])));
        frame[0] = OP_RESPONSE;
        assert_eq!(decode_request(&frame[..len]), None);
    }
}
