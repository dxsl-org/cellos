//! NIC selector — routes TX/RX to the available kernel NIC driver.
//!
//! Note: PCIe e1000 is now a Driver Cell (`cells/drivers/e1000/`), not a
//! kernel driver. The net service cell acquires it via IPC to that Driver Cell.
//! This selector now only covers VirtIO (QEMU para-virtual transport).

/// Send an Ethernet frame. Returns `true` on success.
pub fn send_frame(frame: &[u8]) -> bool {
    super::virtio_net::send_frame(frame)
}

/// Receive one Ethernet frame into `buf`. Returns byte count (0 = no frame ready).
pub fn recv_frame(buf: &mut [u8]) -> usize {
    super::virtio_net::recv_frame(buf)
}
