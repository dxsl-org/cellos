#![no_std]
#![no_main]
#![forbid(unsafe_code)]

// ── Option rules ─────────────────────────────────────────────────────────────
// The options are orthogonal, but two combinations contradict each other and a
// third has a prerequisite. These are compile errors rather than warnings
// because a silent mismatch produces an image that boots and then fails at the
// thing it was built for.
#[cfg(all(feature = "tier3-autostart", not(feature = "tier3")))]
compile_error!(
    "tier3-autostart requires tier3: an image without the guest cell has nothing to preload"
);
#[cfg(all(feature = "hostile-backend-recovery", not(feature = "tier3")))]
compile_error!(
    "hostile-backend-recovery requires tier3: the recovery handler drives the hypervisor cell"
);

extern crate alloc;
extern crate ostd;

api::declare_manifest!(block_io = false, network = false, spawn = true);
api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    IpcSubmit,
    IpcTake,
    IpcWait,
    IpcCancel,
    IpcCurrent,
    IpcReply,
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
    // Demand activation uses the existing grant-backed VFS spawn route and
    // kernel-attested root lifetime endpoints, never caller-provided identity.
    GrantShare,
    GrantFree,
    SpawnFromElf,
    ForceExit,
    NotifyOnExit,
    RegisterService,
    // The boot-order grant-pair launcher publishes each generation's command line
    // (`StateStash`) and waits for that generation's terminal fault by sampling
    // the process table (`GetProcs`).
    StateStash,
    GetProcs,
];

mod boot;
mod activation;
mod activation_state;
mod configuration;
mod service_table;
mod supervisor;

use ostd::io::println;
use alloc::vec::Vec;
use cellos_boot_config::{ordered_indices, CellConfig};

ostd::cell_main!(extern "C" cell_main);

fn cell_main() {
    println("Init: Starting Cellos Orchestrator...");
    boot::start_block_drivers();
    let mut services = Vec::with_capacity(cellos_boot_config::MAX_CELLS + 1);
    services.push(service_table::Service::bootstrap_vfs());
    match service_table::spawn(&mut services[0]) {
        Some(_) => {}
        None => {
            println("Init: required VFS bootstrap failed — boot stopped.");
            return;
        }
    };
    let (service_config, autoload_config) = match configuration::load() {
        Ok(config) => config,
        Err(error) => {
            println(&alloc::format!("Init: configuration failure — {error}"));
            supervisor::run(&mut services);
        }
    };
    let service_order = match ordered_indices(&service_config, &CellConfig::default()) {
        Ok(order) => order,
        Err(error) => {
            println(&alloc::format!("Init: service ordering failure — {error}"));
            supervisor::run(&mut services);
        }
    };
    let autoload_order = match ordered_indices(&autoload_config, &service_config) {
        Ok(order) => order,
        Err(error) => {
            println(&alloc::format!("Init: autoload ordering failure — {error}"));
            supervisor::run(&mut services);
        }
    };
    let autoload_offset = 1 + service_config.cells.len();
    services.extend(service_config.cells.into_iter().map(service_table::Service::new));
    services.extend(autoload_config.cells.into_iter().map(service_table::Service::new));
    if !launch_order(&mut services, &service_order, 1) {
        println("Init: required service failed — autoload and OS ready blocked.");
        supervisor::run(&mut services);
    }

    let init_tid = {
        let mut procs = [api::syscall::ProcessInfo::default(); 16];
        let mut tid = 2;
        if let Ok(count) = ostd::syscall::sys_get_procs(&mut procs) {
            for proc in procs.iter().take(count) {
                if &proc.name[..4] == b"init" && (proc.name[4] == 0 || proc.name[4] == b' ') {
                    tid = proc.id;
                    break;
                }
            }
        }
        tid
    };
    if !matches!(
        ostd::syscall::sys_register_service(api::syscall::service::OCEL_ACTIVATOR, init_tid),
        ostd::syscall::SyscallResult::Ok(0)
    ) {
        println("Init: Ocel activator registration failed — autoload blocked.");
        supervisor::run(&mut services);
    }

    // Test fixtures retain their reviewed boot lanes. Production optional cells,
    // including shell and hypervisor autostart, come exclusively from the plan.
    boot::spawn_test_fixtures();
    if !required_services_available(&services[..autoload_offset]) {
        println("Init: required service lost before autoload — OS ready blocked.");
        supervisor::run(&mut services);
    }
    println("Init: services spawned.");
    println("Init: service registry verified (init registration is publication, not application readiness).");
    println("Init: OS ready.");
    if !launch_order(&mut services, &autoload_order, autoload_offset)
        || !required_services_available(&services)
    {
        println("Init: required cell unavailable — remaining autoload blocked.");
        supervisor::run(&mut services);
    }
    println("Init: supervising services (auto-restart on crash)...");
    supervisor::run(&mut services)
}

fn required_services_available(services: &[service_table::Service]) -> bool {
    services.iter().all(|service| {
        !service.spec.enabled || !service.spec.required || service.available()
    })
}

fn launch_order(services: &mut [service_table::Service], order: &[usize], offset: usize) -> bool {
    for &entry in order {
        if services.iter().any(|service| {
            service.spec.enabled && service.spec.required && service.tid.is_some() && !service.available()
        }) {
            println("Init: required service lost during launch — remaining cells blocked.");
            return false;
        }
        let index = offset + entry;
        if !service_table::dependencies_available(services, index) {
            println(&alloc::format!("Init: {} blocked — dependency unavailable.", services[index].spec.name));
            if services[index].spec.required { return false; }
            continue;
        }
        if service_table::spawn(&mut services[index]).is_none() {
            println(&alloc::format!("Init: {} spawn/registration failed.", services[index].spec.name));
            if services[index].spec.required { return false; }
        } else {
            println(&alloc::format!("Init: {} launched.", services[index].spec.name));
        }
        ostd::task::yield_now();
    }
    true
}
