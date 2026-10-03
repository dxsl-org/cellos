#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

use api::task::TaskPriority;
use ostd::syscall::{SyscallError, SyscallResult};

api::declare_syscalls![Log, SpawnPinned, StateStash, StateRestore, Exit, Yield];
api::declare_manifest!(block_io = false, network = false, spawn = true);

ostd::cell_main!(cell_main);

/// Spawn bound for the sweep.
///
/// The refusal must come from the kernel, never from this loop: with the
/// production ceilings (`MAX_CELLS` 64) it arrives at ~64, while the
/// `cell-scale-experiment` kernel raises them so memory is what binds. Hitting
/// this bound prints OOM_NOT_REACHED, so a truncated sweep can never be read as
/// a capacity result.
const SPAWN_BOUND: usize = 2100;

/// Report the running count this often: the count *is* the measurement, and a run
/// cut short by its boot window must still leave its progress in the log.
const PROGRESS_EVERY: usize = 32;

fn cell_main() {
    match ostd::syscall::sys_mem_info() {
        Err(_) => ostd::io::println("[a2a3-probe] MEMINFO_DENIED"),
        Ok(_) => {
            ostd::io::println("[a2a3-probe] MEMINFO_UNEXPECTEDLY_ALLOWED");
            ostd::syscall::sys_exit(2);
        }
    }

    // Heavy cells first, then the sweep. The D5 gate measures the light ceiling
    // *with* M heavy cells resident (`docs/roadmap/beam-parity-backend-roadmap.md`
    // §2.3), because the per-request server runs beside long-lived data cells.
    let heavy: usize = ostd::args()
        .first()
        .and_then(|arg| arg.as_str().parse().ok())
        .unwrap_or(0);
    if heavy > 0 {
        ostd::io::println(&alloc::format!("[a2a3-probe] heavy_resident={heavy}"));
        for index in 0..heavy {
            // A dedicated binary: its 16 MiB heap arena is declared at build time, so
            // the light children (which are `bench-probe`) keep their small one.
            match ostd::syscall::sys_spawn_pinned(
                "/bin/heavy-probe",
                TaskPriority::Normal as u8,
                0,
            ) {
                SyscallResult::Ok(_) => {}
                SyscallResult::Err(error) => {
                    ostd::io::println(&alloc::format!(
                        "[a2a3-probe] HEAVY_SPAWN_FAILED index={index} error={error:?}"
                    ));
                    ostd::syscall::sys_exit(5);
                }
            }
        }
        // Give them time to commit their grants before the light sweep starts
        // consuming the same heap; the log shows the ordering either way.
        for _ in 0..256 {
            ostd::syscall::sys_yield();
        }
    }

    for count in 0..SPAWN_BOUND {
        if count > 0 && count % PROGRESS_EVERY == 0 {
            ostd::io::println(&alloc::format!("[a2a3-probe] parked count={count}"));
        }
        ostd::syscall::sys_set_spawn_args("resp-echo");
        match ostd::syscall::sys_spawn_pinned("/bin/bench-probe", TaskPriority::Normal as u8, 0) {
            SyscallResult::Ok(_) => {}
            SyscallResult::Err(SyscallError::OutOfMemory) => {
                ostd::io::println(&alloc::format!(
                    "[a2a3-probe] OOM_TYPED count={count} heavy={heavy}"
                ));
                ostd::syscall::sys_exit(0);
            }
            SyscallResult::Err(_) => {
                ostd::io::println("[a2a3-probe] SPAWN_GENERIC_ERROR");
                ostd::syscall::sys_exit(3);
            }
        }
    }

    ostd::io::println(&alloc::format!(
        "[a2a3-probe] OOM_NOT_REACHED bound={SPAWN_BOUND}"
    ));
    ostd::syscall::sys_exit(4)
}
