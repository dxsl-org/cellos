//! Two-hart VFS integration regression.
//!
//! The VFS test client must receive request/reply traffic only from VFS. This
//! runner keeps that transport boundary covered under concurrent SMP traffic;
//! the single-hart quota runner remains the one-hart VFS contract.

use std::path::PathBuf;

use vicell_integration_tests::{qemu_binary, QemuRunner};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn test_hooks_kernel() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let kernel = PathBuf::from(test_hooks_kernel());
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel.exists() {
        eprintln!(
            "SKIP: test-hooks kernel not found ({}). Run scripts/build-test-hooks-cells.ps1 first.",
            test_hooks_kernel()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel.exists() && qemu_ok)
}

fn wait_for_or_dump(runner: &QemuRunner, pattern: &str) {
    runner.wait_for(pattern, 60).unwrap_or_else(|error| {
        panic!("{error}\n--- serial output ---\n{}\n---", runner.dump());
    });
}

#[test]
fn riscv64_vfs_smp_all_pass() {
    if !prerequisites_ok() {
        return;
    }

    let runner = QemuRunner::boot_rv64_smp(&test_hooks_kernel(), 2);
    wait_for_or_dump(&runner, "[smp] hart 1 online");
    // Per-hart UART writes can interleave stage records, so each marker below
    // is checked for presence, not for its position relative to the others.
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-CONTEXT-HANDOFF: PASS aggregate=CTX00-03",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-scheduler-funnel-pre-lock-attempt-published",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart0-scheduler-owner-retained-hart1-retirement-blocked",
    );
    wait_for_or_dump(&runner, "[selftest] SMP-FAULT-RETIREMENT: PASS");
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-fault-retirement-quiesced",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-RETIREMENT: stage=selected-context-blocked-retirement-and-reap",
    );
    // The retirement boundary is a separate, stable post-switch proof; its
    // per-hart progress records need not be contiguous in the UART stream.
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-RETIREMENT: stage=rv64-switch-boundary hart=1",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-RETIREMENT: stage=idle-attribution-cleared current=0 executing=0 selected=0 cell=0",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] SMP-RETIREMENT: PASS (selected Context + zombie switch completion gate owner release + CellId reuse)",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] HEARTBEAT-TERMINAL-IDENTITY: PASS (heartbeat retirement retained nonzero caller through boot switch; ReadLog denied)",
    );
    // Init must survive enough two-hart scheduling pressure to finish its
    // VFS/config launch sequence before the VFS client exercises the service.
    wait_for_or_dump(&runner, "Init: services spawned.");
    wait_for_or_dump(&runner, "Init: service registry verified (");

    for marker in [
        "ATOMIC_PUBLICATION_AP-00: PASS",
        "ATOMIC_PUBLICATION_AP-01: PASS",
        "ATOMIC_PUBLICATION_AP-02: PASS",
        "ATOMIC_PUBLICATION_AP-03: PASS",
        "ATOMIC_PUBLICATION_AP-04: PASS",
        "ATOMIC_PUBLICATION_AP-05: PASS",
        "ATOMIC_PUBLICATION_AP-06: PASS",
        "ATOMIC_PUBLICATION_AP-07: PASS",
        "ATOMIC_PUBLICATION_AP-08: PASS",
        "ATOMIC_PUBLICATION_AP-09: PASS",
        "ATOMIC_PUBLICATION_AP-10: PASS",
        "ATOMIC_PUBLICATION_AP-11: PASS",
        "ATOMIC_PUBLICATION_AP-12: PASS",
        "ATOMIC_PUBLICATION_AP-13: PASS",
        "ATOMIC_PUBLICATION_AP-14: PASS",
        "ATOMIC_PUBLICATION_AP-15: PASS",
        "ATOMIC_PUBLICATION_ALL: PASS",
    ] {
        wait_for_or_dump(&runner, marker);
    }
    wait_for_or_dump(
        &runner,
        "[selftest] VFS-LIFETIME: stage=dead-owner-pending-revoke-exact-release",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] VFS-LIFETIME: stage=smp-stale-context-denied-capacity-preserved",
    );
    wait_for_or_dump(
        &runner,
        "[selftest] VFS-LIFETIME: PASS (atomic grant-table lease + teardown orders + exact quarantine + owner watch)",
    );

    wait_for_or_dump(&runner, "[vfs-test] ALL TESTS PASSED");

    let serial = runner.dump();
    assert!(
        !serial.lines().any(|line| line.contains("[FAIL]")),
        "two-hart VFS runner must have no VFS failure lines:\n{serial}",
    );
    assert!(
        !serial.contains("SMP-CONTEXT-HANDOFF: FAIL"),
        "two-hart VFS runner must not report a failed Context handoff:\n{serial}",
    );
    assert!(
        !serial.contains("[vfs-test] FAILURES DETECTED"),
        "two-hart VFS runner must not report test failures:\n{serial}",
    );
}
