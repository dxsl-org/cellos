#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use driver_dwc2_usb::lan_ipc;
use ostd::io::println;
use ostd::syscall::{sys_heartbeat, sys_recv_timeout, sys_send, sys_try_send, SyscallResult};

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Send, TrySend, RecvTimeout, Heartbeat, Log];

const POLL_TICKS: u64 = 1;
const HEARTBEAT_TICKS: u64 = 5_000;

ostd::cell_main!(cell_main);

/// LAN9514 client front-end. The DWC2 host remains the kernel NIC endpoint
/// because that registration requires USB authority; this worker serializes the
/// untrusted client envelope and has no device capability of its own.
fn cell_main() {
    let mut buf = [0u8; api::ipc::IPC_BUF_SIZE];
    let host_tid = loop {
        match sys_recv_timeout(0, &mut buf, POLL_TICKS) {
            SyscallResult::Ok(sender) if sender > 0 && lan_ipc::is_attach(&buf[..1]) => {
                break sender
            }
            _ => {}
        }
    };
    println("[lan9514] isolated NIC front-end ready");

    loop {
        sys_heartbeat(HEARTBEAT_TICKS);
        match sys_recv_timeout(0, &mut buf, POLL_TICKS) {
            SyscallResult::Ok(sender) if sender == host_tid => {
                if let Some((client_tid, payload)) = lan_ipc::decode_response(&buf) {
                    let _ = sys_try_send(client_tid, payload);
                } else if lan_ipc::decode_request(&buf).is_some() {
                    // The host is the registered NIC endpoint. It forwards each
                    // client request through this capability-free front-end;
                    // send the bounded envelope back for USB transport dispatch.
                    let _ = sys_send(host_tid, &buf);
                }
            }
            _ => {}
        }
    }
}
