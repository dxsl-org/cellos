//! End-to-end gates for typed spawn OOM and opt-in MemInfo telemetry.

use std::path::PathBuf;
use std::time::Duration;
use vicell_integration_tests::{qemu_binary, QemuRunner};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

#[test]
fn meminfo_denial_and_typed_spawn_oom_are_runtime_visible() {
    let root = repo_root();
    let kernel = root.join("target/riscv64gc-unknown-none-elf/release/cellos-kernel");
    let disk = root.join("disk_v3.img");
    assert!(
        kernel.exists(),
        "RV64 kernel missing; build the test image with CELLOS_INCLUDE_CAPACITY_PROBE=1"
    );
    assert!(
        disk.exists(),
        "disk image missing; build it with CELLOS_INCLUDE_CAPACITY_PROBE=1"
    );
    let qemu_version = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .expect("qemu-system-riscv64 must be available for this runtime gate");
    assert!(qemu_version.status.success(), "QEMU version probe failed");

    let mut qemu = QemuRunner::boot_with_fresh_disk(
        &kernel.to_string_lossy(),
        &disk.to_string_lossy(),
    );
    qemu.wait_for("=== Cellos shell ready", 45)
        .unwrap_or_else(|error| panic!("shell: {error}\n{}", qemu.dump()));
    std::thread::sleep(Duration::from_secs(1));

    qemu.send_line("capacity-probe");
    qemu.wait_for("[a2a3-probe] MEMINFO_DENIED", 30)
        .unwrap_or_else(|error| panic!("MemInfo denial: {error}\n{}", qemu.dump()));
    assert!(
        qemu.output_contains("syscall MemInfo (bit 56) denied"),
        "kernel did not log the bit-56 denial\n{}",
        qemu.dump()
    );
    qemu.wait_for("[a2a3-probe] OOM_TYPED", 180)
        .unwrap_or_else(|error| panic!("spawn OOM: {error}\n{}", qemu.dump()));
    assert!(
        qemu.output_contains("spawn OOM: op=SpawnPinned"),
        "kernel did not log caller/path OOM context\n{}",
        qemu.dump()
    );
    assert!(
        qemu.output_contains("Stack alloc failed")
            || qemu.output_contains("segment frame allocation failed")
            || qemu.output_contains("segment page-table allocation failed"),
        "kernel did not log the failed allocation source\n{}",
        qemu.dump()
    );
    assert!(!qemu.output_contains("KERNEL PANIC"), "{}", qemu.dump());

    std::thread::sleep(Duration::from_millis(500));
    qemu.send_line("echo A2A3_SHELL_OK_AFTER_OOM");
    qemu.wait_for("USER: A2A3_SHELL_OK_AFTER_OOM", 20)
        .unwrap_or_else(|error| panic!("shell recovery: {error}\n{}", qemu.dump()));
}
