//! VFS quota integration test.
//!
//! Boots a test-hooks RISC-V kernel (no disk — embedded FS only) and verifies
//! that the in-guest `vfs-test` cell runs all test scenarios — including the
//! quota-enforcement scenario that requires a 2 KiB quota limit.
//!
//! All vfs-test paths use /tmp (RamFS), so no block device is needed.
//! The quota tracker in dispatch.rs charges every successful write regardless
//! of which backend path is used, making /tmp quota tests valid.
//!
//! Prerequisites (run scripts/build-test-hooks-cells.ps1 first):
//!   target/riscv64gc-unknown-none-elf/release/vicell-kernel-test-hooks
//!
//! Run:
//!   cargo test --manifest-path tests/integration/Cargo.toml \
//!              --target x86_64-pc-windows-msvc vfs_quota

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

/// Path to the test-hooks kernel produced by scripts/build-test-hooks-cells.ps1.
fn test_hooks_kernel() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/vicell-kernel-test-hooks")
        .to_string_lossy()
        .into_owned()
}

/// Skip the test instead of failing when prerequisites are missing.
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

/// Boot the test-hooks kernel (no disk — embedded FS only, guarantees the
/// test-hooks service-vfs with 2 KiB quota runs), then wait for vfs-test to
/// report all scenarios passed.
#[test]
fn riscv64_vfs_quota_all_pass() {
    if !prerequisites_ok() {
        return;
    }

    // boot_rv64: no disk, no extra production cells — clean environment for
    // the quota integration test.
    let runner = QemuRunner::boot_rv64(&test_hooks_kernel());

    // vfs-test prints this banner when all scenarios pass (exit 0).
    runner
        .wait_for("[vfs-test] ALL TESTS PASSED", 60)
        .unwrap_or_else(|e| {
            eprintln!("--- serial output ---\n{}\n---", runner.dump());
            panic!("{e}");
        });
}
