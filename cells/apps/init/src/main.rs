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

    // 6. Spawn Shell
    println("Init: Spawning Shell...");
    match ostd::syscall::sys_spawn_from_path("/bin/shell") {
        ostd::syscall::SyscallResult::Ok(_) => println("Init: Shell spawned successfully."),
        _ => println("Init: WARN — Shell spawn failed."),
    }

    ostd::task::yield_now();

    // 7. Spawn benchmark suite if present (non-fatal — only in CI disk images).
    // When /bin/bench is absent from the cell table, this silently skips.
    match ostd::syscall::sys_spawn_from_path("/bin/bench") {
        ostd::syscall::SyscallResult::Ok(_) => println("Init: Benchmark suite spawned."),
        _ => {} // bench not in cell table — normal dev boot, skip silently
    }

    // Keep init alive as the process supervisor.
    loop {
        ostd::task::yield_now();
    }
}
