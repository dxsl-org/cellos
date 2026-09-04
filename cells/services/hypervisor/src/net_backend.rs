//! Raw L2 frame IPC bridge: hypervisor cell ↔ Net Cell.
//!
//! `transmit`: guest TX frame → Net Cell L2Send → kernel NIC TX.
//! `try_receive`: Net Cell L2Recv → one inbound frame for the guest RX queue, or None.
//!
//! Every request uses bounded IPC and refreshes the supervised service generation.

extern crate alloc;

use alloc::boxed::Box;
use api::ipc::{NetRequest, NetResponse, IPC_BUF_SIZE};
use ostd::syscall::sys_lookup_service;

use crate::virtio_net::GUEST_MAC;
const BACKEND_TIMEOUT_TICKS: u64 = 200;

/// Supervised Net Cell connection state retained by one virtual device.
pub struct Connection {
    tid: usize,
    poisoned_tid: usize,
    recovery_pending: bool,
    force_unavailable_once: bool,
}

impl Connection {
    /// Initialize from the boot-time registry snapshot.
    pub fn new(tid: usize) -> Self {
        Self {
            tid,
            poisoned_tid: 0,
            recovery_pending: false,
            force_unavailable_once: false,
        }
    }

    #[cfg(feature = "hostile-backend-recovery")]
    pub fn force_unavailable_once(&mut self) {
        self.poisoned_tid = self.tid;
        self.tid = 0;
        self.recovery_pending = true;
        self.force_unavailable_once = true;
    }

    fn active_tid(&mut self) -> Option<usize> {
        let active_tid = sys_lookup_service(api::syscall::service::NET)?;
        if active_tid == self.poisoned_tid {
            return None;
        }
        if active_tid != self.tid {
            self.tid = active_tid;
            self.poisoned_tid = 0;
            self.recovery_pending = true;
        }
        Some(active_tid)
    }

    fn mark_unavailable(&mut self, active_tid: usize, poison: bool) {
        self.tid = 0;
        if poison {
            self.poisoned_tid = active_tid;
        }
        self.recovery_pending = true;
    }
}

/// Forward a raw Ethernet frame to the active Net Cell for NIC TX.
///
/// Returns `true` only after the active service generation acknowledges the
/// frame with `NetResponse::Ok`. Failed bounded IPC leaves recovery pending.
pub fn transmit(connection: &mut Connection, frame: &[u8]) -> bool {
    if connection.force_unavailable_once {
        connection.force_unavailable_once = false;
        return false;
    }
    let Some(active_tid) = connection.active_tid() else {
        return false;
    };
    let request = NetRequest::L2Send { data: frame };
    let mut send_buffer = [0u8; IPC_BUF_SIZE];
    let mut response_buffer = [0u8; IPC_BUF_SIZE];
    let result = ostd::ipc::service_call_typed_bounded(
        active_tid,
        &request,
        &mut send_buffer,
        &mut response_buffer,
        BACKEND_TIMEOUT_TICKS,
    );
    let ok = matches!(&result, Ok(NetResponse::Ok));
    if ok {
        if connection.recovery_pending {
            #[cfg(feature = "hostile-backend-recovery")]
            ostd::io::println(&alloc::format!(
                "[hv-backend-fault-host] recovered service=net new_tid={}",
                active_tid
            ));
            connection.recovery_pending = false;
        }
    } else {
        connection.mark_unavailable(
            active_tid,
            matches!(&result, Err(ostd::ipc::IpcError::Recv)),
        );
    }
    ok
}

/// Poll the active Net Cell for one inbound Ethernet frame.
///
/// A successful poll may refresh the service generation, but recovery remains
/// pending until a later TX receives `NetResponse::Ok`.
pub fn try_receive(connection: &mut Connection) -> Option<Box<[u8]>> {
    let active_tid = connection.active_tid()?;
    let request = NetRequest::L2Recv {
        guest_mac: GUEST_MAC,
    };
    let mut send_buffer = [0u8; IPC_BUF_SIZE];
    let mut response_buffer = [0u8; IPC_BUF_SIZE];
    match ostd::ipc::service_call_typed_bounded(
        active_tid,
        &request,
        &mut send_buffer,
        &mut response_buffer,
        BACKEND_TIMEOUT_TICKS,
    ) {
        Ok(NetResponse::Data(frame)) if !frame.is_empty() => Some(Box::from(frame)),
        Ok(NetResponse::Data(_)) | Ok(NetResponse::Ok) => None,
        error => {
            connection
                .mark_unavailable(active_tid, matches!(error, Err(ostd::ipc::IpcError::Recv)));
            None
        }
    }
}
