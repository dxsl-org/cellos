//! Test fixture launcher (Phase 13: the fixtures left init).
//!
//! Init launches `/bin/fixture-launcher` from the boot plan when, and only when,
//! a test image packages it (the plan entry is selected by artifact). It starts
//! the fixtures init's boot code used to start, in the same order, on its own
//! reviewed `fixture-launcher` edges with the same child ceilings, then exits.
//! Every spawn tries the kernel path route first (VIFS1 and the P2 table), so a
//! boot-resident child is named by its signed image exactly as it was when
//! trusted init launched it; a cell only the VFS can read (the cell store of an
//! RV64 disk) is then read through the VFS and spawned from those bytes, the
//! route init used for every fixture.
//!
//! Cargo features select the opt-in sets; without any, it still starts the five
//! boot tests (`/bin/silo-test`, `/bin/ai-test`, `/bin/vfs-test`, `/bin/srv-test`,
//! `/bin/std-smoke`), each absent one failing with `NotFound` as before.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;
extern crate ostd;

api::declare_manifest!(block_io = false, network = false, spawn = true);
api::declare_syscalls![
    Log,
    Exit,
    SpawnFromPath,
    SpawnFromElf,
    // The VFS read of a cell-store fixture (`ostd::fs::read_full_via_grant`).
    Send,
    SendGather,
    Recv,
    TryRecv,
    IpcSubmit,
    IpcTake,
    IpcWait,
    IpcCancel,
    GrantAlloc,
    GrantShare,
    GrantFree,
    StateStash,
    RegisterService,
    LookupServiceBound,
    GetProcs,
    GetTime,
];

use ostd::syscall::{sys_spawn_from_path, sys_spawn_from_path_raw, SyscallResult};

ostd::cell_main!(extern "C" cell_main);

/// The Phase-02 cross-tier fixture's service id (`tier2-rpc-proto::SERVICE_ID`).
/// A literal rather than a dependency on the fixture crate; a drift is caught by
/// the driver's first resolution, which fails loudly.
#[cfg(feature = "tier2-rpc-entry")]
const TIER2_RPC_FIXTURE_SERVICE: u16 = 0x7A01;

/// The DMA-export lane's holder service (`edu-export-proto::SERVICE_ID`, admitted
/// by `test-hooks` kernels as `dma_export::TEST_EXPORT_SERVICE`).
#[cfg(feature = "edu-export-entry")]
const EDU_EXPORT_HOLDER_SERVICE: u16 = 0x7AE0;

/// Spawn `path` with the command line `args` (`None`: none). A refused attempt
/// consumes the staged command line, so it is republished before the VFS route.
fn spawn_with(path: &str, args: Option<&str>) -> Option<usize> {
    let stage = || args.is_none_or(ostd::syscall::sys_set_spawn_args);
    if !stage() {
        return None;
    }
    if let SyscallResult::Ok(tid) = sys_spawn_from_path_raw(path) {
        return (tid != 0).then_some(tid);
    }
    if !stage() {
        return None;
    }
    match sys_spawn_from_path(path) {
        SyscallResult::Ok(tid) if tid != 0 => Some(tid),
        _ => None,
    }
}

fn spawn(path: &str) -> Option<usize> {
    spawn_with(path, None)
}

fn cell_main() {
    // Tier-2 entry fixtures (phase 02), first in the order, so the AArch64
    // test-hooks image schedules them before its test root ends the boot.
    #[cfg(feature = "tier2-entry")]
    for (path, admitted, failed) in [
        (
            "/bin/tier2-smoke",
            "fixture-launcher: tier2-smoke admitted.",
            "fixture-launcher: tier2-smoke spawn failed.",
        ),
        (
            "/bin/tier2-exploit",
            "fixture-launcher: tier2-exploit admitted.",
            "fixture-launcher: tier2-exploit spawn failed.",
        ),
        (
            "/bin/cpp-smoke",
            "fixture-launcher: cpp-smoke admitted.",
            "fixture-launcher: cpp-smoke spawn failed.",
        ),
    ] {
        ostd::io::println(if spawn(path).is_some() { admitted } else { failed });
    }

    // Phase-02 slice B cross-tier exchange. A private-root provider holds no
    // capability and cannot register itself, so its spawner registers it; the
    // kernel captures the provider's identity in the same lock hold as the
    // liveness check, and a private id is this spawner's to publish.
    #[cfg(feature = "tier2-rpc-entry")]
    match spawn("/bin/tier2-rpc-provider") {
        Some(provider) => {
            ostd::io::println("fixture-launcher: tier2-rpc-provider admitted.");
            ostd::io::println(
                match ostd::syscall::sys_register_service(TIER2_RPC_FIXTURE_SERVICE, provider, 0) {
                    SyscallResult::Ok(_) => "fixture-launcher: tier2-rpc-provider registered.",
                    SyscallResult::Err(_) => {
                        "fixture-launcher: tier2-rpc-provider registration failed."
                    }
                },
            );
            ostd::io::println(match spawn("/bin/tier2-rpc-driver") {
                Some(_) => "fixture-launcher: tier2-rpc-driver launched.",
                None => "fixture-launcher: tier2-rpc-driver spawn failed.",
            });
        }
        None => ostd::io::println("fixture-launcher: tier2-rpc-provider spawn failed."),
    }

    // ADR-0033 CP1 §7 DMA-export lane: the holder is registered before a client
    // resolves it.
    #[cfg(feature = "edu-export-entry")]
    match spawn("/bin/edu-holder") {
        Some(holder) => {
            ostd::io::println(
                match ostd::syscall::sys_register_service(EDU_EXPORT_HOLDER_SERVICE, holder, 0) {
                    SyscallResult::Ok(_) => "fixture-launcher: edu-holder registered.",
                    SyscallResult::Err(_) => "fixture-launcher: edu-holder registration failed.",
                },
            );
            for _ in 0..2 {
                ostd::io::println(match spawn("/bin/export-client") {
                    Some(_) => "fixture-launcher: export-client launched.",
                    None => "fixture-launcher: export-client spawn failed.",
                });
            }
        }
        None => ostd::io::println("fixture-launcher: edu-holder spawn failed."),
    }

    // Phase-03 step-5 pair, before the boot tests below so a generation's
    // terminal fault never races the image's own test root.
    #[cfg(feature = "tier2-grant-pair")]
    grant_pair::run();

    for path in [
        "/bin/silo-test",
        // Spec 24 G2 Level A oracle: exercises the inference service over typed IPC.
        "/bin/ai-test",
        "/bin/vfs-test",
        "/bin/srv-test",
        "/bin/std-smoke",
    ] {
        let _ = spawn(path);
    }
}

