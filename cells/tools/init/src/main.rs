// Test builds only: a hosted `cargo test` gets std and libtest's entry, so init's
// boot barriers, block-driver wait, restart policy and safe set run as host unit tests.
//
// Init answers no service request (Phase 13): the Ocel activation endpoint is the
// `activator` Cell, the ADR-0029 machine pin is the `machine-pin` Cell, and the test
// fixtures are the `fixture-launcher` Cell. Init launches, registers and supervises;
// its receive loop handles only kernel death records.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
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
    SendGather,
    Recv,
    TryRecv,
    IpcSubmit,
    IpcTake,
    IpcWait,
    IpcCancel,
    IpcCurrent,
    IpcReply,
    RecvTimeout,
    Log,
    Heartbeat,
    LookupServiceBound,
    SpawnFromPath,
    Wait,
    GetTime,
    SetTimer,
    // Services are read from the VFS into a Grant and spawned from those bytes.
    GrantAlloc,
    GrantShare,
    GrantFree,
    SpawnFromElf,
    NotifyOnExit,
    RegisterService,
    // Every launch publishes the entry's structured command line (`StateStash`).
    // Liveness checks and the x86 bootstrap Platform lifetime barrier sample the
    // process table (`GetProcs`).
    StateStash,
    GetProcs,
];

mod boot;
mod configuration;
mod pin_gate;
mod service_table;
mod supervisor;

use alloc::vec::Vec;
use cellos_boot_config::{ordered_indices, CellConfig};
use ostd::io::println;

ostd::cell_main!(extern "C" cell_main);

fn cell_main() {
    println("Init: Starting Cellos Orchestrator...");
    if let Err(error) = boot::start_after_platform(start_services) {
        println(error.diagnostic());
    }
    // `start_services` returns only when boot cannot continue; every other path
    // ends in the supervisor loop, which never returns.
    boot_stop()
}

/// Leave with [`api::retirement::INIT_BOOT_STOP`], never a plain return: the kernel
/// records this death and halts, where any other init death resets the machine.
fn boot_stop() -> ! {
    println("Init: boot cannot continue — exiting with INIT_BOOT_STOP (the kernel halts).");
    ostd::syscall::sys_exit(api::retirement::INIT_BOOT_STOP)
}

/// Test hook (`init-death-fixture`, packaged only by
/// `scripts/build-x86_64-init-death-ci.sh`): fault once boot reports OS ready, so the
/// lane sees the kernel's `[init-death] … action=reset` record and a second boot.
/// Safe code (cells are F1 `forbid(unsafe_code)`): unbounded recursion writes each
/// 1 KiB frame down the user stack until it stores into the unmapped guard pages
/// below it, a user page fault. Never returns; it is unit-typed only so the
/// caller's later code stays reachable.
#[cfg(feature = "init-death-fixture")]
fn init_death_fixture_fault() {
    #[allow(unconditional_recursion)]
    #[inline(never)]
    fn exhaust_stack(depth: usize) -> usize {
        let mut frame = [0u8; 1024];
        // Opaque to the optimizer: the whole frame is written and stays live, and
        // the use after the call keeps the recursion out of tail position.
        core::hint::black_box(&mut frame);
        exhaust_stack(depth + 1) + usize::from(frame[depth % frame.len()])
    }
    println("Init: init-death fixture faulting after OS ready.");
    let _ = exhaust_stack(0);
    ostd::syscall::sys_exit(usize::MAX)
}

