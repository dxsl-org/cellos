//! Local service-call lifecycle witness (x86_64, QEMU q35, BIOS/Limine ISO).
//!
//! Boots the isolated witness image built by
//! `scripts/build-x86_64-c2c-lifecycle-ci.sh` and runs
//! `bench local-service-lifecycle`, which drives two legs against a provider task
//! that dies without replying:
//!
//! * **synchronous** — `ostd::ipc::service_call_typed` (the path
//!   `LocalEndpoint::call` / `ServiceRef::call` use): the caller sends (a
//!   handoff), then parks in a sender-masked `Recv`. `exit_task` deliberately does
//!   not wake a plain reply waiter that has already left `Sending`
//!   (`kernel/src/task/scheduler.rs:1279-1284`), so `SYNC-RESULT=RETURNED` must
//!   **not** appear;
//! * **asynchronous** — the bounded exact-operation path
//!   (`ostd::ipc::submit`/`wait`/`take`), whose peer binding is
//!   `(cell_id, cell_generation)` and whose `peer_died` transition is terminal, so
//!   `ASYNC-TERMINAL=PEER-GONE` must appear.
//!
//! The two legs in one run are the point: the same kernel, the same dead peer,
//! two different observables. This is behaviour evidence for
//! `docs/decisions/0023-local-service-generation-binding.md`; it asserts current
//! behaviour and fixes nothing.
//!
//! Skips gracefully when the ISO or QEMU is absent (hard-fails under `CI=`).

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary_x86, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;
/// The async leg's own budget is 10 × 2 s, plus settling yields for two spawns.
const SCENARIO_TIMEOUT: u64 = 90;

const START: &str = "[local-lifecycle] START";
/// Leg 0: the additive `LookupServiceBound = 429` resolved a real service.
const VFS_BINDING_PREFIX: &str = "[local-lifecycle] VFS-BINDING";
/// Leg 0 negative: an id with no provider is reported as no binding.
const ABSENT_BINDING: &str = "[local-lifecycle] ABSENT-BINDING";
const PROVIDER_READY: &str = "[local-lifecycle] provider-ready";
const SYNC_CALLER_START: &str = "[local-lifecycle] sync-caller-start";
/// Emitted only if the kernel ever wakes the synchronous caller. Absence is the finding.
const SYNC_RETURNED: &str = "[local-lifecycle] SYNC-RESULT=RETURNED";
const SYNC_LEG_DONE: &str = "[local-lifecycle] SYNC-LEG=complete";
const ASYNC_PEER_GONE: &str = "[local-lifecycle] ASYNC-TERMINAL=PEER-GONE";
const PASS: &str = "[local-lifecycle] PASS";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

/// Path to the isolated witness ISO. Override with `CELLOS_X86_C2C_LIFECYCLE_ISO`.
fn lifecycle_iso_path() -> String {
    if let Ok(path) = std::env::var("CELLOS_X86_C2C_LIFECYCLE_ISO") {
        if !path.is_empty() {
            return path;
        }
    }
    repo_root()
        .join("build/vicell-x86-c2c-lifecycle.iso")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let iso = lifecycle_iso_path();
    let iso_exists = PathBuf::from(&iso).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_x86())
        .arg("--version")
        .output()
        .is_ok();
    if !iso_exists {
        eprintln!("SKIP x86_64 local-service lifecycle: witness ISO not built ({iso})");
        eprintln!("  Run: bash scripts/build-x86_64-c2c-lifecycle-ci.sh");
    }
    if !qemu_ok {
        eprintln!("SKIP x86_64 local-service lifecycle: qemu-system-x86_64 not found");
    }
    vicell_integration_tests::ci_guard(iso_exists && qemu_ok)
}

#[test]
fn x86_dead_provider_strands_sync_caller_but_bounds_async_call() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&lifecycle_iso_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "x86_64 shell prompt not reached: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("bench local-service-lifecycle");
    qemu.wait_for(PASS, SCENARIO_TIMEOUT).unwrap_or_else(|e| {
        panic!(
            "lifecycle witness did not complete: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    let output = qemu.dump();

    // The scenario must have actually run both legs: a missing provider anchor
    // would let the negative assertion below pass for the wrong reason.
    for anchor in [
        START,
        VFS_BINDING_PREFIX,
        ABSENT_BINDING,
        PROVIDER_READY,
        SYNC_CALLER_START,
        SYNC_LEG_DONE,
    ] {
        assert!(
            output.contains(anchor),
            "missing lifecycle anchor {anchor:?}\n--- output ---\n{output}"
        );
    }

    // Leg 0: the additive opcode resolved the real provider. The binding must name
    // the same task `LookupService` does, and must carry a live Cell identity — a
    // capture bug stores 0/0, which makes the kernel report *no* binding instead.
    let vfs_line = output
        .lines()
        .find(|line| line.contains(VFS_BINDING_PREFIX))
        .expect("VFS-BINDING line");
    assert!(
        vfs_line.contains("matches_lookup=true"),
        "LookupServiceBound disagreed with LookupService on the provider tid: {vfs_line}"
    );
    assert!(
        !vfs_line.contains("cell=0 ") && !vfs_line.contains("gen=0 "),
        "the binding carried no live Cell identity — the provider's generation was \
         not captured at registration: {vfs_line}"
    );

    // Leg 0 negative: an unregistered id is "no binding", stated the same way by
    // both opcodes.
    let absent_line = output
        .lines()
        .find(|line| line.contains(ABSENT_BINDING))
        .expect("ABSENT-BINDING line");
    assert!(
        absent_line.contains("lookup=None") && absent_line.contains("bound_is_none=true"),
        "an unregistered service must resolve to no provider and no binding: {absent_line}"
    );

    // Bounded exact-operation path: the kernel binds the peer by
    // (cell_id, cell_generation) and transitions the operation to a terminal.
    assert!(
        output.contains(ASYNC_PEER_GONE),
        "the exact-operation path did not report a PEER-GONE terminal\n--- output ---\n{output}"
    );

    // Synchronous masked reply wait: the caller must not have been woken by the
    // provider's death. If this fires, the kernel gained a bounded terminal for
    // this shape and ADR-0023's premise must be re-reviewed rather than silently
    // accepted.
    assert!(
        !output.contains(SYNC_RETURNED),
        "synchronous caller was woken after its provider died — the witness premise \
         in docs/decisions/0023-local-service-generation-binding.md needs review\n\
         --- output ---\n{output}"
    );
}
