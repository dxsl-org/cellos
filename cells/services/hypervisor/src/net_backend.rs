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
            // The poison exists because a reply that arrives after its deadline
            // could be read as the answer to the next request. Collect whatever
            // is still queued from that generation and the poison has done its
            // job - refusing the tid for good was a dead end: the registry keeps
            // naming it while the service stays up, so every later call
            // short-circuited and the guest lost its network in both directions
            // with nothing printed.
            let mut stale = [0u8; IPC_BUF_SIZE];
            for _ in 0..4 {
                match ostd::syscall::sys_recv_timeout(active_tid, &mut stale, 0) {
                    ostd::syscall::SyscallResult::Ok(sender) if sender == active_tid => {}
                    _ => break,
                }
            }
            self.poisoned_tid = 0;
            self.tid = active_tid;
            self.recovery_pending = true;
            return Some(active_tid);
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
    // Once-only, and deliberately not behind `l2-trace`: this is the first half of
    // the answer to "did the command I typed in the guest leave the guest at all",
    // which an operator asks on an interactive image (no trace features) as well
    // as on a diagnostic one. `[net-bridge] first guest L2Recv` / `first RX frame
    // into the guest` carry the other halves the same way.
    {
        static FIRST_FRAME: core::sync::atomic::AtomicBool =
            core::sync::atomic::AtomicBool::new(false);
        if !FIRST_FRAME.swap(true, core::sync::atomic::Ordering::Relaxed) {
            ostd::io::println(&alloc::format!(
                "[hv-net-tx] guest TX frame reached the backend len={}",
                frame.len()
            ));
        }
    }
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
    // One accepted kernel operation owns both admission and the exact reply.
    let result = ostd::ipc::service_call_typed_bounded(
        active_tid,
        &request,
        &mut send_buffer,
        &mut response_buffer,
        BACKEND_TIMEOUT_TICKS,
    );
    let ok = matches!(&result, Ok(NetResponse::Ok));
    if ok {
        // Unconditional and once, like the RX half (`[hv-l2] … L2Recv … result=ok`):
        // "did the guest's frame leave the guest" is asked on an interactive image
        // too, and with the witness behind `l2-trace` a quiet run could only infer
        // the TX direction from `[hv-virtio-host] net-tx-complete`.
        {
            static FIRST_ACCEPTED: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(false);
            if !FIRST_ACCEPTED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                ostd::io::println(&alloc::format!(
                    "[hv-net-tx] L2Send accepted len={} tid={}",
                    frame.len(),
                    active_tid
                ));
            }
        }
        if connection.recovery_pending {
            #[cfg(feature = "hostile-backend-recovery")]
            ostd::io::println(&alloc::format!(
                "[hv-backend-fault-host] recovered service=net new_tid={}",
                active_tid
            ));
            connection.recovery_pending = false;
        }
    } else {
        #[cfg(feature = "l2-trace")]
        trace_l2_outcome("L2Send", active_tid, &result);
        // The RX direction has had an unconditional once-only witness since the
        // board first showed the guest's poll reaching the service
        // (`[hv-net-rx] first L2Recv … result=…`); the TX direction had none, so
        // "the guest's ping never left the guest" could only be inferred from a
        // missing `[hv-virtio-host] net-tx-complete` while the reason — deadline,
        // foreign sender, decode — stayed hidden behind `l2-trace`. The reason is
        // precisely what decides the next fix, so it is named once here too.
        #[cfg(not(feature = "l2-trace"))]
        {
            static REPORTED: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(false);
            if !REPORTED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                ostd::io::println(&alloc::format!(
                    "[hv-net-tx] first L2Send failure: {} tid={}",
                    l2_status(&result),
                    active_tid
                ));
            }
        }
        connection.mark_unavailable(
            active_tid,
            matches!(&result, Err(ostd::ipc::IpcError::Recv | ostd::ipc::IpcError::PeerGone | ostd::ipc::IpcError::Indeterminate)),
        );
    }
    ok
}

