//! smoltcp Device adapter backed by kernel VirtIO net IPC.
//!
//! The kernel VirtIO net driver pushes raw Ethernet frames into the net cell
//! via IPC.  `VirtioNetDevice` queues them and feeds them to smoltcp on each
//! `poll()` call.  Transmitted frames are forwarded back to the kernel.

extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque};
use smoltcp::{
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant,
};
use ostd::syscall::sys_send;

/// Kernel net endpoint — receives TX frames from the net cell.
const KERNEL_NET_ENDPOINT: usize = 1;

/// Maximum Ethernet frame size (VirtIO net header is prepended by kernel).
const MAX_FRAME: usize = 1514;

/// smoltcp `Device` implementation backed by a kernel IPC frame queue.
pub struct VirtioNetDevice {
    rx_queue: VecDeque<Box<[u8]>>,
}

impl VirtioNetDevice {
    pub fn new() -> Self {
        Self { rx_queue: VecDeque::new() }
    }

    /// Enqueue an inbound frame received from the kernel VirtIO net driver.
    pub fn push_rx(&mut self, frame: Box<[u8]>) {
        self.rx_queue.push_back(frame);
    }
}

pub struct NetRxToken(Box<[u8]>);
pub struct NetTxToken;

impl RxToken for NetRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut frame = self.0;
        f(&mut frame)
    }
}

impl TxToken for NetTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        // Allocate a buffer, let smoltcp fill it, then IPC-send to kernel.
        let mut buf = alloc::vec![0u8; len];
        let result = f(&mut buf);
        // Prefix with TX opcode 0x01 for the kernel net driver.
        let mut msg = alloc::vec![0x01u8; len + 1];
        msg[1..].copy_from_slice(&buf);
        sys_send(KERNEL_NET_ENDPOINT, &msg);
        result
    }
}

impl Device for VirtioNetDevice {
    type RxToken<'a> = NetRxToken where Self: 'a;
    type TxToken<'a> = NetTxToken where Self: 'a;

    fn receive(&mut self, _ts: Instant) -> Option<(NetRxToken, NetTxToken)> {
        self.rx_queue
            .pop_front()
            .map(|frame| (NetRxToken(frame), NetTxToken))
    }

    fn transmit(&mut self, _ts: Instant) -> Option<NetTxToken> {
        Some(NetTxToken)
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = MAX_FRAME;
        caps.max_burst_size = Some(4);
        caps
    }
}

impl Default for VirtioNetDevice {
    fn default() -> Self { Self::new() }
}
