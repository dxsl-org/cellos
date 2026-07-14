// SPDX-License-Identifier: Apache-2.0
//! Cellos relay client — raw TCP relay for internet connectivity.
//!
//! Implements a simple relay protocol over raw TCP (no WebSocket).
//! Payload on the relay wire is Noise-encrypted end-to-end; the relay only
//! sees NodeIds and byte counts.
//!
//! ## Frame format (all fields big-endian)
//! ```text
//! [4B length (u32 BE)][1B frame_type][data]
//! ```
//!
//! Frame types:
//!   CLIENT_REGISTER (0x01): node_id(32)        — register with relay
//!   SERVER_ACK      (0x02): status(1)           — 0x00 = ok
//!   SEND_PACKET     (0x08): dest_node_id(32) + payload(N)
//!   RECV_PACKET     (0x09): src_node_id(32)  + payload(N)
//!   PING            (0x0b): data(8)
//!   PONG            (0x0c): data(8)
//!
//! ## Blocking contract
//! `recv_one` BLOCKS until a frame arrives or the connection closes.
//! Re-arm sys_heartbeat before calling to prevent watchdog expiry.
//! Call `recv_one` only when the dispatch loop has a free slot — not on every
//! iteration. See `main.rs` for the recommended polling cadence.

// reason: this module implements the raw-TCP relay fallback protocol for the
// net-broker robot-swarm feature (internet relay when peers aren't LAN-reachable).
// `main.rs` imports and calls only `RelayClient::new` + `is_connected` (main.rs:71,
// 122, 132) as a liveness stub; the frame types, `recv_one`, and send/register paths
// are unused because inbound relay frames are not yet wired into dispatch
// (main.rs:131 TODO). Partially connected, not fully wired.
#![allow(dead_code)]

use api::cluster::CellNetId;
use api::ipc::{NetRequest, NetResponse};
use ostd::service::NetRef;
use ostd::syscall::sys_heartbeat;
use ostd::{ViError, ViResult};

const HEARTBEAT_MS: u64 = 500;

const FT_CLIENT_REGISTER: u8 = 0x01;
const FT_SERVER_ACK: u8 = 0x02;
const FT_SEND_PACKET: u8 = 0x08;
const FT_RECV_PACKET: u8 = 0x09;
const FT_PING: u8 = 0x0b;
const FT_PONG: u8 = 0x0c;

const MAX_FRAME: usize = api::ipc::IPC_BUF_SIZE - 32; // leave room for NodeId header

/// Cellos relay client. Holds one persistent TCP connection to a relay server.
pub struct RelayClient {
    pub node_id: CellNetId,
    relay_ip: [u8; 4],
    relay_port: u16,
    tcp_cap: Option<u32>,
}

impl RelayClient {
    pub fn new(node_id: CellNetId, relay_ip: [u8; 4], relay_port: u16) -> Self {
        Self {
            node_id,
            relay_ip,
            relay_port,
            tcp_cap: None,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.tcp_cap.is_some()
    }

    /// Connect to the relay and register our NodeId.
    pub fn connect(&mut self, net: &mut NetRef) -> ViResult<()> {
        if self.tcp_cap.is_some() {
            return Ok(());
        }

        let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];
        sys_heartbeat(HEARTBEAT_MS);
        let cap = match net
            .call::<NetRequest, NetResponse>(
                &NetRequest::TcpConnect {
                    addr: self.relay_ip,
                    port: self.relay_port,
                },
                &mut resp,
            )
            .map_err(|_| ViError::IO)?
        {
            NetResponse::CapId(id) => id,
            _ => return Err(ViError::IO),
        };

        // CLIENT_REGISTER: [4B len=33][0x01][node_id(32)]
        let mut reg = [0u8; 37];
        reg[0..4].copy_from_slice(&33u32.to_be_bytes());
        reg[4] = FT_CLIENT_REGISTER;
        reg[5..37].copy_from_slice(&self.node_id.0);
        sys_heartbeat(HEARTBEAT_MS);
        net.call::<NetRequest, NetResponse>(
            &NetRequest::TcpSend {
                cap_id: cap,
                data: &reg,
            },
            &mut resp,
        )
        .map_err(|_| ViError::IO)?;

        // Read SERVER_ACK frame (2 bytes body: type + status).
        let mut frame_buf = [0u8; api::ipc::IPC_BUF_SIZE];
        let n = recv_frame_into(net, cap, &mut frame_buf)?;
        if n < 2 || frame_buf[0] != FT_SERVER_ACK || frame_buf[1] != 0x00 {
            let _ = net
                .call::<NetRequest, NetResponse>(&NetRequest::TcpClose { cap_id: cap }, &mut resp);
            return Err(ViError::IO);
        }

        self.tcp_cap = Some(cap);
        Ok(())
    }