/// The phase-03 Tier-2 grant pair, from the boot order.
///
/// The AArch64 test image has no interactive window: the shell's first prompt
/// arrives long after the image's own test root exits the VM, so the pair cannot
/// be typed at a shell the way the RV64 lane drives it. Both cells run on the
/// reviewed `/bin/silo` and `/bin/net-broker` paths, whose real services that
/// image does not build. Every marker the pair emits is namespaced
/// `S22-<arch>-GRANT-PAIR-*`.
#[cfg(feature = "tier2-grant-pair")]
mod grant_pair {
    use super::spawn_with;
    use ostd::io::println;

    const OWNER_PATH: &str = "/bin/silo";
    const RECEIVER_PATH: &str = "/bin/net-broker";
    /// One receiver generation per property the phase must witness, in the order
    /// the owner serves them: ReadWrite, ReadOnly, same-recipient downgrade,
    /// GrantUnregister, owner exit.
    const MODES: [&str; 5] = ["rw", "ro", "downgrade", "unregister", "exit"];
    /// ~3 s of 10 ms scheduler ticks: two orders of magnitude above the ~200 ms
    /// one generation costs, bounding a generation that neither faults nor exits.
    const DEATH_TIMEOUT_TICKS: u64 = 300;
    /// Backstop for a tick clock that never advances.
    const DEATH_POLL_LIMIT: usize = 5_000_000;

    fn task_name(path: &str) -> &str {
        path.rsplit('/').next().unwrap_or(path)
    }

    fn now_ticks() -> u64 {
        ostd::syscall::sys_get_scheduler_ticks().unwrap_or(0)
    }

    /// Has `tid` terminated? A task the listing no longer carries has; a listing
    /// that cannot be taken is not evidence of death.
    fn terminated(tid: usize) -> bool {
        let mut rows = [api::syscall::ProcessInfo::default(); 64];
        match ostd::syscall::sys_get_procs(&mut rows) {
            Ok(count) => rows
                .iter()
                .take(count)
                .all(|row| row.id != tid || row.state == 3),
            Err(_) => false,
        }
    }

    /// The owner starts once with the receiver's task name; each generation
    /// starts with its mode and the owner's task name and must be gone before
    /// the next starts (the phases share grant ids, and the fault address is
    /// what attributes each fault to its phase). Grant ids travel over the
    /// pair's own IPC, never on a command line.
    pub(super) fn run() {
        let owner_name = task_name(OWNER_PATH);
        let receiver_name = task_name(RECEIVER_PATH);
        if spawn_with(OWNER_PATH, Some(&alloc::format!("peer={receiver_name}"))).is_none() {
            println("fixture-launcher: tier2-grant-pair owner spawn failed.");
            return;
        }
        println("fixture-launcher: tier2-grant-pair owner admitted.");
        for mode in MODES {
            // Every launch republishes the command line: a spawn attempt consumes it.
            let args = alloc::format!("{mode} peer={owner_name}");
            let Some(tid) = spawn_with(RECEIVER_PATH, Some(&args)) else {
                println(&alloc::format!(
                    "fixture-launcher: tier2-grant-pair generation {mode} spawn failed."
                ));
                return;
            };
            let started = now_ticks();
            let mut polls = 0usize;
            while !terminated(tid) {
                if now_ticks().wrapping_sub(started) >= DEATH_TIMEOUT_TICKS
                    || polls >= DEATH_POLL_LIMIT
                {
                    println(&alloc::format!(
                        "fixture-launcher: tier2-grant-pair generation {mode} did not terminate."
                    ));
                    return;
                }
                ostd::task::yield_now();
                polls += 1;
            }
        }
        println("fixture-launcher: tier2-grant-pair complete.");
    }
}
