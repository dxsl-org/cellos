//! W^X gate: cell pages lose WRITE after relocation.
//!
//! Invariant under test: once the loader has applied `.rela.dyn` it lowers every
//! cell page to the ELF's real `p_flags`, so a cell can no longer write its own
//! `.text` — and by extension no cell can rewrite another cell's code in the
//! single address space Cellos runs them all in.
//!
//! The `wx-test` cell (`cells/tests/wx-test`) stores one byte into its own
//! `.text` and prints a verdict line only on the path where the store SUCCEEDS.
//! The kernel is the real oracle here, so this harness reads the kernel log:
//!
//!   PASS — `[fault] Cell … terminated` appears AND the shell prompt comes back,
//!          proving the cell died cleanly through the existing fault-report path
//!          and the kernel survived.
//!   FAIL — `wx-test: FAIL` appears: the page was still writable.
//!   FAIL — no `[fault]` line and no prompt: the kernel panicked instead of
//!          terminating just the cell (the PR #15 lesson this phase must not
//!          regress).
//!
//! # Prerequisites
//! - `qemu-system-riscv64` on PATH
//! - `RUSTFLAGS="-C relocation-model=pic" cargo build --release -p vicell-kernel`
//! - `cargo build --release -p wx-test`
//! - `./gen_disk.ps1` (installs `/bin/wx-test` on disk_v3.img)

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 40;
const FAULT_TIMEOUT: u64 = 25;

/// Kernel log prefix emitted by `task::terminate_current_cell_on_fault`.
/// Load-bearing string — `scripts/qemu-hypervisor-smoke.sh` greps for it too.
const FAULT_MARKER: &str = "[fault] Cell";
/// Printed by the cell immediately BEFORE the illegal store, so its presence
/// distinguishes "the cell never ran" from "the cell ran and was stopped".
const ATTEMPT_MARKER: &str = "wx-test: storing to .text now";
/// Printed by the cell only if the store completed — i.e. W^X did not hold.
const VIOLATION_MARKER: &str = "wx-test: FAIL";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/vicell-kernel")
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
        eprintln!("SKIP wx-text-write: kernel not built ({})", kernel_path());
    }
    if !disk_ok {
        eprintln!("SKIP wx-text-write: disk_v3.img missing — run ./gen_disk.ps1");
    }
    if !qemu_ok {
        eprintln!("SKIP wx-text-write: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_ok && disk_ok && qemu_ok)
}

/// Send a shell command one byte at a time.
///
/// The guest UART FIFO drops characters past 16 bytes, so every other suite in
/// this tree paces input the same way; `wx-test` is 7 bytes but the pacing also
/// gives the shell time to echo.
fn send_command(qemu: &mut QemuRunner, cmd: &str) {
    for b in cmd.as_bytes() {
        qemu.send_bytes(&[*b]);
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    qemu.send_bytes(b"\n");
}

/// A cell writing its own `.text` faults, is terminated, and the kernel lives.
#[test]
fn text_write_faults_and_terminates_cell() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "wx-test");

    // The cell must at least reach the store; otherwise a spawn failure would
    // masquerade as a pass (no violation line, no fault line).
    qemu.wait_for(ATTEMPT_MARKER, FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "wx-test never attempted the .text store: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    // The store must trap and route through the cell fault-report path.
    qemu.wait_for(FAULT_MARKER, FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "no '{FAULT_MARKER}' after the .text store — W^X not enforced, or the kernel \
                 panicked instead of terminating the cell: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });

    let log = qemu.dump();
    assert!(
        !log.contains(VIOLATION_MARKER),
        "cell reported a successful .text write — cell pages are still mapped WRITE after \
         relocation\n--- output ---\n{log}"
    );
}

/// The kernel keeps scheduling after the cell dies — the fault killed one cell,
/// not the system. Separated from the assertion above so a regression tells you
/// WHICH half broke: enforcement, or survivability.
#[test]
fn kernel_survives_the_faulting_cell() {
    if !prerequisites_ok() {
        return;
    }

    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));
    send_command(&mut qemu, "wx-test");
    qemu.wait_for(FAULT_MARKER, FAULT_TIMEOUT)
        .unwrap_or_else(|e| panic!("cell did not fault: {e}\n{}", qemu.dump()));

    // A returning prompt is the cheapest proof the scheduler still runs: a
    // panicking kernel parks in the panic handler and never echoes again.
    send_command(&mut qemu, "echo wx-alive");
    qemu.wait_for("wx-alive", FAULT_TIMEOUT)
        .unwrap_or_else(|e| {
            panic!(
                "shell unresponsive after the W^X fault — the kernel did not survive \
                 terminating the cell: {e}\n--- output ---\n{}",
                qemu.dump()
            )
        });
}
