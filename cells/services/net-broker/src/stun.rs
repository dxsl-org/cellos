// SPDX-License-Identifier: Apache-2.0
//! STUN Binding Request client (RFC 8489, §5.3).
//!
//! Discovers the machine's reflexive (public) IPv4:port behind NAT.
//! UDP socket is opened, queried, then closed — zero persistent socket budget.
//!
//! ## UdpRecv header
//! The net cell prepends a 6-byte src-addr header to every UDP receive buffer:
//!   [0..4] source IPv4 (BE)
//!   [4..6] source port (BE)
//! STUN parsing skips these 6 bytes before reading the STUN frame.

use api::ipc::{NetRequest, NetResponse};
use ostd::service::NetRef;
use ostd::syscall::sys_heartbeat;
use ostd::{ViError, ViResult};

const HEARTBEAT_MS: u64 = 500;
const STUN_MAGIC:   u32 = 0x2112_A442;

/// Query a STUN server for the machine's reflexive IPv4:port.
///
/// Opens a transient UDP socket (close on return, no steady-state cost).
/// `rng_bytes` must contain ≥ 12 bytes of random data for the transaction ID.
pub fn query_reflexive_addr(
    net: &mut NetRef,
    stun_ip:   [u8; 4],
    stun_port: u16,
    rng_bytes: &[u8; 12],
) -> ViResult<([u8; 4], u16)> {
    let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];

    // 1. Create UDP socket.
    let cap_id = match net.call::<NetRequest, NetResponse>(
        &NetRequest::UdpCreate, &mut resp,
    ).map_err(|_| ViError::IO)? {
        NetResponse::CapId(id) => id,
        _ => return Err(ViError::IO),
    };

    let result = do_stun_query(net, cap_id, stun_ip, stun_port, rng_bytes, &mut resp);

    // Always close the socket (even on error).
    let _ = net.call::<NetRequest, NetResponse>(&NetRequest::TcpClose { cap_id }, &mut resp);
    result
}

fn do_stun_query(
    net:       &mut NetRef,
    cap_id:    u32,
    stun_ip:   [u8; 4],
    stun_port: u16,
    tx_id:     &[u8; 12],
    resp:      &mut [u8; api::ipc::IPC_BUF_SIZE],
) -> ViResult<([u8; 4], u16)> {
    // 2. Build 20-byte STUN Binding Request.
    let mut req = [0u8; 20];
    req[0..2].copy_from_slice(&0x0001u16.to_be_bytes()); // type = Binding Request
    req[2..4].copy_from_slice(&0x0000u16.to_be_bytes()); // length = 0 (no attrs)
    req[4..8].copy_from_slice(&STUN_MAGIC.to_be_bytes());
    req[8..20].copy_from_slice(tx_id);

    // 3. Send.
    sys_heartbeat(HEARTBEAT_MS);
    net.call::<NetRequest, NetResponse>(
        &NetRequest::UdpSend { cap_id, addr: stun_ip, port: stun_port, data: &req },
        resp,
    ).map_err(|_| ViError::IO)?;

    // 4. Receive (up to 512B; account for 6-byte src-addr header).
    sys_heartbeat(HEARTBEAT_MS);
    let raw = match net.call::<NetRequest, NetResponse>(
        &NetRequest::UdpRecv { cap_id, buf_len: 512 },
        resp,
    ).map_err(|_| ViError::IO)? {
        NetResponse::Data(d) => d,
        _ => return Err(ViError::IO),
    };

    parse_stun_response(raw, tx_id)
}

/// Parse STUN Binding Response from raw UDP data (with 6-byte src header).
fn parse_stun_response(raw: &[u8], tx_id: &[u8; 12]) -> ViResult<([u8; 4], u16)> {
    // Skip 6-byte UdpRecv src header injected by the net cell.
    if raw.len() < 6 + 20 { return Err(ViError::IO); }
    let frame = &raw[6..];

    // Validate STUN message type = 0x0101 (Binding Success Response).
    if u16::from_be_bytes([frame[0], frame[1]]) != 0x0101 { return Err(ViError::IO); }
    let msg_len = u16::from_be_bytes([frame[2], frame[3]]) as usize;
    if u32::from_be_bytes([frame[4], frame[5], frame[6], frame[7]]) != STUN_MAGIC {
        return Err(ViError::IO);
    }
    if &frame[8..20] != tx_id { return Err(ViError::IO); } // transaction ID mismatch
    if frame.len() < 20 + msg_len { return Err(ViError::IO); }

    // Walk TLV attributes looking for XOR-MAPPED-ADDRESS (0x0020).
    let attrs = &frame[20..20 + msg_len];
    let mut pos = 0;
    while pos + 4 <= attrs.len() {
        let attr_type = u16::from_be_bytes([attrs[pos], attrs[pos + 1]]);
        let attr_len  = u16::from_be_bytes([attrs[pos + 2], attrs[pos + 3]]) as usize;
        pos += 4;
        if pos + attr_len > attrs.len() { break; }
        let attr_val = &attrs[pos..pos + attr_len];

        if attr_type == 0x0020 && attr_len >= 8 {
            // attr_val: [0]=reserved [1]=family(0x01=IPv4) [2..4]=XOR port [4..8]=XOR addr
            if attr_val[1] != 0x01 { return Err(ViError::NotSupported); }
            let magic_bytes = STUN_MAGIC.to_be_bytes();
            let xport = u16::from_be_bytes([attr_val[2], attr_val[3]]);
            let port  = xport ^ u16::from_be_bytes([magic_bytes[0], magic_bytes[1]]);
            let xip = u32::from_be_bytes([attr_val[4], attr_val[5], attr_val[6], attr_val[7]]);
            let ip_u32 = xip ^ STUN_MAGIC;
            let ip = ip_u32.to_be_bytes();
            return Ok((ip, port));
        }

        // Attributes are padded to 4-byte boundaries.
        pos += (attr_len + 3) & !3;
    }

    Err(ViError::NotFound)
}
