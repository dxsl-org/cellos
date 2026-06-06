#![no_std]
#![no_main]

extern crate ostd;

// Declares spawn capability; the kernel grants SpawnCap at spawn.
api::declare_manifest!(block_io = false, network = false, spawn = true);

use ostd::io::println;

/// Kernel spawns init from its embedded ELF.  Init's job is to bring up the
/// rest of the system by loading cell ELFs from the bootstrap disk table.
///
/// Boot sequence:
///   1. Spawn VFS service — serves `/bin/*` once running.
///   2. Spawn Config service — configuration KV store.
///   3. Spawn Shell — interactive REPL.
#[no_mangle]
pub extern "C" fn main() {
    use ostd::syscall::{
        sys_lookup_service, sys_notify_on_exit, sys_recv, sys_register_service,
        sys_spawn_from_path, SyscallResult,
    };
    use api::syscall::service;
    println("Init: Starting ViCell Orchestrator...");

    // Supervised services in bring-up order — VFS first (it serves /bin/*).
    // tids[i] is the current live tid of paths[i] (None when down).
    const NSVC: usize = 6;
    let paths: [&str; NSVC] = [
        "/bin/vfs",
        "/bin/config",
        "/bin/input",
        "/bin/net",
        "/bin/compositor",
        "/bin/shell",
    ];
    let mut tids: [Option<usize>; NSVC] = [None; NSVC];

    // Well-known service ID per path (None = not a looked-up service, e.g. shell).
    // The supervisor registers each service's CURRENT tid here so clients resolve it
    // via sys_lookup_service and reconnect transparently across a respawn.
    let svc_ids: [Option<u16>; NSVC] = [
        Some(service::VFS),
        Some(service::CONFIG),
        Some(service::INPUT),
        Some(service::NET),
        Some(service::COMPOSITOR),
        None, // shell is not a registered service
    ];

    for i in 0..NSVC {
        match sys_spawn_from_path(paths[i]) {
            SyscallResult::Ok(tid) => {
                tids[i] = Some(tid);
                if let Some(sid) = svc_ids[i] {
                    let _ = sys_register_service(sid, tid);
                }
            }
            // Non-fatal: input/net/compositor may be absent (no device/binary).
            _ => {}
        }
        // Let each service initialise before the next; VFS gets an extra beat to
        // register /bin/* before the others try to load from it.
        ostd::task::yield_now();
        if i == 0 {
            ostd::task::yield_now();
        }
    }
    println("Init: services spawned.");

    // Service-registry round-trip self-check (observable boot proof): every registered
    // service must resolve via sys_lookup_service to the tid we recorded at spawn.
    let mut ok = true;
    for i in 0..NSVC {
        if let (Some(sid), Some(tid)) = (svc_ids[i], tids[i]) {
            if sys_lookup_service(sid) != Some(tid) {
                ok = false;
            }
        }
    }
    if ok {
        println("Init: service registry verified.");
    } else {
        println("Init: WARN service registry mismatch.");
    }

    // Optional benchmark suite (CI disk images only) — not supervised.
    let _ = sys_spawn_from_path("/bin/bench");

    // Register a death notification for every live service. A single recv loop
    // below now supervises ALL of them (wait-any): when any service exits or
    // faults, sys_recv returns its tid and we respawn it. This is the full
    // supervisor tree built on NotifyOnExit (Law 1 syscall 204).
    for t in tids.iter().flatten() {
        let _ = sys_notify_on_exit(*t);
    }
    println("Init: supervising services (auto-restart on crash)...");

    // Death notifications carry the dead tid as the recv "sender"; no payload, so a
    // tiny throwaway buffer suffices.
    let mut buf = [0u8; 16];
    let mut restarts: u32 = 0;
    const MAX_RESTARTS: u32 = 200;
    loop {
        let dead = match sys_recv(0, &mut buf) {
            SyscallResult::Ok(d) => d,
            _ => {
                ostd::task::yield_now();
                continue;
            }
        };
        // Which supervised service died? (Ignore notifications for unknown tids.)
        let mut which = None;
        for (i, t) in tids.iter().enumerate() {
            if *t == Some(dead) {
                which = Some(i);
                break;
            }
        }
        let i = match which {
            Some(i) => i,
            None => continue,
        };
        if restarts >= MAX_RESTARTS {
            println("Init: restart limit reached — backing off supervision.");
            tids[i] = None;
            continue;
        }
        restarts += 1;
        println("Init: service died — restarting...");
        match sys_spawn_from_path(paths[i]) {
            SyscallResult::Ok(newt) => {
                tids[i] = Some(newt);
                let _ = sys_notify_on_exit(newt); // re-arm for the new instance
                if let Some(sid) = svc_ids[i] {
                    // Re-point the service registry at the new instance so clients that
                    // resolve via sys_lookup_service reconnect to the restarted service.
                    let _ = sys_register_service(sid, newt);
                }
                println("Init: service restarted.");
            }
            _ => {
                tids[i] = None;
                println("Init: service restart FAILED.");
            }
        }
    }
}
