//! Raw L2 frame IPC bridge: hypervisor cell ↔ Net Cell.
//!
//! `transmit`: guest TX frame → Net Cell L2Send → kernel NIC TX.
//! `try_receive`: Net Cell L2Recv → one inbound frame for the guest RX queue, or None.
//!
//! Both functions block until the Net Cell acknowledges (it always replies synchronously).

extern crate alloc;
use alloc::boxed::Box;
use api::ipc::{self, NetRequest, NetResponse, IPC_BUF_SIZE};
use ostd::syscall::{sys_recv, sys_send, SyscallResult};

use crate::virtio_net::GUEST_MAC;

/// Forward a raw Ethernet frame to the Net Cell for NIC TX.
///
/// `net_tid` identifies the Net Cell and `frame` contains the complete L2 frame.
/// Returns `true` only when the request is encoded and the Net Cell acknowledges
/// it with `NetResponse::Ok`; returns `false` when the service is unavailable or
/// returns any other response.
pub fn transmit(net_tid: usize, frame: &[u8]) -> bool {
    if net_tid == 0 {
        return false;
    }
    let req = NetRequest::L2Send { data: frame };
    let mut buf = [0u8; IPC_BUF_SIZE];
    let Ok(msg) = ipc::encode(&req, &mut buf) else {
        return false;
    };
    if !matches!(sys_send(net_tid, msg), SyscallResult::Ok(_)) {
        return false;
    }
    // Bind the acknowledgement to the Net Cell so queued traffic cannot satisfy it.
    let mut rb = [0u8; IPC_BUF_SIZE];
    if !matches!(sys_recv(net_tid, &mut rb), SyscallResult::Ok(sender) if sender == net_tid) {
        return false;
    }
    matches!(
        ipc::decode::<NetResponse<'_>>(&rb),
        Ok(NetResponse::Ok)
    )
}

/// Poll the Net Cell for one inbound Ethernet frame destined for the guest MAC.
///
/// Returns `Some(frame)` if a frame was available, `None` otherwise.
/// Blocks for one round-trip to the Net Cell.  No-op when `net_tid == 0`.
pub fn try_receive(net_tid: usize) -> Option<Box<[u8]>> {
    if net_tid == 0 {
        return None;
    }
    let req = NetRequest::L2Recv {
        guest_mac: GUEST_MAC,
    };
    let mut buf = [0u8; IPC_BUF_SIZE];
    let Ok(msg) = ipc::encode(&req, &mut buf) else {
        return None;
    };
    if !matches!(sys_send(net_tid, msg), SyscallResult::Ok(_)) {
        return None;
    }
    let mut rb = [0u8; IPC_BUF_SIZE];
    if !matches!(sys_recv(net_tid, &mut rb), SyscallResult::Ok(sender) if sender == net_tid) {
        return None;
    }
    match ipc::decode::<NetResponse<'_>>(&rb) {
        Ok(NetResponse::Data(frame)) if !frame.is_empty() => Some(Box::from(frame)),
        _ => None,
    }
}
