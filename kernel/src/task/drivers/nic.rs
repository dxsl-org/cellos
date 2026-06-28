//! NIC selector — routes TX/RX to the available NIC driver.
//!
//! VirtIO NIC (QEMU MMIO) is now the virtio-net Driver Cell (`cells/drivers/virtio-net/`).
//! The net service locates the Cell via `sys_lookup_service(service::NIC_DRIVER)` and
//! routes all frames via IPC to it.  The kernel-path functions below are retained so
//! syscall.rs compiles, but they return "not available" — the Cell is always the active
//! provider on RISC-V and ARM64.  x86_64 uses the e1000 Driver Cell.

/// Send an Ethernet frame. Returns `true` on success.
///
/// Always returns `false`: VirtIO net is a Driver Cell; kernel-resident NIC path removed.
pub fn send_frame(_frame: &[u8]) -> bool {
    false
}

/// Receive one Ethernet frame into `buf`. Returns byte count (0 = no frame ready).
///
/// Always returns `0`: VirtIO net is a Driver Cell; kernel-resident NIC path removed.
pub fn recv_frame(_buf: &mut [u8]) -> usize {
    0
}
