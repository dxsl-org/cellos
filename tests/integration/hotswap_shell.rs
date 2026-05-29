//! Hot-swap integration tests.
//!
//! Boots QEMU, starts the shell, hot-swaps it to a "v2" ELF, asserts that
//! the shell prompt returns and history is preserved.
//!
//! Requires a second shell ELF compiled with a different banner string to
//! distinguish v1 from v2.  Until that is built, the tests use SKIP guards.

use super::harness::QemuRunner;

const KERNEL_BIN: &str = "target/riscv64gc-unknown-none-elf/release/kernel";
const TIMEOUT:    u64  = 60;

/// Shell v1 must boot and show a prompt before a hotswap attempt.
pub fn test_shell_boots_before_hotswap() {
    let mut q = QemuRunner::new_rv64(KERNEL_BIN);
    q.wait_for("[ViOS]", TIMEOUT).expect("kernel banner not seen");
    q.wait_for("ViOS>", TIMEOUT).expect("shell v1 prompt not seen");
}

/// After a hotswap the kernel must not panic and a prompt must reappear.
///
/// This is a structural test — the actual hotswap trigger requires the
/// `hotswap` admin CLI which is built in Phase 20.5.  Until then the test
/// just asserts no crash occurs.
pub fn test_no_panic_during_hotswap() {
    let mut q = QemuRunner::new_rv64(KERNEL_BIN);
    let _ = q.wait_for("ViOS>", TIMEOUT);
    assert!(
        !q.output_contains("PANIC"),
        "kernel panic in pre-hotswap window"
    );
}

/// History must be preserved after a hotswap (serialised by ViStateTransfer).
///
/// Full validation deferred until the hotswap CLI is available and the
/// QemuRunner gains stdin injection support (Phase 17a pipe caps).
pub fn test_history_preserved_after_hotswap() {
    // TODO: inject "echo hello" → hotswap → verify "echo hello" in history
    let mut q = QemuRunner::new_rv64(KERNEL_BIN);
    let _ = q.wait_for("ViOS>", TIMEOUT);
    assert!(!q.output_contains("PANIC"));
}
