//! NIC device state shared by both virtio transports.
//!
//! The transport is chosen per architecture: virtio-MMIO on riscv64/aarch64
//! (`crate::mmio`) and virtio-over-PCI on x86_64 (`crate::pci_x86`). Both
//! produce a [`NetDevice`] whose Tx/Rx/MAC surface is identical, so the
//! dispatcher and the net service never see which transport is underneath.

extern crate alloc;

#[cfg(not(target_arch = "x86_64"))]
pub use crate::mmio::find_and_init_net;
#[cfg(target_arch = "x86_64")]
pub use crate::pci_x86::find_and_init_net;

/// RX/TX virtqueue depth. Power-of-two; QEMU supports up to 1024.
pub(crate) const NET_QUEUE_SIZE: usize = 16;

/// RX buffer length per slot. Must be ≥ 1526 (MAX Ethernet + VirtioNetHdr).
pub(crate) const RX_BUF_LEN: usize = 2048;

#[cfg(not(target_arch = "x86_64"))]
pub(crate) type CellNet = virtio_drivers::device::net::VirtIONet<
    crate::mmio::CellHal,
    virtio_drivers::transport::mmio::MmioTransport,
    NET_QUEUE_SIZE,
>;
#[cfg(target_arch = "x86_64")]
pub(crate) type CellNet = crate::pci_x86::PciNet;

/// Runtime state for the active VirtIO NIC.
pub struct NetDevice {
    pub(crate) net: CellNet,
    // reason: consumed by wait_recv()'s IRQ-wake path (crate::mmio); the cell's
    // current main loop uses try_recv() polling instead, so both this field
    // pair and wait_recv() are dormant until the IRQ-driven path is wired in.
    #[cfg(not(target_arch = "x86_64"))]
    #[allow(dead_code)]
    pub irq: u8,
    #[cfg(not(target_arch = "x86_64"))]
    #[allow(dead_code)]
    pub mmio_base: usize,
}

impl NetDevice {
    /// Get the MAC address of this NIC.
    pub fn mac(&self) -> [u8; 6] {
        self.net.mac_address()
    }

    /// Transmit `frame`. Returns `true` on success.
    #[cfg(not(target_arch = "x86_64"))]
    pub fn send(&mut self, frame: &[u8]) -> bool {
        let mut tx = self.net.new_tx_buffer(frame.len());
        tx.packet_mut().copy_from_slice(frame);
        self.net.send(tx).is_ok()
    }

    /// Try to receive one frame into `buf`. Returns byte count (0 = nothing ready).
    #[cfg(not(target_arch = "x86_64"))]
    pub fn try_recv(&mut self, buf: &mut [u8]) -> usize {
        match self.net.receive() {
            Ok(rx) => {
                let len = rx.packet_len().min(buf.len());
                buf[..len].copy_from_slice(&rx.packet()[..len]);
                if let Err(e) = self.net.recycle_rx_buffer(rx) {
                    // Non-fatal: log and continue; packet is already in buf.
                    let _ = e;
                }
                len
            }
            Err(_) => 0,
        }
    }

    /// Transmit `frame` (virtio 1.x header framing). Returns `true` on success.
    #[cfg(target_arch = "x86_64")]
    pub fn send(&mut self, frame: &[u8]) -> bool {
        self.net.send(frame)
    }

    /// Try to receive one frame into `buf`. Returns byte count (0 = nothing ready).
    ///
    /// Never blocks: this is the only receive path on x86_64, where the device
    /// runs with interrupts disabled and the net service polls through Rx
    /// requests.
    #[cfg(target_arch = "x86_64")]
    pub fn try_recv(&mut self, buf: &mut [u8]) -> usize {
        self.net.try_recv(buf)
    }
}
