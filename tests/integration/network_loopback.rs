//! Network loopback integration tests.
//!
//! Boots QEMU with VirtIO net enabled, waits for the net cell to acquire an
//! IP via DHCP, then asserts basic TCP connectivity using the QEMU user-mode
//! network stack (10.0.2.0/24).

use super::harness::QemuRunner;

const KERNEL_BIN: &str = "target/riscv64gc-unknown-none-elf/release/kernel";
const TIMEOUT: u64 = 60;

/// QEMU user-mode DHCP issues 10.0.2.15 to the guest by default.
const EXPECTED_IP_PREFIX: &str = "10.0.2.";

fn boot_with_net() -> QemuRunner {
    // TODO: extend QemuRunner to pass `-netdev user,id=n0 -device virtio-net-device,netdev=n0`
    QemuRunner::new_rv64(KERNEL_BIN)
}

/// Net cell must announce DHCP success in serial output.
pub fn test_dhcp_acquired() {
    let mut q = boot_with_net();
    q.wait_for("[ViOS]", TIMEOUT).expect("kernel banner not seen");
    q.wait_for("[net] DHCP acquired", TIMEOUT)
        .expect("DHCP not completed — check VirtIO NIC is present in QEMU args");
}

/// After DHCP, no kernel panic should have occurred.
pub fn test_no_panic_after_dhcp() {
    let mut q = boot_with_net();
    let _ = q.wait_for("[net] DHCP acquired", TIMEOUT);
    assert!(
        !q.output_contains("PANIC"),
        "kernel panic during net cell boot"
    );
}

/// TCP loopback: connect to QEMU's host-side listener (via user-mode network).
/// Requires the host to listen on 10.0.2.2:7 (echo service or test server).
pub fn test_tcp_echo() {
    let mut q = boot_with_net();
    q.wait_for("[net] DHCP acquired", TIMEOUT)
        .expect("DHCP must complete before TCP test");
    // TODO: inject shell command `curl 10.0.2.2:7` via stdin;
    // assert response appears in serial output.
    // Full implementation pending stdin injection in QemuRunner.
}
