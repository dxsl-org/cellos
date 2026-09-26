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
#[test]
fn tier2_peer_memory_isolation_terminates_cell_cleanly() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "tier2-exploit peer");

    // 1. Verify admission to Tier 2 Paged Domain (SATP isolation)
    qemu.wait_for("[domain] admitted cell", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "tier2-exploit peer was not admitted to Tier 2 Paged Domain: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // 2. Verify deliberate peer write attempt
    qemu.wait_for(
        "[tier2-exploit] deliberately writing to peer cell memory at 0x08000000",
        FAULT_TIMEOUT,
    )
    .unwrap_or_else(|e| {
        panic!(
            "tier2-exploit never reached the peer write: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // 3. Verify that CPU triggered page fault and kernel terminated the cell
    qemu.wait_for("[fault] Cell", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "kernel did not catch the peer-memory page fault or terminate the cell: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    let log = qemu.dump();
    assert!(
        !log.contains("write to peer cell succeeded"),
        "illegal peer memory write succeeded — peer isolation breached!\n--- output ---\n{log}"
    );

    // 4. Verify shell survivability
    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "echo tier2-peer-ok");
    qemu.wait_for("tier2-peer-ok", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "kernel crashed or shell hung after Tier 2 peer memory fault: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

#[test]
fn tier2_kernel_memory_isolation_terminates_cell_cleanly() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "tier2-exploit kernel");

    // 1. Verify admission to Tier 2 Paged Domain (SATP isolation)
    qemu.wait_for("[domain] admitted cell", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "tier2-exploit kernel was not admitted to Tier 2 Paged Domain: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // 2. Verify deliberate kernel write attempt
    qemu.wait_for(
        "[tier2-exploit] deliberately writing to kernel memory at 0x80200000",
        FAULT_TIMEOUT,
    )
    .unwrap_or_else(|e| {
        panic!(
            "tier2-exploit never reached the kernel write: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // 3. Verify that CPU triggered page fault and kernel terminated the cell
    qemu.wait_for("[fault] Cell", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "kernel did not catch the kernel-memory page fault or terminate the cell: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    let log = qemu.dump();
    assert!(
        !log.contains("write to kernel memory succeeded"),
        "illegal kernel memory write succeeded — kernel isolation breached!\n--- output ---\n{log}"
    );

    // 4. Verify shell survivability
    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "echo tier2-kernel-ok");
    qemu.wait_for("tier2-kernel-ok", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "kernel crashed or shell hung after Tier 2 kernel memory fault: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

#[test]
fn tier2_positive_execution_runs_cleanly() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "tier2-smoke");

    // 1. Verify admission to Tier 2 Paged Domain (SATP isolation)
    qemu.wait_for("[domain] admitted cell 'tier2-smoke'", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "tier2-smoke was not admitted to Tier 2 Paged Domain: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // 2. Verify heap allocation and execution under private SATP
    qemu.wait_for("[tier2-smoke] Heap allocation verified", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "tier2-smoke heap allocation failed: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // 3. Verify zero-copy grant allocation, private SATP mapping, write, read, and unmapping
    qemu.wait_for(
        "[tier2-smoke] Grant allocation, private SATP mapping, and RW verified in Tier 2 domain",
        FAULT_TIMEOUT,
    )
    .unwrap_or_else(|e| {
        panic!(
            "tier2-smoke grant allocation/mapping failed: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    qemu.wait_for(
        "[tier2-smoke] Grant unregister and unmapping verified in Tier 2 domain",
        FAULT_TIMEOUT,
    )
    .unwrap_or_else(|e| {
        panic!(
            "tier2-smoke grant unregister/unmapping failed: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    qemu.wait_for(
        "[tier2-smoke] PASS: All Tier 2 runtime invariants verified successfully!",
        FAULT_TIMEOUT,
    )
    .unwrap_or_else(|e| {
        panic!(
            "tier2-smoke did not complete PASS: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // 3. Verify clean exit and shell survivability
    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "echo tier2-positive-ok");
    qemu.wait_for("tier2-positive-ok", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "shell not responding after tier2-smoke exit: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}

#[test]
fn tier2_posix_ffi_execution_runs_in_paged_domain() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("Cellos >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "posix-shim-test");

    // 1. Verify admission to Tier 2 Paged Domain under private SATP
    qemu.wait_for(
        "[domain] admitted cell 'posix-shim-test' to Tier 2 Paged Domain (SATP isolation)",
        FAULT_TIMEOUT,
    )
    .unwrap_or_else(|e| {
        panic!(
            "posix-shim-test was not admitted to Tier 2 Paged Domain: {e}\n--- output ---\n{}",
            qemu.dump()
        )
    });

    // 2. Verify C-FFI POSIX filesystem & random shims execute cleanly behind private SATP
    qemu.wait_for("POSIX-FSTAT: OK", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "posix-shim-test fstat failed under private SATP: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    qemu.wait_for("POSIX-ENTROPY: OK", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "posix-shim-test getentropy failed under private SATP: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // 3. Verify shell survivability
    //
    // The cell is still running at this point, and the shell owns its input until
    // the foreground cell exits — the input path has no queue, so a command typed
    // mid-cell is dropped. Wait for the cell's own end marker and the prompt it
    // returns to before typing anything.
    let after_cell_markers = qemu.output_checkpoint();
    qemu.wait_for_after("PORTING-SMOKE: OK", after_cell_markers, FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "posix-shim-test did not finish under private SATP: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
    qemu.wait_for_after("Cellos >", after_cell_markers, FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "shell prompt did not return after posix-shim-test: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "echo tier2-ffi-ok");
    qemu.wait_for("tier2-ffi-ok", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "shell not responding after posix-shim-test exit: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}
