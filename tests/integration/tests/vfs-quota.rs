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
fn wait_for_or_dump(runner: &QemuRunner, pattern: &str) {
    runner.wait_for(pattern, 60).unwrap_or_else(|e| {
        eprintln!("--- serial output ---\n{}\n---", runner.dump());
        panic!("{e}");
    });
}

#[test]
fn riscv64_vfs_quota_all_pass() {
    if !prerequisites_ok() {
        return;
    }

    // boot_rv64: no disk, no extra production cells — clean environment for
    // the quota integration test.
    let runner = QemuRunner::boot_rv64(&test_hooks_kernel());

    wait_for_or_dump(
        &runner,
        "stack-probe self-test PASS (two guards, overflow target unmapped, watermark)",
    );
    wait_for_or_dump(
        &runner,
        "stack-sizing policy self-test PASS (measured=16, unknown=64)",
    );
    wait_for_or_dump(
        &runner,
        "[stack-guard] deliberate overflow armed guard_pages=2",
    );
    wait_for_or_dump(
        &runner,
        "'stack_overflow_probe') terminated: cause=0xf",
    );
    wait_for_or_dump(&runner, "[stack-baseline] name=init phase=boot ");
    wait_for_or_dump(&runner, "[stack-baseline] name=shell phase=boot ");
    wait_for_or_dump(&runner, "[stack-baseline] name=vfs phase=boot ");
    wait_for_or_dump(&runner, "[PASS] dircap: GetFile returns a nonempty pointer before sealing");
    wait_for_or_dump(&runner, "[PASS] grant: ReadFileGrant clamps to grant length");
    wait_for_or_dump(&runner, "[PASS] grant: ReadFileGrant copies nonzero bytes");
    wait_for_or_dump(&runner, "[PASS] grant: ReadFileGrant is refused after sealing");
    wait_for_or_dump(&runner, "[vfs-test] ALL TESTS PASSED");
    wait_for_or_dump(&runner, "[stack-baseline] name=vfs-test phase=exit ");

    let serial = runner.dump();
    assert!(
        !serial.contains("[FAIL] grant:"),
        "--- serial output ---\n{}\n---",
        serial
    );
}
