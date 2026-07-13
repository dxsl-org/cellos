#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

// Declares spawn capability; the kernel grants SpawnCap at spawn.
// gpio/uart: held for DELEGATION only (P2 monotonic downgrade intersects a
// child's manifest with the spawner's caps) — the shell never opens MMIO
// itself, but interactively-spawned peripheral demos (periph-demo, robot-demo,
// sensor-demo, …) would otherwise lose their gpio/uart caps and fail with
// PermissionDenied. The interactive operator at the shell IS the robot
// operator, so shell-level peripheral delegation matches the trust model.
api::declare_manifest!(block_io = false, network = false, spawn = true, gpio = true, uart = true);

// Narrow syscall allowlist — kernel enforces this at dispatch (Phase 27).
// ForceExit is always-permitted (SpawnCap-gated at dispatch).
api::declare_syscalls![
    Send, Recv, TryRecv, RecvTimeout, Reply, Log, Heartbeat, LookupService,
    SpawnFromPath, SpawnFromMem, SpawnPinned, Wait, GetTime, GetProcs, SetTimer,
    HotSwap, StateStash, StateRestore,
    OpenCap, ReadCap, CloseCap,
    GrantAlloc, GrantShare, GrantSlice, GrantFree,
    // Read = stdin readline; Open/Close (+Read) = `cat` over the kernel FS;
    // ReadDir = the `ls` built-in (reads the kernel FS directly, not VFS IPC);
    // Snapshot = the `snapshot` built-in. Omitting Read silently bricked the
    // shell's serial input once dispatch-level allowlist enforcement landed
    // (Phase 31b check_allowlist denies without logging).
    Read, Open, Close, ReadDir, Snapshot,
];

mod aliases;
mod async_utils;
mod state_transfer;
mod cmd_fs;
mod cmd_sys;
mod commands;
mod config_client;
mod executor;
mod history;
mod jobs;
mod parser;
mod shell;

#[cfg(feature = "shell_test")]
mod shell_test;

use shell::ViShell;

#[no_mangle]
pub fn main() {
    #[cfg(feature = "shell_test")]
    shell_test::run();

    #[cfg(not(feature = "shell_test"))]
    {
        let _ = ostd::syscall::sys_log("DEBUG: Shell Started (Async Mode)\n");
        // Claim keyboard focus so VirtIO keyboard events are routed here via
        // the input service (fb_console keyboard relay).  Spin-wait for the
        // input service to come online — it races with shell at boot.
        // Don't spin indefinitely — attempt up to 50 times (~25 seconds),
        // then proceed without focus (UART fallback still works).
        for _ in 0..50 {
            if ostd::input::request_focus() { break; }
            ostd::task::yield_now();
        }
        let _ = ostd::syscall::sys_log("DEBUG: Shell focus acquired (or timed out)\n");
        let mut shell = ViShell::new();
        ostd::executor::block_on(shell.run());
    }
}
