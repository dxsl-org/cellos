//! RV64 QEMU continuity guard for the frozen Manifest v1/v2 baseline.
//!
//! Test contract: run `python3 scripts/validate-manifest-abi-predesign.py`
//! before this test. `scripts/qemu-manifest-continuity.sh` performs that
//! immutable-artifact preflight before invoking this integration target.
//!
//! The test intentionally observes only existing test-hooks runtime terminals:
//! the ELF loader's v1/v2 cases and the Manifest-v2 self-test. It does not
//! create a Manifest v3 fixture, parser, writer, or readiness claim.

use std::path::PathBuf;

use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;

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
            "SKIP manifest-qemu-continuity: test-hooks kernel not found ({}). Run scripts/build-test-hooks-cells.ps1 first.",
            kernel.display()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP manifest-qemu-continuity: qemu-system-riscv64 not on PATH");
    }

    vicell_integration_tests::ci_guard(kernel.exists() && qemu_ok)
}

fn wait_for_or_dump(runner: &QemuRunner, marker: &str) {
    runner.wait_for(marker, BOOT_TIMEOUT).unwrap_or_else(|error| {
        panic!(
            "manifest QEMU continuity marker {marker:?} not observed: {error}\n--- serial output ---\n{}\n---",
            runner.dump()
        )
    });
}

#[test]
fn riscv64_manifest_v1_v2_runtime_continuity() {
    if !prerequisites_ok() {
        return;
    }

    // `boot_rv64` fixes this regression guard to the existing one-hart RV64
    // QEMU tuple; it cannot be read as SMP, hardware, or readiness evidence.
    let runner = QemuRunner::boot_rv64(&test_hooks_kernel());
    for marker in [
        "[selftest] ELF-LOADER: PASS",
        "[selftest] MANIFEST-V2: PASS",
    ] {
        wait_for_or_dump(&runner, marker);
    }

    let serial = runner.dump();
    for failure in [
        "[selftest] ELF-LOADER: FAIL",
        "[selftest] MANIFEST-V2: FAIL",
        "Manifest-v2 self-test FAIL",
        "[KERNEL PANIC]",
    ] {
        assert!(
            !serial.contains(failure),
            "manifest QEMU continuity observed {failure:?}\n--- serial output ---\n{serial}\n---"
        );
    }
}
