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
//!   target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks
//!
//! Run:
//!   cargo test --manifest-path tests/integration/Cargo.toml \
//!              --target x86_64-pc-windows-msvc vfs_quota

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};
const VFS_LIFECYCLE_SELFTEST: &str =
    include_str!("../../../kernel/src/task/vfs_lifecycle_selftest.rs");
const VFS_LIFETIME_PASS_PREFIX: &str = "[selftest] VFS-LIFETIME: PASS";

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
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-test-hooks")
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

/// Boot the single-hart test-hooks kernel (no disk — embedded FS only,
/// guarantees the test-hooks service-vfs with 2 KiB quota runs). AP-12/AP-14
/// still prove governed pre-ready publication, while AP-13 explicitly skips:
/// this runner has no live remote scheduler barrier to certify.
fn wait_for_or_dump(runner: &QemuRunner, pattern: &str) {
    runner.wait_for(pattern, 60).unwrap_or_else(|e| {
        eprintln!("--- serial output ---\n{}\n---", runner.dump());
        panic!("{e}");
    });
}
#[test]
fn vfs_lifecycle_selftest_covers_required_stages() {
    let self_test = &VFS_LIFECYCLE_SELFTEST[VFS_LIFECYCLE_SELFTEST
        .find("pub fn self_test()")
        .expect("VFS lifetime self-test entry point exists")..];
    let exact_lease = self_test
        .find("exact_lease_release()")
        .expect("VFS lifetime self-test runs exact-lease coverage");
    let quarantine = self_test
        .find("quarantine_waits_for_exact_release()")
        .expect("VFS lifetime self-test runs quarantine coverage");
    let stale_install = self_test
        .find("stale_context_install_is_denied_without_capacity_loss()")
        .expect("VFS lifetime self-test runs stale-install denial coverage");
    let owner_watch = self_test
        .find("vfs_owner_watch()")
        .expect("VFS lifetime self-test runs owner-watch coverage");
    assert!(
        exact_lease < quarantine && quarantine < stale_install && stale_install < owner_watch,
        "VFS lifetime self-test must retain its exact-lease, quarantine, stale-install, and owner-watch stages"
    );

    let pending_revoke = VFS_LIFECYCLE_SELFTEST
        .find("stage=dead-owner-pending-revoke-exact-release")
        .expect("VFS lifetime self-test records pending-revoke exact-release proof");
    let stale_install_marker = VFS_LIFECYCLE_SELFTEST
        .find("stage=smp-stale-context-denied-capacity-preserved")
        .expect("VFS lifetime self-test records stale-install denial proof");
    assert!(
        pending_revoke < stale_install_marker,
        "VFS lifetime proof must record pending revoke before stale-install denial"
    );
}

#[test]
fn riscv64_vfs_quota_all_pass() {
    if !prerequisites_ok() {
        return;
    }

    // boot_rv64 intentionally remains single-hart: this is the VFS contract,
    // not the separate SMP atomic-publication contract.
    let runner = QemuRunner::boot_rv64(&test_hooks_kernel());

    for marker in [
        "ATOMIC_PUBLICATION_AP-12: PASS",
        "ATOMIC_PUBLICATION_AP-14: PASS",
        "ATOMIC_PUBLICATION_AP-13: SKIP (hart 1 not online; SMP probe not required)",
        "ATOMIC_PUBLICATION_AP-15: PASS",
    ] {
        wait_for_or_dump(&runner, marker);
    }

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
    wait_for_or_dump(&runner, "terminated: cause=0xf");
    wait_for_or_dump(&runner, "[stack-baseline] name=init phase=boot ");
    wait_for_or_dump(&runner, "[stack-baseline] name=shell phase=boot ");
    wait_for_or_dump(&runner, "[stack-baseline] name=vfs phase=boot ");
    wait_for_or_dump(
        &runner,
        "[vfs-file-handle] wrong-owner-read-close-preserves-entry PASS",
    );
    wait_for_or_dump(&runner, "[vfs-file-handle] quota-32-per-owner PASS");
    wait_for_or_dump(
        &runner,
        "[vfs-file-handle] nonreuse-and-u64-exhaustion PASS",
    );
    wait_for_or_dump(&runner, "[vfs-file-handle] exact-generation-purge PASS");
    wait_for_or_dump(
        &runner,
        "[vfs-file-handle] parent-cross-owner-transitive-revoke PASS",
    );
    wait_for_or_dump(
        &runner,
        "[vfs-file-handle] owner-watch-filehandle-cleanup PASS",
    );
    wait_for_or_dump(&runner, "[vfs-file-handle] higher-generation-cleanup PASS");
    wait_for_or_dump(&runner, VFS_LIFETIME_PASS_PREFIX);
    wait_for_or_dump(
        &runner,
        "[PASS] dircap: GetFile returns a nonempty pointer before sealing",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] dircap: revoking a parent dir reaps file handles opened below it",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] grant: ReadFileGrant clamps to grant length",
    );
    wait_for_or_dump(&runner, "[PASS] grant: ReadFileGrant copies nonzero bytes");
    wait_for_or_dump(
        &runner,
        "[PASS] grant-write: unknown handle is denied before grant access",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] grant-write: helper returns committed byte count",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] grant-write: commits exact bytes before acknowledgement",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] grant-write: invalid offset is refused before grant access",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] grant-write: short grant is refused without mutation",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] grant-write: failed writes leave committed bytes unchanged",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] grant-write: write authorization precedes grant access",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] grant-write: quota overflow is refused before grant access",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] grant-write: quota refusal leaves committed bytes unchanged",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] grant: ReadFileGrant is refused after sealing",
    );
    wait_for_or_dump(
        &runner,
        "[PASS] dircap: ReadFileHandle still works after sealing",
    );
    wait_for_or_dump(&runner, "[vfs-test] ALL TESTS PASSED");
    wait_for_or_dump(&runner, "[stack-baseline] name=vfs-test phase=exit ");

    let serial = runner.dump();
    assert!(
        !serial.contains("[FAIL] grant:"),
        "--- serial output ---\n{}\n---",
        serial
    );
    assert!(
        !serial.contains("[FAIL] grant-write:"),
        "--- serial output ---\n{}\n---",
        serial
    );
    assert!(
        !serial.contains("ATOMIC_PUBLICATION_AP-13: PASS")
            && !serial.contains("ATOMIC_PUBLICATION_ALL: PASS"),
        "single-hart VFS runner must not certify the SMP atomic contract:\n{serial}",
    );
}
