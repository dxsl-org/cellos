//! Tier 2 Native Domain hardware fault isolation test.
//!
//! Proves that when a cell runs in Tier 2 (private SATP domain):
//! 1. The cell is admitted to Tier 2 Paged Domain.
//! 2. When the cell deliberately triggers a hardware fault (e.g. NULL pointer write),
//!    the CPU generates a Page Fault.
//! 3. The kernel catches the fault, attributes it to the faulting cell, terminates
//!    the cell cleanly, and the kernel and other cells keep running.

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 40;
const FAULT_TIMEOUT: u64 = 25;

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
    let p = repo_root().join("disk_v3.img");
    if p.exists() {
        p.to_string_lossy().into_owned()
    } else {
        repo_root().join("bench-disk.img").to_string_lossy().into_owned()
    }
}

fn prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(kernel_path()).exists();
    let disk_ok = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    vicell_integration_tests::ci_guard(kernel_ok && disk_ok && qemu_ok)
}

fn send_command(qemu: &mut QemuRunner, cmd: &str) {
    for b in cmd.as_bytes() {
        qemu.send_bytes(&[*b]);
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    qemu.send_bytes(b"\n");
}

#[test]
fn tier2_hardware_page_fault_terminates_cell_cleanly() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "tier2-exploit");

    // 1. Verify that the cell was admitted to Tier 2 Paged Domain (SATP isolation)
    qemu.wait_for("[domain] admitted cell", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "tier2-exploit was not admitted to Tier 2 Paged Domain: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // 2. Verify that the cell started executing and attempted the fault
    qemu.wait_for("[tier2-exploit] deliberately writing to NULL", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "tier2-exploit never reached the NULL write: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // 3. Verify that the CPU triggered a fault and the kernel terminated the cell
    qemu.wait_for("[fault] Cell", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "kernel did not catch the page fault or terminate the cell: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    let log = qemu.dump();
    assert!(
        !log.contains("write to NULL succeeded"),
        "illegal NULL write succeeded — SATP hardware isolation was NOT active!\n--- output ---\n{log}"
    );

    // 4. Verify kernel survivability: shell prompt returns and responds to subsequent commands
    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "echo tier2-alive");
    qemu.wait_for("tier2-alive", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "kernel crashed or shell hung after Tier 2 fault: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}
