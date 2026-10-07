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

    let init_tid = {
        let mut procs = [api::syscall::ProcessInfo::default(); 16];
        let mut tid = 2;
        if let Ok(count) = ostd::syscall::sys_get_procs(&mut procs) {
            for proc in &procs[..count] {
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
        println("Init: Ocel activator registration failed — shell not started.");
        return;
    }

    let hypervisor_tid = boot::spawn_optional_services();

    // The shell is the last table entry in both profiles, and both boot to it:
    // the full profile's desktop prompt, and the Tier-3 profile's prompt where
    // the guest is started on demand (`hv`). Spawning it here rather than from
    // the table loop is what keeps it last, so the cells it may launch are up.
    let shell = services.last_mut().expect("service table is nonempty");
    if shell.path != "/bin/shell" {
        println("Init: invalid service table — shell must remain last.");
        return;
    }
    if service_table::spawn(shell).is_none() {
        println("Init: shell spawn failed.");
    }

    for tid in services.iter().filter_map(|service| service.tid) {
        let _ = sys_notify_on_exit(tid);
    }
    println("Init: supervising services (auto-restart on crash)...");
    supervisor::run(&mut services, hypervisor_tid)
}
