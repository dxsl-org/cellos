//! Multi-cell boot chain test — verifies Init → Config → VFS → Shell sequence.
//!
//! Each test drives QEMU and asserts that specific log lines appear, confirming
//! the expected cell was spawned and initialised successfully.

use super::harness::QemuRunner;

const KERNEL_BIN: &str = "target/riscv64gc-unknown-none-elf/release/kernel";
const TIMEOUT: u64 = 45;

/// Init cell must announce itself before spawning other cells.
pub fn test_init_cell_starts() {
    let mut q = QemuRunner::new_rv64(KERNEL_BIN);
    q.wait_for("[init]", TIMEOUT)
        .expect("init Cell did not start");
}

/// Config cell must be spawned by init and announce itself.
pub fn test_config_cell_spawned() {
    let mut q = QemuRunner::new_rv64(KERNEL_BIN);
    q.wait_for("[init]", TIMEOUT).expect("init Cell not seen");
    q.wait_for("[config]", TIMEOUT)
        .expect("config Cell not spawned — check init's SpawnFromPath call");
}

/// VFS cell must come up after config.
pub fn test_vfs_cell_spawned() {
    let mut q = QemuRunner::new_rv64(KERNEL_BIN);
    q.wait_for("[init]", TIMEOUT).expect("init Cell not seen");
    q.wait_for("VFS Service", TIMEOUT)
        .expect("VFS Cell not seen — check init's SpawnFromPath call for /bin/vfs");
}

/// Shell cell must come up last and print the prompt.
pub fn test_shell_cell_spawned() {
    let mut q = QemuRunner::new_rv64(KERNEL_BIN);
    q.wait_for("VFS Service", TIMEOUT).expect("VFS Cell not seen");
    q.wait_for("ViOS>", TIMEOUT)
        .expect("shell prompt not seen — shell Cell did not start");
}

/// The full boot chain must complete within the timeout with no panic.
pub fn test_full_boot_chain_no_panic() {
    let mut q = QemuRunner::new_rv64(KERNEL_BIN);

    let result = q.wait_for("ViOS>", TIMEOUT);
    assert!(
        !q.output_contains("PANIC"),
        "kernel panic during multi-cell boot"
    );
    result.expect("boot chain did not complete — shell prompt missing");
}
