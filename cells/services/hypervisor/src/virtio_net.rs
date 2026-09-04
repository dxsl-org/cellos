//! Shared VirtIO network device model.
//!
//! ARM uses VirtIO-MMIO slot 2/SPI18; x86 uses slot 1/IRQ6. TX forwards L2
//! frames to the Net Cell. RX writes one guest buffer and reports completion so
//! the transport owner can set its ISR bit and retry interrupt delivery.
//! The 12-byte modern virtio_net_hdr_v1 is prepended on RX and stripped on TX.

extern crate alloc;
use crate::virtio_mmio::{QueueCfg, VirtioDevice, VirtioMmio};
use crate::virtqueue::{process_notify, read_descriptor_chain, DescBuf};

/// MAC address presented to the guest virtio-net device.
pub const GUEST_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0xAA, 0xBB, 0xCC];

/// virtio-net feature bit: device provides a MAC address in config space.
const VIRTIO_NET_F_MAC: u32 = 1 << 5;
const VIRTIO_NET_HDR_V1_LEN: usize = 12;
pub struct NetDev {
    /// Supervised Net Cell connection and recovery state.
    pub backend: crate::net_backend::Connection,
    rx_last_avail: u16,
    rx_used_idx: u16,
    tx_last_avail: u16,
    tx_used_idx: u16,
    tx_completion_logged: bool,
    irq: Option<u32>,
}

impl NetDev {
    pub fn new(net_tid: usize, irq: Option<u32>) -> Self {
        Self {
            backend: crate::net_backend::Connection::new(net_tid),
            rx_last_avail: 0,
            rx_used_idx: 0,
            tx_last_avail: 0,
            tx_used_idx: 0,
            tx_completion_logged: false,
            irq,
        }
    }

    /// Inject one received Ethernet frame into the guest RX virtqueue.
    ///
    /// Prepends a 12-byte `virtio_net_hdr_v1`, fills one available descriptor
    /// chain, and advances the used ring. `num_buffers` is one because this
    /// device does not negotiate `VIRTIO_NET_F_MRG_RXBUF`.
    /// Returns true only when it publishes a used entry; the caller owns ISR signaling.
    /// An optional device IRQ supports ARM; x86 retries its pending ISR centrally.
    pub fn push_rx_frame(
        &mut self,
        frame: &[u8],
        vm_id: usize,
        vcpu_id: usize,
        net_vmio: &VirtioMmio,
    ) -> bool {
        let qcfg = net_vmio.queue_cfg(0);
        let q_size = qcfg.num as usize;
        if !qcfg.ready || !qcfg.is_valid() {
            return false;
        }

        let Some(avail_idx_gpa) = crate::virtqueue_guard::checked_gpa(qcfg.avail_gpa, 2, 2) else {
            return false;
        };
        let mut b2 = [0u8; 2];
        if crate::vmm::read_guest_memory(vm_id, avail_idx_gpa, &mut b2) != 2 {
            return false;
        }
        let avail_idx = u16::from_le_bytes(b2);
        match crate::virtqueue_guard::pending_count(q_size, self.rx_last_avail, avail_idx) {
            Some(0) => return false,
            Some(_) => {}
            None => {
                ostd::io::println("[hv-virtio-host] reject pending-delta");
                return false;
            }
        }

        let Some(ring_gpa) =
            crate::virtqueue_guard::avail_entry_gpa(qcfg.avail_gpa, self.rx_last_avail, q_size)
        else {
            return false;
        };
        if crate::vmm::read_guest_memory(vm_id, ring_gpa, &mut b2) != 2 {
            return false;
        }
        let head = u16::from_le_bytes(b2) as usize;
        let Some(bufs) = read_descriptor_chain(vm_id, &qcfg, head) else {
            ostd::io::println("[hv-virtio-host] reject descriptor-chain");
            return false;
        };
        if bufs.iter().any(|buf| !buf.writable) {
            ostd::io::println("[hv-virtio-host] reject descriptor-chain");
            return false;
        }

        let Some(payload_len) = VIRTIO_NET_HDR_V1_LEN.checked_add(frame.len()) else {
            return false;
        };
        if payload_len > u32::MAX as usize {
            return false;
        }
        let Some(capacity) = bufs
            .iter()
            .try_fold(0usize, |sum, buf| sum.checked_add(buf.len as usize))
        else {
            return false;
        };
        if capacity < payload_len {
            return false;
        }
        let mut payload = alloc::vec::Vec::new();
        if payload.try_reserve_exact(payload_len).is_err() {
            return false;
        }
        payload.resize(payload_len, 0);
        payload[10..12].copy_from_slice(&1u16.to_le_bytes());
        payload[VIRTIO_NET_HDR_V1_LEN..].copy_from_slice(frame);

        let mut pos = 0usize;
        for buf in &bufs {
            let n = payload.len().saturating_sub(pos).min(buf.len as usize);
            if n == 0 {
                continue;
            }
            let Some(gpa) = crate::virtqueue_guard::checked_gpa(buf.gpa, 0, n as u64) else {
                return false;
            };
            if crate::vmm::write_guest_memory(vm_id, gpa, &payload[pos..pos + n]) != n {
                return false;
            }
            pos += n;
        }

        let Some(elem_gpa) =
            crate::virtqueue_guard::used_entry_gpa(qcfg.used_gpa, self.rx_used_idx, q_size)
        else {
            return false;
        };
        let next_used = self.rx_used_idx.wrapping_add(1);
        let Some(used_idx_gpa) = crate::virtqueue_guard::checked_gpa(qcfg.used_gpa, 2, 2) else {
            return false;
        };
        let mut elem = [0u8; 8];
        elem[0..4].copy_from_slice(&(head as u32).to_le_bytes());
        elem[4..8].copy_from_slice(&(pos as u32).to_le_bytes());
        if crate::vmm::write_guest_memory(vm_id, elem_gpa, &elem) != 8
            || crate::vmm::write_guest_memory(vm_id, used_idx_gpa, &next_used.to_le_bytes()) != 2
        {
            return false;
        }
        self.rx_last_avail = self.rx_last_avail.wrapping_add(1);
        self.rx_used_idx = next_used;

        if let Some(irq) = self.irq {
            crate::vmm::inject_irq(vm_id, vcpu_id, irq);
        }
        true
    }
}


