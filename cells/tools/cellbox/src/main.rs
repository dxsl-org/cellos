#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;
extern crate ostd;

// `cellbox sh` carries no ambient launch or lifecycle authority. Exact reviewed
// launch edges are enforced in-kernel (`loader::launch_profile`) per
// `(caller= cellbox, route, target path)`.
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
// split now denies cellbox lifecycle actions at dispatch.
// This list is the retired `/bin/shell` ceiling plus `GetRandom` (Phase 09 D79-9 as
// revised): the one cellbox ELF is also installed at `/bin/cellbox-ssh`, whose ssh
// client needs kernel entropy for KEX. Entropy grants no authority; the network
// rights stay on that path's launch edge only. Nothing else may be added for one
// command.
api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    RecvTimeout,
    Reply,
    Log,
    MemInfo,
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
    Write,
    Open,
    Close,
    ReadDir,
    Chdir,
    Getcwd,
    VfsMutate,
    GetRandom,
];

/// Cellbox heap arena (D76-3 M2). `rg` compiles ignore-file globs with
/// `regex-automata`, whose construction peak (≈20 KiB per glob rule) does not fit
/// ostd's 1 MiB default next to the shell; see `rg::tests::HEAP_TARGET`.
pub const HEAP_ARENA_BYTES: usize = 6 * 1024 * 1024;
#[cfg(target_os = "none")]
ostd::declare_custom_heap!(HEAP_ARENA_BYTES);

mod cmd_cwd;
mod cmd_fs;
mod cmd_sys;
mod commands;
mod context_session;
mod edit;
mod executor;
mod jobs;
// Command disposition ledger: plain data, consumed only by host tests (S2-lite).
#[allow(dead_code)] // reason: production code never reads the ledger; host tests diff it against the registries
mod ledger;
mod native_rows;
mod parser;
mod rg;
mod oneshot;
mod ssh;
mod screen_reset;
mod shell_state;
mod text_engine;
mod text_tools;
mod top;
mod typed;

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
pub mod history;
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

/// Entry: the launch context is read once and classified (`oneshot::classify`, S10-0):
/// the ssh client child, the Cellos shell session (`cellbox sh`) or one catalog command
/// (`/bin/cellbox <command>`).
#[cfg(not(test))]
#[no_mangle]
pub fn main() {
    #[cfg(target_os = "none")]
    init_custom_heap();
    #[cfg(feature = "shell_test")]
    shell_test::run();

    #[cfg(not(feature = "shell_test"))]
    match ostd::child_context::ChildContext::read_own() {
        Ok(context) => {
            let argv: alloc::vec::Vec<alloc::string::String> =
                context.argv().map(alloc::string::String::from).collect();
            match oneshot::classify(context.frontend_grant().is_some(), &argv) {
                // The ssh client Cell, which the shell itself starts from
                // `/bin/cellbox-ssh` with argv `["ssh", ...]`.
                oneshot::Launch::Ssh => ostd::syscall::sys_exit(ssh::run_child(&context) as u32 as usize),
                oneshot::Launch::Shell => run_frontend_shell(&context),
                launch => {
                    let status = oneshot::run(&context, launch, &argv);
                    ostd::syscall::sys_exit(status as u32 as usize)
                }
            }
        }
        Err(status) => match context_session::classify_read_failure(status) {
            context_session::Launch::Legacy => run_legacy_shell(),
            context_session::Launch::Refused(status) => {
                let _ = ostd::syscall::sys_log(&alloc::format!(
                    "shell: launch context refused: {:?}\n", status
                ));
                ostd::syscall::sys_exit(125);
            }
        },
    }
}

