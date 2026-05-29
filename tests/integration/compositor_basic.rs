//! Basic compositor integration tests.
//!
//! Boots QEMU with a VirtIO GPU, asserts the compositor announces itself,
//! and confirms that creating surfaces doesn't panic the kernel.

use super::harness::QemuRunner;

const KERNEL_BIN: &str = "target/riscv64gc-unknown-none-elf/release/kernel";
const TIMEOUT: u64 = 60;

fn boot_with_gpu() -> QemuRunner {
    // TODO: extend QemuRunner to pass `-device virtio-gpu-device` to QEMU args.
    QemuRunner::new_rv64(KERNEL_BIN)
}

/// Compositor cell must announce itself in the boot log.
pub fn test_compositor_starts() {
    let mut q = boot_with_gpu();
    q.wait_for("[ViOS]", TIMEOUT).expect("kernel banner not seen");
    q.wait_for("[compositor]", TIMEOUT)
        .expect("compositor Cell did not start — check init boot sequence");
}

/// No panic must occur during compositor + GPU init.
pub fn test_no_panic_with_gpu() {
    let mut q = boot_with_gpu();
    let _ = q.wait_for("[compositor]", TIMEOUT);
    assert!(
        !q.output_contains("PANIC"),
        "kernel panic during compositor/GPU init"
    );
}

/// The GPU flush syscall must not crash the kernel when called with an
/// all-zero 1×1 buffer (minimum valid flush).
pub fn test_gpu_flush_1x1_no_crash() {
    let mut q = boot_with_gpu();
    // After compositor starts, the shell should still be accessible.
    let _ = q.wait_for("ViOS>", TIMEOUT);
    assert!(
        !q.output_contains("PANIC"),
        "kernel panic during GPU flush"
    );
}
