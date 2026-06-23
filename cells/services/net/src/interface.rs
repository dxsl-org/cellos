//! smoltcp Device adapter backed by kernel VirtIO net IPC or e1000 Driver Cell.
//!
//! On first Tx/Rx operation the adapter probes the service registry for a
//! registered e1000 NIC Driver Cell (`service::NIC_DRIVER`). When found, frames
//! are exchanged via IPC (e1000 DrvRequest protocol). When absent the kernel
//! VirtIO path (`sys_net_tx` / `sys_net_rx`) is used as the fallback — QEMU
//! VirtIO builds are unaffected.

extern crate alloc;

use core::sync::atomic::{AtomicUsize, Ordering};
use alloc::{boxed::Box, collections::VecDeque};
use smoltcp::{
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant,
};
use ostd::syscall::{
    sys_lookup_service, sys_net_tx, sys_recv, sys_send, SyscallResult,
};

/// Maximum Ethernet frame size (VirtIO net header is prepended by kernel).
const MAX_FRAME: usize = 1514;

/// e1000 IPC op codes (matching cells/drivers/e1000/src/dispatch.rs).
const OP_TX:     u8 = 0;
const OP_RX:     u8 = 1;
const OP_GETMAC: u8 = 2;

/// Sentinel values for E1000_TID.
const NOT_PROBED: usize = 0;
const ABSENT:     usize = usize::MAX;

/// Cached e1000 Driver Cell TID. NOT_PROBED on startup.
static E1000_TID: AtomicUsize = AtomicUsize::new(NOT_PROBED);

/// Returns the e1000 Driver Cell TID if one has registered, else `None`.
pub fn e1000_tid() -> Option<usize> {
    let cached = E1000_TID.load(Ordering::Relaxed);
    if cached == ABSENT     { return None; }
    if cached != NOT_PROBED { return Some(cached); }

    match sys_lookup_service(api::syscall::service::NIC_DRIVER) {
        Some(tid) if tid != 0 => {
            E1000_TID.store(tid, Ordering::Relaxed);
            Some(tid)
        }
        _ => {
            E1000_TID.store(ABSENT, Ordering::Relaxed);
            None
        }
    }
}

/// smoltcp `Device` implementation backed by a kernel IPC frame queue.
pub struct VirtioNetDevice {
    rx_queue:       VecDeque<Box<[u8]>>,
    /// Frames destined for the hypervisor guest, separated by dst MAC.
    guest_rx_queue: VecDeque<Box<[u8]>>,
    guest_mac:      Option<[u8; 6]>,
}

impl VirtioNetDevice {
    pub fn new() -> Self {
        Self {
            rx_queue:       VecDeque::new(),
            guest_rx_queue: VecDeque::new(),
            guest_mac:      None,
        }
    }

    /// Enqueue an inbound frame received from the kernel VirtIO net driver.
    pub fn push_rx(&mut self, frame: Box<[u8]>) {
        self.rx_queue.push_back(frame);
    }

    /// Register the guest MAC address for L2 bridging.
    pub fn set_guest_mac(&mut self, mac: [u8; 6]) {
        self.guest_mac = Some(mac);
    }

    /// Pop one frame from the guest RX queue.
    pub fn pop_guest_rx(&mut self) -> Option<Box<[u8]>> {
        self.guest_rx_queue.pop_front()
    }

    /// Drain pending RX frames from the active NIC into the local queue.
    ///
    /// Routes to the e1000 Driver Cell when registered; falls back to the
    /// kernel VirtIO `NetRx` syscall.
    /// Returns the number of frames pulled.
    pub fn pump_rx(&mut self) -> usize {
        let mut pulled = 0;
        let mut scratch = [0u8; MAX_FRAME];
        for _ in 0..16 {
            let n = if let Some(tid) = e1000_tid() {
                nic_rx_from_cell(tid, &mut scratch)
            } else {
                ostd::syscall::sys_net_rx(&mut scratch)
            };
            if n == 0 { break; }
            self.rx_queue.push_back(Box::from(&scratch[..n]));
            pulled += 1;
        }
        pulled
    }

    /// Drain pending RX frames, splitting by dst MAC when a guest MAC is registered.
    pub fn pump_rx_split(&mut self) {
        let mut scratch = [0u8; MAX_FRAME];
        for _ in 0..16 {
            let n = if let Some(tid) = e1000_tid() {
                nic_rx_from_cell(tid, &mut scratch)
            } else {
                ostd::syscall::sys_net_rx(&mut scratch)
            };
            if n == 0 { break; }
            let frame = &scratch[..n];
            match &self.guest_mac {
                None => {
                    self.rx_queue.push_back(Box::from(frame));
                }
                Some(mac) => {
                    let is_broadcast = n >= 6 && frame[0..6] == [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
                    let is_guest     = n >= 6 && frame[0..6] == mac[..];
                    if is_broadcast {
                        self.guest_rx_queue.push_back(Box::from(frame));
                        self.rx_queue.push_back(Box::from(frame));
                    } else if is_guest {
                        self.guest_rx_queue.push_back(Box::from(frame));
                    } else {
                        self.rx_queue.push_back(Box::from(frame));
                    }
                }
            }
        }
    }

    /// Query the e1000 Driver Cell for the MAC address, if registered.
    pub fn get_driver_mac(&self) -> Option<[u8; 6]> {
        let tid = e1000_tid()?;
        match sys_send(tid, &[OP_GETMAC]) {
            SyscallResult::Err(_) => return None,
            SyscallResult::Ok(_)  => {}
        }
        let mut mac = [0u8; 6];
        sys_recv(tid, &mut mac);
        Some(mac)
    }
}

/// Receive one Ethernet frame from the e1000 Driver Cell.
/// Returns the frame length (0 = nothing ready).
fn nic_rx_from_cell(tid: usize, buf: &mut [u8]) -> usize {
    // Rx request: [0x01] — 1 byte.
    match sys_send(tid, &[OP_RX]) {
        SyscallResult::Err(_) => return 0,
        SyscallResult::Ok(_)  => {}
    }
    // Reply: [len_lo, len_hi] ++ frame_bytes. Total ≤ 2 + MAX_FRAME.
    let mut reply = [0u8; 2 + MAX_FRAME];
    sys_recv(tid, &mut reply);
    let len = u16::from_le_bytes([reply[0], reply[1]]) as usize;
    if len == 0 || len > buf.len() { return 0; }
    buf[..len].copy_from_slice(&reply[2..2 + len]);
    len
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
        let mut buf = alloc::vec![0u8; len];
        let result = f(&mut buf);
        // Route to e1000 Driver Cell when registered; otherwise kernel VirtIO.
        if let Some(tid) = e1000_tid() {
            // Tx request: [0x00] ++ frame_bytes.
            let mut req = alloc::vec![OP_TX];
            req.extend_from_slice(&buf);
            match sys_send(tid, &req) {
                SyscallResult::Ok(_) => {
                    // Discard the 1-byte status reply (fire-and-continue).
                    let mut status = [0u8; 1];
                    sys_recv(tid, &mut status);
                }
                SyscallResult::Err(_) => {}
            }
        } else {
            sys_net_tx(&buf);
        }
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
