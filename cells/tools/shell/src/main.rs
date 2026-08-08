#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;
extern crate ostd;

// The shell carries no ambient launch or lifecycle authority. Exact reviewed
// launch edges are enforced in-kernel (`loader::launch_profile`) per
// `(caller= shell, route, target path)`.
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// Narrow syscall allowlist — kernel enforces this at dispatch (Phase 27).
// ForceExit is always-permitted at the allowlist layer; the kernel launch-edge
// split now denies shell lifecycle actions at dispatch.
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
    SpawnFromElf,
    Wait,
    GetTime,
    GetProcs,
    GetProcs2,
    SetTimer,
    WaitCompletion,
    OpenCap,
    ReadCap,
    CloseCap,
    GrantAlloc,
    GrantShare,
    GrantSlice,
    GrantFree,
    // Structured argv is staged in the shell's private state-stash slot and
    // transferred by the successful spawn syscall to that exact child.
    StateStash,
    // Read = stdin readline; Open/Close (+Read) = `cat` over the kernel FS;
    // ReadDir = the `ls` built-in. Omitting Read silently bricked the shell's
    // serial input once dispatch-level allowlist enforcement landed
    // (Phase 31b check_allowlist denies without logging).
    Read,
    Open,
    Close,
    ReadDir,
];

mod cmd_fs;
mod cmd_sys;
mod commands;
mod executor;
mod jobs;
mod parser;
mod shell_state;
mod snapshot_client;
mod text_engine;
mod text_tools;
mod top;

// Interactive REPL only: the shell_test harness drives `executor::capture_line`
// directly, so the line editor, alias table, history and config client have no
// caller there. Gating them keeps that build warning-free instead of carrying a
// blanket #[allow(dead_code)].
#[cfg(not(feature = "shell_test"))]
mod aliases;
#[cfg(not(feature = "shell_test"))]
mod async_utils;
#[cfg(not(feature = "shell_test"))]
mod config_client;
#[cfg(not(feature = "shell_test"))]
mod history;
#[cfg(not(feature = "shell_test"))]
mod shell;
// Hot-swap session transfer serialises the history + alias table, both of which
// only exist in the interactive build.
#[cfg(not(feature = "shell_test"))]
mod state_transfer;

#[cfg(feature = "shell_test")]
mod shell_test;

#[cfg(not(feature = "shell_test"))]
use shell::ViShell;

#[cfg(not(test))]
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
            if ostd::input::request_focus() {
                break;
            }
            ostd::task::yield_now();
        }
        let _ = ostd::syscall::sys_log("DEBUG: Shell focus acquired (or timed out)\n");
        let mut shell = ViShell::new();
        ostd::executor::block_on(shell.run());
    }
}