    /// Send a Noise-encrypted payload to `dest` via the relay.
    pub fn send(&mut self, net: &mut NetRef, dest: &CellNetId, payload: &[u8]) -> ViResult<()> {
        let cap = self.tcp_cap.ok_or(ViError::IO)?;
        if payload.len() > MAX_FRAME {
            return Err(ViError::InvalidArgument);
        }

        // Header: [4B len = 1 + 32 + payload_len][0x08][dest(32)]
        let data_len: u32 = (1 + 32 + payload.len()) as u32;
        let mut hdr = [0u8; 37];
        hdr[0..4].copy_from_slice(&data_len.to_be_bytes());
        hdr[4] = FT_SEND_PACKET;
        hdr[5..37].copy_from_slice(&dest.0);

        let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];
        sys_heartbeat(HEARTBEAT_MS);
        net.call::<NetRequest, NetResponse>(
            &NetRequest::TcpSend {
                cap_id: cap,
                data: &hdr,
            },
            &mut resp,
        )
        .map_err(|_| ViError::IO)?;
        net.call::<NetRequest, NetResponse>(
            &NetRequest::TcpSend {
                cap_id: cap,
                data: payload,
            },
            &mut resp,
        )
        .map_err(|_| ViError::IO)?;
        Ok(())
    }

    /// Receive one relay frame (BLOCKS until data arrives or connection closes).
    ///
    /// Re-arm heartbeat before calling. Returns:
    /// - `Ok(Some((src_node_id, payload_end)))` — RECV_PACKET received; payload is
    ///   stored in `buf[33..payload_end]`.
    /// - `Ok(None)` — PING (handled internally) or unknown frame type.
    /// - `Err(_)` — connection lost; caller should mark relay as disconnected.
    pub fn recv_one(
        &mut self,
        net: &mut NetRef,
        buf: &mut [u8; api::ipc::IPC_BUF_SIZE],
    ) -> ViResult<Option<(CellNetId, usize)>> {
        let cap = self.tcp_cap.ok_or(ViError::IO)?;
        sys_heartbeat(HEARTBEAT_MS);
        let n = match recv_frame_into(net, cap, buf) {
            Ok(n) => n,
            Err(e) => {
                self.tcp_cap = None;
                return Err(e);
            }
        };
        if n == 0 {
            self.tcp_cap = None;
            return Err(ViError::IO);
        }

        match buf[0] {
            FT_RECV_PACKET if n >= 33 => {
                let mut src = [0u8; 32];
                src.copy_from_slice(&buf[1..33]);
                Ok(Some((CellNetId::from_bytes(src), n)))
            }
            FT_PING if n >= 9 => {
                let mut ping_data = [0u8; 8];
                ping_data.copy_from_slice(&buf[1..9]);
                self.send_pong(net, &ping_data);
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Check TCP connection state (0x03 = Established).
    pub fn check_connected(&mut self, net: &mut NetRef) -> bool {
        let cap = match self.tcp_cap {
            Some(c) => c,
            None => return false,
        };
        let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];
        matches!(
            net.call::<NetRequest, NetResponse>(
                &NetRequest::SocketState { cap_id: cap },
                &mut resp
            ),
            Ok(NetResponse::State(0x03))
        )
    }

    pub fn disconnect(&mut self, net: &mut NetRef) {
        if let Some(cap) = self.tcp_cap.take() {
            let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];
            let _ = net
                .call::<NetRequest, NetResponse>(&NetRequest::TcpClose { cap_id: cap }, &mut resp);
        }
    }

    fn send_pong(&self, net: &mut NetRef, data: &[u8; 8]) {
        if let Some(cap) = self.tcp_cap {
            let mut frame = [0u8; 13];
            frame[0..4].copy_from_slice(&9u32.to_be_bytes()); // len = 1 + 8
            frame[4] = FT_PONG;
            frame[5..13].copy_from_slice(data);
            let mut resp = [0u8; api::ipc::IPC_BUF_SIZE];
            let _ = net.call::<NetRequest, NetResponse>(
                &NetRequest::TcpSend {
                    cap_id: cap,
                    data: &frame,
                },
                &mut resp,
            );
        }
    }
}

// ── TCP helpers ───────────────────────────────────────────────────────────────

/// Read exactly `n` bytes from TCP into `dest[..n]`, looping on short reads.
///
/// Each TcpRecv call may return fewer bytes than requested (TCP segmentation).
/// This function retries until all `n` bytes are received or an error occurs.
fn tcp_read_exact(net: &mut NetRef, cap_id: u32, dest: &mut [u8], n: usize) -> ViResult<()> {
    let mut received = 0;
    while received < n {
        sys_heartbeat(HEARTBEAT_MS);
        let want = (n - received) as u32;
        let mut tmp = [0u8; api::ipc::IPC_BUF_SIZE];
        let chunk = match net
            .call::<NetRequest, NetResponse>(
                &NetRequest::TcpRecv {
                    cap_id,
                    buf_len: want,
                },
                &mut tmp,
            )
            .map_err(|_| ViError::IO)?
        {
            NetResponse::Data(d) => d,
            _ => return Err(ViError::IO),
        };
        if chunk.is_empty() {
            return Err(ViError::IO);
        } // connection closed
        let len = chunk.len().min(n - received);
        dest[received..received + len].copy_from_slice(&chunk[..len]);
        received += len;
    }
    Ok(())
}

/// Read one length-prefixed relay frame into `out`.
/// Returns the number of bytes written (= frame body length, including type byte).
/// `out[0]` = frame type; `out[1..]` = frame data.
fn recv_frame_into(
    net: &mut NetRef,
    cap_id: u32,
    out: &mut [u8; api::ipc::IPC_BUF_SIZE],
) -> ViResult<usize> {
    // Read 4-byte big-endian length prefix.
    let mut hdr = [0u8; 4];
    tcp_read_exact(net, cap_id, &mut hdr, 4)?;
    let frame_len = u32::from_be_bytes(hdr) as usize;
    if frame_len == 0 || frame_len > MAX_FRAME {
        return Err(ViError::IO);
    }

    // Read frame body (type + data) directly into `out`.
    tcp_read_exact(net, cap_id, &mut out[..frame_len], frame_len)?;
    Ok(frame_len)
}
