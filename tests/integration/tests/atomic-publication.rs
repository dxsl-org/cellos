//! Two-hart governed atomic-publication integration test.
//!
//! This is intentionally separate from the one-hart VFS quota runner: AP-13
//! only passes when an online remote hart reaches the scheduler barrier.

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
fn riscv64_atomic_publication_smp_all_pass() {
    if !prerequisites_ok() {
        return;
    }

    // Explicitly request both harts. AP-13 must not be inferred from the
    // single-hart runner or from the other governed-success observations.
    let runner = QemuRunner::boot_rv64_smp(&test_hooks_kernel(), 2);
    wait_for_or_dump(&runner, "[smp] hart 1 online");

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
        "ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED",
        "ATOMIC_PUBLICATION_ALL: PASS",
    ] {
        wait_for_or_dump(&runner, marker);
    }

    let serial = runner.dump();
    assert!(
        !serial.contains("ATOMIC_PUBLICATION_AP-13: SKIP"),
        "two-hart runner must prove, not skip, AP-13:\n{serial}",
    );
}
