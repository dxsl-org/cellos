#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate ostd;

use api::caller_identity::CallerIdentity;
use api::syscall::service;
use service_kms::{KmsService, ServiceRegistrySnapshot};
use types::kms::KMS_MESSAGE_LEN;

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![Send, Recv, Log, LookupService];

ostd::cell_main!(cell_main);

fn cell_main() {
    ostd::io::println("[kms] fail-closed node identity service starting");
    let mut service_state = KmsService::new();
    loop {
        let mut buffer = [0u8; api::ipc::IPC_BUF_SIZE];
        match ostd::syscall::sys_recv_attested(0, &mut buffer) {
            ostd::syscall::SyscallResult::Ok(sender) if sender > 0 => {
                let caller = CallerIdentity::from_recv_buf(&buffer);
                let registry = ServiceRegistrySnapshot {
                    net_broker_tid: ostd::syscall::sys_lookup_service(service::NET_BROKER),
                    supervisor_tid: ostd::syscall::sys_lookup_service(service::SUPERVISOR),
                };
                if let Some(response) =
                    service_state.handle(&buffer[..KMS_MESSAGE_LEN], sender, caller, registry)
                {
                    let _ = ostd::syscall::sys_send(sender, &response.to_bytes());
                }
            }
            _ => ostd::task::yield_now(),
        }
    }
}
