#![no_std]
#![no_main]
#![forbid(unsafe_code)]

#[cfg(all(feature = "hostile-backend-recovery", not(feature = "hypervisor-min")))]
compile_error!("hostile-backend-recovery requires hypervisor-min");

extern crate ostd;

api::declare_manifest!(block_io = false, network = false, spawn = true);
api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    RecvTimeout,
    Reply,
    Log,
    Heartbeat,
    LookupService,
    SpawnFromPath,
    Wait,
    GetTime,
    SetTimer,
    GrantAlloc,
];

mod boot;
mod service_table;
mod supervisor;

use ostd::io::println;
use ostd::syscall::{sys_lookup_service, sys_notify_on_exit};

ostd::cell_main!(extern "C" cell_main);

fn cell_main() {
    println("Init: Starting Cellos Orchestrator...");
    let mut services = service_table::configured();

    boot::start_block_drivers();
    for service in &mut services {
        if service.path == "/bin/shell" {
            continue;
        }
        boot::prepare_service(service.path);
        let tid = match service_table::spawn(service) {
            Some(tid) => tid,
            None => {
                #[cfg(feature = "development-silo-provider")]
                if matches!(
                    service.registration,
                    service_table::Registration::SelfReady(api::syscall::service::SILO)
                ) {
                    println("Init: Silo spawn failed — KMS not started.");
                    return;
                }
                println("Init: cell not found — skipping:");
                println(service.path);
                ostd::task::yield_now();
                continue;
            }
        };
        #[cfg(not(feature = "development-silo-provider"))]
        let _ = tid;
        #[cfg(feature = "development-silo-provider")]
        if matches!(
            service.registration,
            service_table::Registration::SelfReady(api::syscall::service::SILO)
        ) && !service_table::wait_for_exact_registration(api::syscall::service::SILO, tid)
        {
            println("Init: Silo readiness registration failed — KMS not started.");
            return;
        }
        ostd::task::yield_now();
        if service.path == "/bin/vfs" {
            ostd::task::yield_now();
        }
    }
    println("Init: services spawned.");

    if services
        .iter()
        .all(|service| match (service.service_id(), service.tid) {
            (Some(service_id), Some(tid)) => sys_lookup_service(service_id) == Some(tid),
            _ => true,
        })
    {
        println("Init: service registry verified.");
    } else {
        println("Init: WARN service registry mismatch.");
    }

    let hypervisor_tid = boot::spawn_optional_services();

    #[cfg(not(feature = "hypervisor-min"))]
    {
        let shell = services.last_mut().expect("service table is nonempty");
        if shell.path != "/bin/shell" {
            println("Init: invalid service table — shell must remain last.");
            return;
        }
        if service_table::spawn(shell).is_none() {
            println("Init: shell spawn failed.");
        }
    }

    for tid in services.iter().filter_map(|service| service.tid) {
        let _ = sys_notify_on_exit(tid);
    }
    println("Init: supervising services (auto-restart on crash)...");
    supervisor::run(&mut services, hypervisor_tid)
}
