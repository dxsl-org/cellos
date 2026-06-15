//! NIC selector — routes TX/RX to the highest-priority available NIC driver.
//!
//! Priority: e1000 (PCIe) > VirtIO. Mirrors the `block::block_device()` pattern.

/// Send an Ethernet frame. Returns `true` on success.
pub fn send_frame(frame: &[u8]) -> bool {
    if super::nic_e1000::is_present() {
        super::nic_e1000::send_frame(frame)
    } else {
        super::virtio_net::send_frame(frame)
    }
}

/// Receive one Ethernet frame into `buf`. Returns byte count (0 = no frame ready).
pub fn recv_frame(buf: &mut [u8]) -> usize {
    if super::nic_e1000::is_present() {
        super::nic_e1000::recv_frame(buf)
    } else {
        super::virtio_net::recv_frame(buf)
    }
}
