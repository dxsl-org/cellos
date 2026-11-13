#![no_std]
#![no_main]
#![forbid(unsafe_code)]

use abi::syscall::service;
use driver_dwc2_usb::lan_ipc;
use ostd::io::println;
use ostd::service::sender_is_live_provider;
use ostd::syscall::{sys_heartbeat, sys_recv_timeout, SyscallResult};

abi::declare_manifest!(block_io = false, network = false, spawn = false);
abi::declare_syscalls![
    Send,
    SendGather,
    TrySend,
    RecvTimeout,
    Heartbeat,
    Log,
    LookupServiceBound
];

const POLL_TICKS: u64 = 1;
const HEARTBEAT_TICKS: u64 = 5_000;

ostd::cell_main!(cell_main);

/// LAN9514 client front-end. The DWC2 host remains the kernel NIC endpoint
/// because that registration requires USB authority; this worker serializes the
/// untrusted client envelope and has no device capability of its own.
fn cell_main() {
    let mut buf = [0u8; wire::ipc::IPC_BUF_SIZE];
    // Attach only from the registered NIC endpoint: the DWC2 host publishes
    // `service::NIC_DRIVER` before it spawns this worker, so the kernel's registry
    // names it. First-come attach let any cell that guessed this tid bind the
    // front-end and black-hole every NIC request the real host forwards.
    let host_tid = loop {
        match sys_recv_timeout(0, &mut buf, POLL_TICKS) {
            SyscallResult::Ok(sender)
                if sender > 0
                    && lan_ipc::is_attach(&buf[..1])
                    && sender_is_live_provider(service::NIC_DRIVER, sender) =>
            {
                break sender
            }
            _ => {}
        }
    };
    println("[lan9514] isolated NIC front-end ready");

    loop {
        sys_heartbeat(HEARTBEAT_TICKS);
        match sys_recv_timeout(0, &mut buf, POLL_TICKS) {
            SyscallResult::Ok(sender)
                if sender == host_tid
                // The host is the registered NIC endpoint. It forwards each
                // client request through this capability-free front-end; send
                // the bounded envelope back for USB transport dispatch. Replies
                // go from the host to the client directly: a reply relayed here
                // would arrive from this cell's tid, and a client waiting on the
                // host's tid never receives it.
                && lan_ipc::decode_request(&buf).is_some() =>
            {
                let _ = ostd::ipc::post(host_tid, &buf);
            }
            _ => {}
        }
    }
}