/// A context carrying an input frontend (a GUI Terminal or another `FrontendHost`
/// launched this shell with `sh`) makes the shell create and control its own session for
/// that frontend and run the serial controller path, relaying output to the holder's
/// writers: it never calls `create_serial`, never touches the physical UART and never
/// reads legacy fd 0. A context that cannot be bound is refused, not downgraded to the
/// serial path.
#[cfg(all(not(test), not(feature = "shell_test")))]
fn run_frontend_shell(context: &ostd::child_context::ChildContext) {
    let _ = ostd::syscall::sys_log("shell: starting\n");
    match context_session::open(context) {
        Ok(launch) => {
            executor::start_frontend_session(launch.session, launch.output, launch.merged);
            if let Err(status) = context_session::ensure_default_cwd() {
                let _ = ostd::syscall::sys_log(&alloc::format!(
                    "shell: default cwd unavailable: {:?}\n", status
                ));
            }
            if let Err(status) = run_session_loop() {
                let _ = ostd::syscall::sys_log(&alloc::format!(
                    "shell: frontend session failed: {:?}\n", status
                ));
                ostd::syscall::sys_exit(125);
            }
        }
        Err(status) => {
            let _ = ostd::syscall::sys_log(&alloc::format!(
                "shell: launch context refused: {:?}\n", status
            ));
            ostd::syscall::sys_exit(125);
        }
    }
}

/// Without any committed context (the init launch): serial session unless the
/// compositor is present.
#[cfg(all(not(test), not(feature = "shell_test")))]
fn run_legacy_shell() {
    let _ = ostd::syscall::sys_log("shell: starting\n");
    // The graphical frontend has not been admitted to SessionV1. Preserve
    // its existing path; a serial session never falls back to public fd 0.
    if ostd::syscall::sys_lookup_service(api::syscall::service::COMPOSITOR).is_some() {
        let mut shell = ViShell::new();
        ostd::executor::block_on(shell.run());
    } else if let Err(status) = run_serial_session() {
        let _ = ostd::syscall::sys_log(&alloc::format!(
            "shell: authenticated serial session failed: {:?}\n", status
        ));
        ostd::syscall::sys_exit(125);
    }
}

/// Serial is an actual owner-backed terminal consumer, not a public-stdin
/// adapter. The existing dispatcher retains aliases, configuration and grammar.
#[cfg(all(not(test), not(feature = "shell_test")))]
fn run_serial_session() -> Result<(), api::cell_session::Status> {
    executor::start_serial_session()?;
    run_session_loop()
}

/// The line editor and dispatcher over the installed terminal session (serial or
/// frontend). Returns when the session reports end of input.
#[cfg(all(not(test), not(feature = "shell_test")))]
fn run_session_loop() -> Result<(), api::cell_session::Status> {
    let mut shell = ViShell::new();
    let mut lines = alloc::vec::Vec::<alloc::string::String>::new();
    executor::terminal_stdout("\n=== Cellos shell ready — type 'help' for commands ===\n".as_bytes())?;
    loop {
        executor::terminal_stdout(b"Cellos > ")?;
        let Some(mut line) = executor::terminal_read_line(&mut lines)? else {
            return executor::close_session();
        };
        let skip = {
            let trimmed = line.trim();
            trimmed.is_empty() || trimmed.starts_with('#')
        };
        if skip {
            ostd::session::zeroize_string(&mut line);
            continue;
        }
        crate::history::record_history(line.trim());
        let capture = shell_state::CaptureGuard::new();
        let dispatched = ostd::executor::block_on(shell.dispatch(&line));
        let mut output = capture.finish();
        let written = executor::terminal_stdout(&output);
        // The typed line and captured output may carry secrets (`echo $SECRET`): wipe both. History
        // keeps its own copy only because the user asked for command history.
        ostd::session::zeroize_string(&mut line);
        ostd::session::zeroize_vec(&mut output);
        written?;
        if dispatched.is_err() {
            executor::terminal_stderr(b"shell: command failed\n")?;
        }
        if let Some(code) = executor::take_exit_request() {
            executor::close_session()?;
            ostd::syscall::sys_exit(code as usize);
        }
    }
}