impl VirtioDevice for NetDev {
    fn device_id(&self) -> u32 {
        1
    } // DeviceID=1 = virtio-net

    fn device_features_lo(&self) -> u32 {
        VIRTIO_NET_F_MAC
    }

    /// Config space: MAC[0..5] at bytes 0-5, status at bytes 6-7.
    fn config_read(&self, offset: usize) -> u32 {
        match offset {
            0 => u32::from_le_bytes([GUEST_MAC[0], GUEST_MAC[1], GUEST_MAC[2], GUEST_MAC[3]]),
            4 => u32::from_le_bytes([GUEST_MAC[4], GUEST_MAC[5], 0, 0]),
            _ => 0,
        }
    }

    fn notify(&mut self, q: usize, qcfg: &QueueCfg, vm_id: usize, vcpu_id: usize) -> bool {
        match q {
            0 => false, // RX queue notify — guest added empty buffers; no action until frame arrives.
            1 => {
                // TX queue — drain guest TX descriptors and forward to the Net Cell.
                let backend = &mut self.backend;
                let mut tx_completed = false;
                let published = process_notify(
                    vm_id,
                    qcfg,
                    &mut self.tx_last_avail,
                    &mut self.tx_used_idx,
                    |bufs| {
                        tx_completed |= handle_tx(bufs, vm_id, backend);
                        0
                    },
                );
                if tx_completed && !self.tx_completion_logged {
                    ostd::io::println("[hv-virtio-host] net-tx-complete");
                    self.tx_completion_logged = true;
                }
                if published > 0 {
                    if let Some(irq) = self.irq {
                        crate::vmm::inject_irq(vm_id, vcpu_id, irq);
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn reset(&mut self) {
        self.rx_last_avail = 0;
        self.rx_used_idx = 0;
        self.tx_last_avail = 0;
        self.tx_used_idx = 0;
    }

    #[cfg(feature = "hostile-backend-recovery")]
    fn hostile_backend_fault(&mut self, command: u32) {
        if command == 1 && crate::backend_fault_control::disconnect(api::syscall::service::NET) {
            self.backend.force_unavailable_once();
            ostd::io::println("[hv-backend-fault-host] net unavailable");
        }
    }
}

/// Read all device-readable descriptor bytes, strip `virtio_net_hdr_v1`, and
/// forward the frame. Returns `true` only when the Net Cell acknowledges TX.
fn handle_tx(bufs: &[DescBuf], vm_id: usize, backend: &mut crate::net_backend::Connection) -> bool {
    if bufs.iter().any(|buf| buf.writable) {
        return false;
    }
    let Some(total) = bufs
        .iter()
        .try_fold(0usize, |sum, buf| sum.checked_add(buf.len as usize))
    else {
        return false;
    };
    if total <= VIRTIO_NET_HDR_V1_LEN || total > api::ipc::IPC_BUF_SIZE {
        ostd::io::println("[hv-virtio-host] reject net-tx-payload");
        return false;
    }

    let mut payload = alloc::vec::Vec::new();
    if payload.try_reserve_exact(total).is_err() {
        return false;
    }
    for buf in bufs {
        let start = payload.len();
        payload.resize(start + buf.len as usize, 0);
        if crate::vmm::read_guest_memory(vm_id, buf.gpa, &mut payload[start..]) != buf.len as usize
        {
            return false;
        }
    }
    crate::net_backend::transmit(backend, &payload[VIRTIO_NET_HDR_V1_LEN..])
}
