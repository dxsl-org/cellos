//! Runtime proof for the loaded-policy missing-entry P-TRUST branch.

use std::path::PathBuf;
use vicell_integration_tests::{skip_notice, QemuRunner};

#[test]
fn incomplete_policy_strips_ptrust_and_audits() {
    let Some(kernel) = std::env::var_os("ViCell_POLICY_NOENTRY_KERNEL") else {
        skip_notice("ViCell_POLICY_NOENTRY_KERNEL is not set");
        return;
    };
    let kernel = PathBuf::from(kernel);
    assert!(
        kernel.is_file(),
        "fixture kernel missing: {}",
        kernel.display()
    );

    let qemu = QemuRunner::boot_rv64(&kernel.to_string_lossy());
    qemu.wait_for("policy verify+parse self-test PASS", 15)
        .unwrap_or_else(|error| panic!("{error}\n--- output ---\n{}", qemu.dump()));
    qemu.wait_for("[policy] loaded + verified (22 entries", 15)
        .unwrap_or_else(|error| panic!("{error}\n--- output ---\n{}", qemu.dump()));
    qemu.wait_for("no entry for \"/bin/nvme\"", 20)
        .unwrap_or_else(|error| panic!("{error}\n--- output ---\n{}", qemu.dump()));
    qemu.wait_for("privileged caps stripped 0b001", 5)
        .unwrap_or_else(|error| panic!("{error}\n--- output ---\n{}", qemu.dump()));
    assert!(
        !qemu.output_contains("[KERNEL PANIC]"),
        "incomplete policy caused a kernel panic\n{}",
        qemu.dump()
    );
}

#[test]
fn complete_policy_has_no_false_positive() {
    let (Some(kernel), Some(disk)) = (
        std::env::var_os("ViCell_POLICY_COMPLETE_KERNEL"),
        std::env::var_os("ViCell_POLICY_COMPLETE_DISK"),
    ) else {
        skip_notice("complete-policy kernel or disk is not set");
        return;
    };
    let kernel = PathBuf::from(kernel);
    let disk = PathBuf::from(disk);
    assert!(kernel.is_file(), "kernel missing: {}", kernel.display());
    assert!(disk.is_file(), "disk missing: {}", disk.display());

    let qemu = QemuRunner::boot_with_fresh_disk(&kernel.to_string_lossy(), &disk.to_string_lossy());
    qemu.wait_for("[policy] loaded + verified (23 entries", 40)
        .unwrap_or_else(|error| panic!("{error}\n--- output ---\n{}", qemu.dump()));
    qemu.wait_for("Cellos >", 40)
        .unwrap_or_else(|error| panic!("{error}\n--- output ---\n{}", qemu.dump()));
    assert!(
        !qemu.output_contains("privileged caps stripped"),
        "complete policy emitted a missing-entry event\n{}",
        qemu.dump()
    );
}