fn start_services() {
    // Found before any spawn, while init and the kernel-spawned Platform are the
    // only named tasks; the machine-pin Cell learns it from its command line.
    let init_tid = own_tid();
    // Supervised block drivers go after every configured entry: launch and
    // autoload orders index the table from 1. Each early `supervisor::run`
    // below appends them first so a driver death is still watched.
    let mut block_drivers = boot::start_block_drivers();
    let mut services = Vec::with_capacity(cellos_boot_config::MAX_CELLS + 3 + block_drivers.len());
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
            services.append(&mut block_drivers);
            supervisor::run(&mut services, None);
        }
    };
    let service_order = match ordered_indices(&service_config, &CellConfig::default()) {
        Ok(order) => order,
        Err(error) => {
            println(&alloc::format!("Init: service ordering failure — {error}"));
            services.append(&mut block_drivers);
            supervisor::run(&mut services, None);
        }
    };
    let autoload_order = match ordered_indices(&autoload_config, &service_config) {
        Ok(order) => order,
        Err(error) => {
            println(&alloc::format!("Init: autoload ordering failure — {error}"));
            services.append(&mut block_drivers);
            supervisor::run(&mut services, None);
        }
    };
    let autoload_offset = 1 + service_config.cells.len();
    services.extend(
        service_config
            .cells
            .into_iter()
            .map(service_table::Service::new),
    );
    services.extend(
        autoload_config
            .cells
            .into_iter()
            .map(service_table::Service::new),
    );
    let autoload_end = services.len();
    services.append(&mut block_drivers);
    // Phase 13 fixed rows after the VFS: the machine-pin Cell (x86) and the
    // Ocel activator, launched in that order before any configured service.
    let pin_index = cfg!(target_arch = "x86_64").then(|| {
        services.push(service_table::Service::machine_pin(init_tid));
        services.len() - 1
    });
    services.push(service_table::Service::activator());
    let activator_index = services.len() - 1;

    // ADR-0029 §3: the machine pin gates every configured service. The VFS and
    // the boot-critical block drivers above are already running and stay so.
    let decision = pin_gate::gate(&mut services, pin_index);
    if decision.safe_mode {
        let console = pin_gate::enter_safe_mode(
            &mut services,
            autoload_offset,
            autoload_end,
            &decision,
            pin_index,
        );
        supervisor::run(&mut services, console);
    }

    // Optional: images without Ocel engines do not package the activator.
    match service_table::spawn(&mut services[activator_index]) {
        Some(_) => println("Init: activator launched."),
        None => println("Init: activator unavailable — Ocel engines cannot be activated."),
    }

    if !launch_order(&mut services, &service_order, 1) {
        println("Init: required service failed — autoload and OS ready blocked.");
        supervisor::run(&mut services, None);
    }
    if !required_services_available(&services[..autoload_offset]) {
        println("Init: required service lost before autoload — OS ready blocked.");
        supervisor::run(&mut services, None);
    }
    println("Init: services spawned.");
    println("Init: service registry verified (init registration is publication, not application readiness).");
    println("Init: OS ready.");
    #[cfg(feature = "init-death-fixture")]
    init_death_fixture_fault();
    if !launch_order(&mut services, &autoload_order, autoload_offset)
        || !required_services_available(&services)
    {
        println("Init: required cell unavailable — remaining autoload blocked.");
        supervisor::run(&mut services, None);
    }
    println("Init: supervising services (auto-restart on crash)...");
    supervisor::run(&mut services, None)
}

/// Init's own tid, found by name in the process table; 2 when the table
/// cannot be read (the fallback init has always used here).
fn own_tid() -> usize {
    let mut procs = [api::syscall::ProcessInfo::default(); 16];
    if let Ok(count) = ostd::syscall::sys_get_procs(&mut procs) {
        for proc in procs.iter().take(count) {
            if &proc.name[..4] == b"init" && (proc.name[4] == 0 || proc.name[4] == b' ') {
                return proc.id;
            }
        }
    }
    2
}

fn required_services_available(services: &[service_table::Service]) -> bool {
    services
        .iter()
        .all(|service| !service.spec.enabled || !service.spec.required || service.available())
}

fn launch_order(services: &mut [service_table::Service], order: &[usize], offset: usize) -> bool {
    for &entry in order {
        if services.iter().any(|service| {
            service.spec.enabled
                && service.spec.required
                && service.tid.is_some()
                && !service.available()
        }) {
            println("Init: required service lost during launch — remaining cells blocked.");
            return false;
        }
        let index = offset + entry;
        if !service_table::dependencies_available(services, index) {
            println(&alloc::format!(
                "Init: {} blocked — dependency unavailable.",
                services[index].spec.name
            ));
            if services[index].spec.required {
                return false;
            }
            continue;
        }
        if service_table::spawn(&mut services[index]).is_none() {
            println(&alloc::format!(
                "Init: {} spawn/registration failed.",
                services[index].spec.name
            ));
            if services[index].spec.required {
                return false;
            }
        } else {
            println(&alloc::format!(
                "Init: {} launched.",
                services[index].spec.name
            ));
        }
        ostd::task::yield_now();
    }
    true
}
