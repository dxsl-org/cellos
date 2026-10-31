//! Private 1/2-hart CSpace authority provenance witness (ginux-lessons 07C).
//!
//! Boots the RV64 test-hooks kernel (`scripts/build-test-hooks-ci.sh`) with an
//! explicit hart count. One hart: the probe passes on hart 0 and the remote half
//! is reported runtime-gated. Two harts: hart 1 must come online and pass the
//! same probe against the shared authority table.

use std::path::PathBuf;

use vicell_integration_tests::{qemu_binary, QemuRunner};

fn test_hooks_kernel() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let kernel = PathBuf::from(test_hooks_kernel()).exists();
    if !kernel {
        eprintln!("SKIP: test-hooks kernel missing; run scripts/build-test-hooks-ci.sh");
    }
    let qemu = std::process::Command::new(qemu_binary()).arg("--version").output().is_ok();
    vicell_integration_tests::ci_guard(kernel && qemu)
}

fn expect(runner: &QemuRunner, pattern: &str) {
    runner.wait_for(pattern, 90).unwrap_or_else(|error| {
        panic!("{error}\n--- serial output ---\n{}\n---", runner.dump());
    });
}

fn assert_no_failure(runner: &QemuRunner) {
    let serial = runner.dump();
    assert!(!serial.contains("AUTHORITY-PROVENANCE: FAIL"), "probe failed:\n{serial}");
}

#[test]
fn one_hart_proves_local_and_gates_remote() {
    if !prerequisites_ok() {
        return;
    }
    let runner = QemuRunner::boot_rv64_smp(&test_hooks_kernel(), 1);
    expect(&runner, "[selftest] AUTHORITY-PROVENANCE: PASS hart=0");
    expect(&runner, "[selftest] AUTHORITY-PROVENANCE: remote RUNTIME-GATED (hart 1 offline)");
    assert_no_failure(&runner);
}

#[test]
fn two_harts_prove_the_probe_on_the_remote_hart() {
    if !prerequisites_ok() {
        return;
    }
    let runner = QemuRunner::boot_rv64_smp(&test_hooks_kernel(), 2);
    expect(&runner, "[smp] hart 1 online");
    expect(&runner, "[selftest] AUTHORITY-PROVENANCE: PASS hart=0");
    expect(&runner, "[selftest] AUTHORITY-PROVENANCE: PASS hart=1");
    assert_no_failure(&runner);
    assert!(!runner.dump().contains("AUTHORITY-PROVENANCE: remote RUNTIME-GATED"));
}
