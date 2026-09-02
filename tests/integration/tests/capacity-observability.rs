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

fn assert_latest_meminfo(output: &str, surface: &str) {
    assert!(
        !output.contains("approx") && !output.contains("not yet wired"),
        "{surface} emitted placeholder memory text\n{output}"
    );

    let row = output
        .lines()
        .rev()
        .find(|line| line.contains("Mem (KiB):"))
        .unwrap_or_else(|| panic!("{surface} did not emit a Mem (KiB) row\n{output}"));
    let values = row
        .split_once("Mem (KiB):")
        .expect("matched row contains its marker")
        .1;
    let mut fields = values.split_whitespace();
    let total = fields
        .next()
        .unwrap_or_else(|| panic!("{surface} row has no total: {row}"))
        .parse::<u64>()
        .unwrap_or_else(|error| panic!("{surface} total is not an integer: {error}: {row}"));
    let used = fields
        .next()
        .unwrap_or_else(|| panic!("{surface} row has no used value: {row}"))
        .parse::<u64>()
        .unwrap_or_else(|error| panic!("{surface} used is not an integer: {error}: {row}"));
    let free = fields
        .next()
        .unwrap_or_else(|| panic!("{surface} row has no free value: {row}"))
        .parse::<u64>()
        .unwrap_or_else(|error| panic!("{surface} free is not an integer: {error}: {row}"));
    assert!(
        fields.next().is_none(),
        "{surface} row has unexpected fields: {row}"
    );
    assert!(total > 0, "{surface} reported zero total memory: {row}");
    let accounted = used
        .checked_add(free)
        .unwrap_or_else(|| panic!("{surface} displayed capacity overflows: {row}"));
    assert!(
        accounted <= total && total - accounted <= 1,
        "{surface} displayed capacity exceeds one KiB of independent rounding: {row}"
    );
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

    let mut qemu =
        QemuRunner::boot_with_fresh_disk(&kernel.to_string_lossy(), &disk.to_string_lossy());
    qemu.wait_for("=== Cellos shell ready", 45)
        .unwrap_or_else(|error| panic!("shell: {error}\n{}", qemu.dump()));
    std::thread::sleep(Duration::from_secs(1));
    let shell_free_checkpoint = qemu.output_checkpoint();
    qemu.send_line("free");
    qemu.wait_for_after("Cellos >", shell_free_checkpoint, 20)
        .unwrap_or_else(|error| panic!("shell free: {error}\n{}", qemu.dump()));
    let shell_free_output = qemu.dump();
    let shell_free_output = shell_free_output
        .get(shell_free_checkpoint..)
        .expect("shell free checkpoint remains a UTF-8 boundary");
    assert_latest_meminfo(shell_free_output, "shell free");

    let standalone_free_checkpoint = qemu.output_checkpoint();
    qemu.send_line("exec /bin/free");
    qemu.wait_for_after("Cellos >", standalone_free_checkpoint, 20)
        .unwrap_or_else(|error| panic!("standalone free: {error}\n{}", qemu.dump()));
    let standalone_free_output = qemu.dump();
    let standalone_free_output = standalone_free_output
        .get(standalone_free_checkpoint..)
        .expect("standalone free checkpoint remains a UTF-8 boundary");
    assert_latest_meminfo(standalone_free_output, "/bin/free");


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
