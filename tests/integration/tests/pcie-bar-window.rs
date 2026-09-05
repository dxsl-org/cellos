//! Dedicated RV64 `no_std` PCIe BAR-window validation gate.

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

const PASS: &str = "[selftest] PCIE-BAR-WINDOW: PASS (bounded, aligned, overflow-safe)";
const FAIL: &str = "[selftest] PCIE-BAR-WINDOW: FAIL";
const POST_SELFTEST_MILESTONE: &str = "ATOMIC_PUBLICATION_AP-15: PASS";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn test_hooks_kernel() -> PathBuf {
    repo_root().join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks")
}

fn prerequisites_ok() -> bool {
    let kernel = test_hooks_kernel();
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel.exists() {
        eprintln!(
            "SKIP: test-hooks kernel not found ({}). Run scripts/build-test-hooks-ci.sh first.",
            kernel.display()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel.exists() && qemu_ok)
}

#[test]
fn riscv64_pcie_bar_window_no_std() {
    if !prerequisites_ok() {
        return;
    }

    let runner = QemuRunner::boot_rv64(test_hooks_kernel().to_str().expect("UTF-8 kernel path"));
    runner.wait_for(PASS, 60).unwrap_or_else(|error| {
        eprintln!("--- serial output ---\n{}\n---", runner.dump());
        panic!("PCIe BAR-window self-test did not pass: {error}");
    });
    runner
        .wait_for(POST_SELFTEST_MILESTONE, 60)
        .unwrap_or_else(|error| {
            eprintln!("--- serial output ---\n{}\n---", runner.dump());
            panic!("kernel did not reach the post-selftest boot milestone: {error}");
        });

    let serial = runner.dump();
    assert_eq!(
        serial.matches(PASS).count(),
        1,
        "PCIe BAR-window PASS marker must appear exactly once:\n{serial}"
    );
    assert!(
        !serial.contains(FAIL)
            && !serial.contains("PCIE-BAR-WINDOW case failed")
            && !serial.contains("[KERNEL PANIC]")
            && !serial.contains("panicked at")
            && !serial.contains("Load access fault")
            && !serial.contains("Store/AMO access fault")
            && !serial.contains("Instruction access fault"),
        "PCIe BAR-window harness reported a failure, panic, or fault:\n{serial}"
    );
}