/// Name an L2 exchange result for the console.
///
/// The *response* is half of the answer, and the board showed why: the line read
/// `first L2Send failure: ok tid=5`, because this classified the IPC call alone.
/// `Ok(NetResponse::Err(..))` is the Net Cell answering that it could not put the
/// frame on the wire — a different fault, with a different fix, from a deadline
/// with no answer at all.
fn l2_status(result: &Result<NetResponse<'_>, ostd::ipc::IpcError>) -> alloc::string::String {
    use ostd::ipc::IpcError;
    match result {
        Ok(NetResponse::Ok) => "ok".into(),
        Ok(NetResponse::Err(status)) => {
            alloc::format!("refused by the net service (driver status {status})")
        }
        Ok(other) => alloc::format!("unexpected response {other:?}"),
        Err(IpcError::Send) => "send deadline".into(),
        Err(IpcError::Recv) => "receive deadline/error".into(),
        Err(IpcError::WrongSender) => "wrong sender".into(),
        Err(IpcError::Encode) => "encode error".into(),
        Err(IpcError::Decode) => "decode error".into(),
        Err(IpcError::InvalidOperation) => "invalid operation".into(),
        Err(IpcError::BufferTooSmall) => "reply buffer too small".into(),
        Err(IpcError::Busy) => "operation capacity busy".into(),
        Err(IpcError::PeerGone) => "peer exited; effects may have occurred".into(),
        Err(IpcError::PreDispatchTimeout) => "deadline before dispatch".into(),
        Err(IpcError::Indeterminate) => "deadline after dispatch; effects unknown".into(),
        Err(IpcError::Cancelled) => "cancelled before dispatch".into(),
    }
}

/// Report one L2 exchange outcome (`l2-trace` images).
///
/// The once-only line cannot separate "never admitted" from "admitted once and
/// then never again", and the guest's path needs both directions placed from a
/// single board log. Failures are sampled densely (first four, then every 64th)
/// because they are what the image is built to place; successes are sampled
/// sparsely (first two, then every 512th) because a working path polls
/// continuously and the console is a 115200-baud UART.
#[cfg(feature = "l2-trace")]
fn trace_l2_outcome(
    direction: &str,
    tid: usize,
    result: &Result<NetResponse<'_>, ostd::ipc::IpcError>,
) {
    use core::sync::atomic::{AtomicU32, Ordering};
    static FAILURES: AtomicU32 = AtomicU32::new(0);
    static SUCCESSES: AtomicU32 = AtomicU32::new(0);
    let status = l2_status(result);
    let (n, report) = if status == "ok" {
        let n = SUCCESSES.fetch_add(1, Ordering::Relaxed) + 1;
        (n, n <= 2 || n % 512 == 0)
    } else {
        let n = FAILURES.fetch_add(1, Ordering::Relaxed) + 1;
        (n, n <= 4 || n % 64 == 0)
    };
    if report {
        ostd::io::println(&alloc::format!(
            "[hv-l2] n={} {} tid={} result={}",
            n,
            direction,
            tid,
            status
        ));
    }
}

/// Poll the active Net Cell for one inbound Ethernet frame.
///
/// A successful poll may refresh the service generation, but recovery remains
/// pending until a later TX receives `NetResponse::Ok`.
pub fn try_receive(connection: &mut Connection) -> Option<Box<[u8]>> {
    let active_tid = match connection.active_tid() {
        Some(tid) => tid,
        None => {
            static REPORTED: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(false);
            if !REPORTED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                ostd::io::println("[hv-net-rx] NET service lookup returned none");
            }
            return None;
        }
    };
    let request = NetRequest::L2Recv {
        guest_mac: GUEST_MAC,
    };
    let mut send_buffer = [0u8; IPC_BUF_SIZE];
    let mut response_buffer = [0u8; IPC_BUF_SIZE];
    let result = ostd::ipc::service_call_typed_bounded(
        active_tid,
        &request,
        &mut send_buffer,
        &mut response_buffer,
        BACKEND_TIMEOUT_TICKS,
    );
    #[cfg(feature = "l2-trace")]
    trace_l2_outcome("L2Recv", active_tid, &result);
    #[cfg(not(feature = "l2-trace"))]
    {
        static REPORTED: core::sync::atomic::AtomicBool =
            core::sync::atomic::AtomicBool::new(false);
        if !REPORTED.swap(true, core::sync::atomic::Ordering::Relaxed) {
            ostd::io::println(&alloc::format!(
                "[hv-net-rx] first L2Recv tid={} result={}",
                active_tid,
                l2_status(&result)
            ));
        }
    }
    match result {
        Ok(NetResponse::Data(frame)) if !frame.is_empty() => {
            #[cfg(feature = "l2-trace")]
            {
                static FIRST_DATA: core::sync::atomic::AtomicBool =
                    core::sync::atomic::AtomicBool::new(false);
                if !FIRST_DATA.swap(true, core::sync::atomic::Ordering::Relaxed) {
                    ostd::io::println(&alloc::format!(
                        "[hv-net-rx] L2Recv data len={} tid={}",
                        frame.len(),
                        active_tid
                    ));
                }
            }
            Some(Box::from(frame))
        }
        Ok(NetResponse::Data(_)) | Ok(NetResponse::Ok) => None,
        error => {
            connection
                .mark_unavailable(active_tid, matches!(error, Err(ostd::ipc::IpcError::Recv | ostd::ipc::IpcError::PeerGone | ostd::ipc::IpcError::Indeterminate)));
            None
        }
    }
}
