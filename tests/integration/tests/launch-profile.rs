//! Phase 03 launch-profile integration proof.
//!
//! Manual QEMU proof already passed; this test makes the exact shell-facing
//! behavior durable in the integration harness without mutating the shared disk.

use std::path::PathBuf;
use std::time::{Duration, Instant};
use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;
const CMD_TIMEOUT: u64 = 60;
const PROMPT: &str = "Cellos >";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root()
        .join("disk_v3.img")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(kernel_path()).exists();
    let disk_ok = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_ok {
        eprintln!("SKIP launch-profile: kernel not built ({})", kernel_path());
    }
    if !disk_ok {
        eprintln!("SKIP launch-profile: disk_v3.img missing — run ./gen_disk.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP launch-profile: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_ok && disk_ok && qemu_ok)
}

fn prompt_count(output: &str) -> usize {
    output.matches(PROMPT).count()
}

fn wait_for_prompt_advance(qemu: &QemuRunner, previous_count: usize, timeout_secs: u64) {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    while Instant::now() <= deadline {
        if prompt_count(&qemu.dump()) > previous_count {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "shell prompt did not advance within {timeout_secs}s\n--- output ---\n{}",
        qemu.dump()
    );
}

#[test]
fn shell_launch_profile_allows_vfs_test_and_routes_snapshot_via_supervisor() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot(&kernel_path(), &disk_path());
    qemu.wait_for("[selftest] boot-ceiling: PASS", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("boot selftest not observed: {e}\n{}", qemu.dump()));
    qemu.wait_for("Init: service registry verified.", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("init verification missing: {e}\n{}", qemu.dump()));
    qemu.wait_for(PROMPT, BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell prompt not reached: {e}\n{}", qemu.dump()));

    let prompts_before_vfs = prompt_count(&qemu.dump());
    qemu.send_line("vfs-test");
    qemu.wait_for("[vfs-test] ALL TESTS PASSED", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("vfs-test did not pass: {e}\n{}", qemu.dump()));
    wait_for_prompt_advance(&qemu, prompts_before_vfs, CMD_TIMEOUT);

    let prompts_before_snapshot = prompt_count(&qemu.dump());
    qemu.send_line("snapshot");
    qemu.wait_for("[snapshot] unavailable", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("kernel snapshot unavailability not observed: {e}\n{}", qemu.dump()));
    qemu.wait_for("snapshot: unavailable on this platform", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell snapshot unavailability not observed: {e}\n{}", qemu.dump()));
    wait_for_prompt_advance(&qemu, prompts_before_snapshot, CMD_TIMEOUT);

    let prompts_before_bench = prompt_count(&qemu.dump());
    qemu.send_line("bench snapshot-authority");
    qemu.wait_for("[snapshot] denied", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("kernel snapshot denial not observed: {e}\n{}", qemu.dump()));
    qemu.wait_for("no SupervisorCap", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("kernel denial reason not observed: {e}\n{}", qemu.dump()));
    qemu.wait_for(
        "[snapshot-authority-runtime] PASS (allowlisted bench caller denied: no SupervisorCap)",
        CMD_TIMEOUT,
    )
    .unwrap_or_else(|e| panic!("bench snapshot authority proof not observed: {e}\n{}", qemu.dump()));
    wait_for_prompt_advance(&qemu, prompts_before_bench, CMD_TIMEOUT);

    let output = qemu.dump();
    assert!(
        !output.contains("[snapshot] wrote"),
        "snapshot failure paths must not claim frames were written\n--- output ---\n{output}"
    );
    assert!(
        !output.contains("wrote 18446744073709551615 frames"),
        "snapshot failure paths must not print wrapped usize::MAX success\n--- output ---\n{output}"
    );
}
