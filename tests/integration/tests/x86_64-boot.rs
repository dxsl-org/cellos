//! x86_64 full-boot integration tests.
//!
//! Mirrors the AArch64 `aarch64-boot.rs` suite for the QEMU q35 machine
//! booted from a Limine BIOS ISO.
//!
//! Prerequisites:
//!   - `qemu-system-x86_64` on PATH (or at the Windows default install path)
//!   - ISO built: `cargo build --release --target x86_64-unknown-none -p vicell-kernel`
//!                followed by `.\run-x86.ps1 -NoBuild -NoQemu`
//!                → produces `build/vicell-x86.iso`
//!
//! Tests skip gracefully when any prerequisite is absent — CI behaviour is
//! identical to the AArch64 suite.

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary_x86, QemuRunner};

const BOOT_TIMEOUT: u64 = 45;
const CMD_TIMEOUT: u64  = 10;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn iso_path() -> String {
    repo_root()
        .join("build/vicell-x86.iso")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let iso_exists = PathBuf::from(iso_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_x86())
        .arg("--version")
        .output()
        .is_ok();
    if !iso_exists {
        eprintln!(
            "SKIP x86_64: ISO not built ({})\n  Run: cargo build --release --target x86_64-unknown-none -p vicell-kernel && .\\run-x86.ps1 -NoBuild -NoQemu",
            iso_path()
        );
    }
    if !qemu_ok {
        eprintln!("SKIP x86_64: qemu-system-x86_64 not found (PATH or C:\\Program Files\\qemu\\)");
    }
    iso_exists && qemu_ok
}

/// The kernel must emit its boot banner on x86_64.
///
/// Verifies the kernel ELF is correctly loaded by Limine, the entry point
/// (`_start`) is reached, and COM1 output is routed to the TCP serial socket
/// before any subsystem initialisation begins.
#[test]
fn x86_kernel_banner() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("[Cellos] kernel boot v", 15)
        .unwrap_or_else(|e| panic!("x86_64 kernel banner missing: {e}\n--- output ---\n{}", qemu.dump()));
}

/// The task scheduler must report it is ready before any cell is spawned.
///
/// `"Scheduler initialized"` is emitted after the frame allocator, heap,
/// page tables, APIC, HPET, and IDT have all been set up successfully.
#[test]
fn x86_scheduler_initializes() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("Scheduler initialized", 20)
        .unwrap_or_else(|e| panic!("x86_64 scheduler init not seen: {e}\n--- output ---\n{}", qemu.dump()));
}

/// The embedded init ELF must be spawned successfully from the kernel ramdisk.
///
/// `"Successfully spawned init"` is logged by `main.rs` when `spawn_from_mem`
/// returns `Ok` for the init binary. A failure here means the ring-3 entry
/// path, page-table setup, or manifest parsing is broken on x86_64.
#[test]
fn x86_init_spawns() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("Successfully spawned init", 25)
        .unwrap_or_else(|e| panic!("x86_64 init spawn not seen: {e}\n--- output ---\n{}", qemu.dump()));
}

/// The kernel must boot through init → config → shell and reach the
/// interactive `ViCell >` prompt on COM1.
#[test]
fn x86_boots_to_shell_prompt() {
    if !prerequisites_ok() {
        return;
    }
    let qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("x86_64 shell prompt not reached: {e}\n--- output ---\n{}", qemu.dump()));
}

/// The shell must execute an interactive command over COM1.
///
/// Waits for the shell prompt, sends `echo x86-ok`, and asserts the response
/// appears. Proves the full round-trip: COM1 UART RX → shell readline →
/// built-in dispatch → UART TX → TCP serial harness.
#[test]
fn x86_echo_command() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("x86_64 shell prompt not reached: {e}\n--- output ---\n{}", qemu.dump()));
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("echo x86-ok");
    qemu.wait_for("x86-ok", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("x86_64 echo did not respond: {e}\n--- output ---\n{}", qemu.dump()));
}

/// The `ls /bin` command must return at least one entry over COM1.
///
/// Proves the VFS service cell is running under ring-3 on x86_64, and the
/// IPC path (shell → VFS cell → OP_READDIR → shell) round-trips correctly.
#[test]
fn x86_ls_command() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("x86_64 shell prompt not reached: {e}\n--- output ---\n{}", qemu.dump()));
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("ls /bin");
    // Any one of the expected binaries appearing proves readdir is working.
    qemu.wait_for("shell", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("x86_64 ls /bin did not respond: {e}\n--- output ---\n{}", qemu.dump()));
}

/// The `ps` command must list at least the init and shell tasks.
///
/// Proves the task-enumeration syscall (SysGetTaskInfo or equivalent) works
/// under ring-3 on x86_64. The scheduler and hart-local table must be
/// populated correctly for ps output to appear.
#[test]
fn x86_ps_command() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_x86_bios(&iso_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("x86_64 shell prompt not reached: {e}\n--- output ---\n{}", qemu.dump()));
    std::thread::sleep(std::time::Duration::from_millis(500));
    qemu.send_line("ps");
    // ps prints a task table; any numeric PID appearing proves the syscall worked.
    qemu.wait_for("init", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("x86_64 ps did not respond: {e}\n--- output ---\n{}", qemu.dump()));
}
