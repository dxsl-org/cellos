//! Ring-3 smoke test — boots QEMU and verifies the kernel reaches user mode.
//!
//! Requires `qemu-system-riscv64` on PATH and a built kernel binary at
//! `target/riscv64gc-unknown-none-elf/release/kernel`.
//!
//! Run via the helper script:
//!   scripts/qemu-boot-test.sh
//!
//! Or when wired into a host-target test crate:
//!   cargo test --test ring3_smoke --target x86_64-pc-windows-msvc

use super::harness::QemuRunner;

const KERNEL_BIN: &str = "target/riscv64gc-unknown-none-elf/release/kernel";
const BOOT_TIMEOUT_SECS: u64 = 30;

/// Kernel must print its banner within BOOT_TIMEOUT_SECS.
pub fn test_kernel_boots() {
    let mut qemu = QemuRunner::new_rv64(KERNEL_BIN);
    qemu.wait_for("[ViOS]", BOOT_TIMEOUT_SECS)
        .expect("kernel banner not seen — kernel may have panicked or QEMU is not on PATH");
}

/// After the banner, the kernel must reach U-mode and print the hello message.
pub fn test_ring3_hello_visible() {
    let mut qemu = QemuRunner::new_rv64(KERNEL_BIN);

    // Wait for banner first.
    qemu.wait_for("[ViOS]", BOOT_TIMEOUT_SECS)
        .expect("kernel banner not seen");

    // Then confirm U-mode execution reached the hello task.
    qemu.wait_for("Hi from U-mode", BOOT_TIMEOUT_SECS)
        .expect("Ring-3 user_hello task not seen");
}

/// Shell prompt must appear after cells are spawned from /bin/.
pub fn test_shell_prompt_visible() {
    let mut qemu = QemuRunner::new_rv64(KERNEL_BIN);

    qemu.wait_for("[ViOS]", BOOT_TIMEOUT_SECS)
        .expect("kernel banner not seen");

    // Shell prompt — the exact string depends on cells/apps/shell configuration.
    qemu.wait_for("ViOS>", BOOT_TIMEOUT_SECS)
        .expect("shell prompt not seen — init or vfs Cell may have crashed");
}

/// No panic messages should appear in the boot log.
pub fn test_no_kernel_panics() {
    let mut qemu = QemuRunner::new_rv64(KERNEL_BIN);

    // Drive boot to completion; collect output for inspection.
    let _ = qemu.wait_for("ViOS>", BOOT_TIMEOUT_SECS);

    // A kernel panic prints "PANIC" (uppercase) to the serial console.
    assert!(
        !qemu.output_contains("PANIC"),
        "kernel panic detected in serial output"
    );
}
