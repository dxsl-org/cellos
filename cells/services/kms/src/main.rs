#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate ostd;

use api::syscall::service;
use service_kms::{boot_probe_store, KmsService, ServiceRegistrySnapshot};
use types::kms::KMS_MESSAGE_LEN;

api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![
    Send,
    SendGather,
    Recv,
    RecvTimeout,
    Heartbeat,
    Log,
    LookupServiceBound,
    VfsMutate
];

ostd::cell_main!(cell_main);

fn cell_main() {
    ostd::io::println("[kms] fail-closed node identity service starting");
    #[cfg(all(feature = "ssh-dev-custody", not(feature = "ssh-dev-persist")))]
    ostd::io::println("[kms] DEV_POSTURE ssh-dev-custody: volatile single-identity ssh leaf");
    #[cfg(feature = "ssh-dev-persist")]
    ostd::io::println(
        "[kms] DEV_POSTURE ssh-dev-persist: wrapped identity persisted to /srv, dev root key co-resident, no rollback or crash-durability claim",
    );
    boot_probe_store();
    let mut kms = Kms(KmsService::new());
    #[cfg(feature = "development-silo-provider")]
    if kms.0.development_silo_boot_probe() {
        ostd::io::println("[kms] DEV_REFERENCE Silo TLS signature self-verified");
    } else {
        ostd::io::println("[kms] development Silo unavailable; relay remains fail-closed");
    }
    ostd::service::serve(ostd::service::ServeConfig::with_heartbeat(), &mut kms);
}

struct Kms(KmsService);

impl ostd::service::Handler for Kms {
    fn on_request(&mut self, request: ostd::service::Request<'_>) {
        let registry = ServiceRegistrySnapshot {
            net_broker_tid: ostd::service::provider_tid(service::NET_BROKER),
            supervisor_tid: ostd::service::provider_tid(service::SUPERVISOR),
            net_tid: ostd::service::provider_tid(service::NET),
        };
        if let Some(response) = self.0.handle(
            &request.payload()[..KMS_MESSAGE_LEN],
            request.sender(),
            Some(*request.caller()),
            registry,
        ) {
            let _ = request.reply(&response.to_bytes());
        }
    }
}
