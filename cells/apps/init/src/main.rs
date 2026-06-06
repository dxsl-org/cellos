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
    println("Init: Starting ViCell Orchestrator...");

    // 1. Spawn VFS Service (reads disk, serves /bin/*)
    println("Init: Spawning VFS Service...");
    match ostd::syscall::sys_spawn_from_path("/bin/vfs") {
        ostd::syscall::SyscallResult::Ok(_) => println("Init: VFS Service spawned."),
        _ => println("Init: WARN — VFS spawn failed; subsequent SpawnFromPath calls will fail."),
    }

    // Brief yield so VFS can initialise and register files before we ask for more.
    ostd::task::yield_now();
    ostd::task::yield_now();

    // 2. Spawn Config Service
    println("Init: Spawning Config Service...");
    match ostd::syscall::sys_spawn_from_path("/bin/config") {
        ostd::syscall::SyscallResult::Ok(_) => println("Init: Config Service spawned."),
        _ => println("Init: WARN — Config spawn failed."),
    }

    ostd::task::yield_now();

    // 3. Spawn Input Service (keyboard/mouse translation — non-fatal).
    println("Init: Spawning Input Service...");
    match ostd::syscall::sys_spawn_from_path("/bin/input") {
        ostd::syscall::SyscallResult::Ok(_) => println("Init: Input Service spawned."),
        _ => println("Init: INFO — Input service not found; UART input still works."),
    }

    ostd::task::yield_now();

    // 4. Spawn Network Service (TCP/IP stack — non-fatal, needs VirtIO NIC).
    println("Init: Spawning Network Service...");
    match ostd::syscall::sys_spawn_from_path("/bin/net") {
        ostd::syscall::SyscallResult::Ok(_) => println("Init: Network Service spawned."),
        _ => println("Init: INFO — Network service not found (no VirtIO NIC or binary missing)."),
    }

    ostd::task::yield_now();

    // 5. Spawn Compositor (software blending + GPU framebuffer — non-fatal).
    println("Init: Spawning Compositor...");
    match ostd::syscall::sys_spawn_from_path("/bin/compositor") {
        ostd::syscall::SyscallResult::Ok(_) => println("Init: Compositor spawned."),
        _ => println("Init: INFO — Compositor not found (no VirtIO GPU or binary missing)."),
    }

    ostd::task::yield_now();

    // 6. Spawn Shell (capture its tid so the supervisor below can restart it).
    println("Init: Spawning Shell...");
    let mut shell_tid = match ostd::syscall::sys_spawn_from_path("/bin/shell") {
        ostd::syscall::SyscallResult::Ok(tid) => {
            println("Init: Shell spawned successfully.");
            Some(tid)
        }
        _ => {
            println("Init: WARN — Shell spawn failed.");
            None
        }
    };

    ostd::task::yield_now();

    // 7. Spawn benchmark suite if present (non-fatal — only in CI disk images).
    // When /bin/bench is absent from the cell table, this silently skips.
    match ostd::syscall::sys_spawn_from_path("/bin/bench") {
        ostd::syscall::SyscallResult::Ok(_) => println("Init: Benchmark suite spawned."),
        _ => {} // bench not in cell table — normal dev boot, skip silently
    }

    // Supervisor: keep the shell alive ("let it crash, restart it"). sys_wait
    // blocks until the shell exits OR faults — the kernel wakes waiters on both
    // (reliability Phase 00) — then we respawn it so the user always has a prompt.
    //
    // A restart cap stops a crash-storm (a shell that dies immediately on every
    // spawn) from spin-respawning forever. A time-windowed intensity limit is the
    // proper long-running behavior but needs a ticks syscall in ostd — follow-up.
    // Multi-child supervision (vfs/net/...) needs wait-any (NotifyOnExit, a Law 1
    // syscall) — the next Phase 03 step; this MVP supervises the single most
    // user-visible service with existing primitives only.
    let mut restarts: u32 = 0;
    const MAX_RESTARTS: u32 = 100;
    loop {
        match shell_tid {
            Some(tid) => {
                // Block until the shell dies; the return value is its exit/fault reason.
                let _ = ostd::syscall::sys_wait(tid);
                if restarts >= MAX_RESTARTS {
                    println("Init: shell restart limit reached — supervision giving up.");
                    shell_tid = None;
                    continue;
                }
                restarts += 1;
                println("Init: shell died — restarting...");
                shell_tid = match ostd::syscall::sys_spawn_from_path("/bin/shell") {
                    ostd::syscall::SyscallResult::Ok(tid) => {
                        println("Init: shell restarted.");
                        Some(tid)
                    }
                    _ => {
                        println("Init: shell restart FAILED.");
                        None
                    }
                };
            }
            // No shell to supervise (initial spawn failed or supervision gave up).
            None => ostd::task::yield_now(),
        }
    }
}
