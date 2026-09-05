//! smoltcp Device adapter backed by a registered NIC Driver Cell.
//!
//! Tx/Rx operations resolve the provider registered under
//! `service::NIC_DRIVER`. A missed lookup remains retryable so a slow-starting
//! driver can become available later; transport failures invalidate the cached
//! TID so a restarted driver can be discovered.

extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use ostd::{
    io::println,
    syscall::{sys_lookup_service, sys_net_tx, sys_recv_timeout, sys_send, SyscallResult},
};
use smoltcp::{
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant,
};

/// Max scheduler ticks (10 ms each) to wait for a Driver Cell reply.
/// Bounded so a wedged/killed driver degrades to "no frames" instead of
/// parking net in Recv past its 5 s heartbeat (watchdog would kill net).
const DRV_REPLY_TIMEOUT_TICKS: u64 = 20; // 200 ms

/// Maximum Ethernet frame size accepted by the Net Cell.
const MAX_FRAME: usize = 1514;

/// NIC Driver Cell IPC op codes shared by VirtIO and e1000 providers.
const OP_TX: u8 = 0;
const OP_RX: u8 = 1;
// reason: only consumed by get_driver_mac(), which is not yet wired into net
// cell startup (interface MAC currently comes from a fixed default) — kept for
// the planned "adopt driver MAC" init path.
#[allow(dead_code)]
const OP_GETMAC: u8 = 2;

/// Zero means no NIC Driver Cell has been discovered yet.
const NOT_PROBED: usize = 0;

/// Cached active NIC Driver Cell TID.
static NIC_DRIVER_TID: AtomicUsize = AtomicUsize::new(NOT_PROBED);
static FIRST_BRIDGE_TX: AtomicBool = AtomicBool::new(false);
static FIRST_BRIDGE_RX: AtomicBool = AtomicBool::new(false);

fn resolve_cached_nic_driver(
    cache: &AtomicUsize,
    lookup: impl FnOnce() -> Option<usize>,
) -> Option<usize> {
    let cached = cache.load(Ordering::Relaxed);
    if cached != NOT_PROBED {
        return Some(cached);
    }

    let tid = lookup().filter(|tid| *tid != NOT_PROBED)?;
    cache.store(tid, Ordering::Relaxed);
    Some(tid)
}

fn invalidate_cached_nic_driver(cache: &AtomicUsize, tid: usize) {
    let _ = cache.compare_exchange(tid, NOT_PROBED, Ordering::Relaxed, Ordering::Relaxed);
}

/// Returns the active NIC Driver Cell TID, re-probing after absence or failure.
fn nic_driver_tid() -> Option<usize> {
    resolve_cached_nic_driver(&NIC_DRIVER_TID, || {
        sys_lookup_service(api::syscall::service::NIC_DRIVER)
    })
}

fn invalidate_nic_driver(tid: usize) {
    invalidate_cached_nic_driver(&NIC_DRIVER_TID, tid);
}

#[cfg(test)]
mod nic_driver_cache_tests {
    use super::*;

    #[test]
    fn retries_lookup_after_delayed_registration() {
        let cache = AtomicUsize::new(NOT_PROBED);

        assert_eq!(resolve_cached_nic_driver(&cache, || None), None);
        assert_eq!(cache.load(Ordering::Relaxed), NOT_PROBED);
        assert_eq!(resolve_cached_nic_driver(&cache, || Some(7)), Some(7));
        assert_eq!(cache.load(Ordering::Relaxed), 7);
    }

    #[test]
    fn invalidates_failed_driver_and_discovers_restart() {
        let cache = AtomicUsize::new(7);

        invalidate_cached_nic_driver(&cache, 7);
        assert_eq!(resolve_cached_nic_driver(&cache, || Some(9)), Some(9));

        invalidate_cached_nic_driver(&cache, 7);
        assert_eq!(cache.load(Ordering::Relaxed), 9);
    }
}

/// smoltcp `Device` implementation backed by a kernel IPC frame queue.
pub struct VirtioNetDevice {
    rx_queue: VecDeque<Box<[u8]>>,
    /// Frames destined for the hypervisor guest, separated by dst MAC.
    guest_rx_queue: VecDeque<Box<[u8]>>,
    guest_mac: Option<[u8; 6]>,
}

impl VirtioNetDevice {
    pub fn new() -> Self {
        Self {
            rx_queue: VecDeque::new(),
            guest_rx_queue: VecDeque::new(),
            guest_mac: None,
        }
    }

    /// Enqueue an inbound frame received from a NIC provider.
    // reason: pump_rx()/pump_rx_split() currently pull frames themselves via
    // the Driver Cell or disabled legacy syscalls; push_rx is the counterpart
    // for a future push-notification delivery path.
    #[allow(dead_code)]
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

    /// Transmit one raw Ethernet frame through the active NIC provider.
    pub fn send_l2(&self, frame: &[u8]) -> bool {
        send_l2_frame(frame)
    }
    /// Drain pending RX frames from the active NIC into the local queue.
    ///
    /// Routes through the registered NIC Driver Cell. The legacy kernel
    /// `NetRx` syscall is a disabled fallback while no provider is registered.
    /// Returns the number of frames pulled.
    pub fn pump_rx(&mut self) -> usize {
        let mut pulled = 0;
        let mut scratch = [0u8; MAX_FRAME];
        for _ in 0..16 {
            let n = if let Some(tid) = nic_driver_tid() {
                nic_rx_from_cell(tid, &mut scratch)
            } else {
                ostd::syscall::sys_net_rx(&mut scratch)
            };
            if n == 0 {
                break;
            }
            self.rx_queue.push_back(Box::from(&scratch[..n]));
            pulled += 1;
        }
        pulled
    }

    /// Drain pending RX frames, splitting by dst MAC when a guest MAC is registered.
    ///
    /// Returns the number of frames pulled from the active NIC path.
    pub fn pump_rx_split(&mut self) -> usize {
        let mut pulled = 0;
        let mut scratch = [0u8; MAX_FRAME];
        for _ in 0..16 {
            let n = if let Some(tid) = nic_driver_tid() {
                nic_rx_from_cell(tid, &mut scratch)
            } else {
                ostd::syscall::sys_net_rx(&mut scratch)
            };
            if n == 0 {
                break;
            }
            if !FIRST_BRIDGE_RX.swap(true, Ordering::Relaxed) {
                println(&alloc::format!("[net-bridge] first e1000 RX len={n}"));
            }
            let frame = &scratch[..n];
            match &self.guest_mac {
                None => {
                    self.rx_queue.push_back(Box::from(frame));
                }
                Some(mac) => {
                    let is_broadcast =
                        n >= 6 && frame[0..6] == [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
                    let is_guest = n >= 6 && frame[0..6] == mac[..];
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
            pulled += 1;
        }
        pulled
    }

    /// Query the NIC Driver Cell for the MAC address, if registered.
    // reason: not yet called from net cell init — see OP_GETMAC above.
    #[allow(dead_code)]
    pub fn get_driver_mac(&self) -> Option<[u8; 6]> {
        let tid = nic_driver_tid()?;
        match sys_send(tid, &[OP_GETMAC]) {
            SyscallResult::Err(_) => {
                invalidate_nic_driver(tid);
                return None;
            }
            SyscallResult::Ok(_) => {}
        }
        let mut mac = [0u8; 6];
        match sys_recv_timeout(tid, &mut mac, DRV_REPLY_TIMEOUT_TICKS) {
            SyscallResult::Ok(s) if s == tid => Some(mac),
            _ => {
                invalidate_nic_driver(tid);
                None
            }
        }
    }
}

/// Receive one Ethernet frame from the NIC Driver Cell.
/// Returns the frame length (0 = nothing ready).
fn nic_rx_from_cell(tid: usize, buf: &mut [u8]) -> usize {
    // Rx request: [0x01] — 1 byte.
    match sys_send(tid, &[OP_RX]) {
        SyscallResult::Err(_) => {
            invalidate_nic_driver(tid);
            return 0;
        }
        SyscallResult::Ok(_) => {}
    }
    // Reply: [len_lo, len_hi] ++ frame_bytes. Total ≤ 2 + MAX_FRAME.
    // Bounded wait: a blocking sys_recv here parked net past its 5 s heartbeat
    // whenever the driver dropped a request (watchdog kill → restart loop).
    // On timeout (Ok(0)) treat as "no frame ready" and move on.
    let mut reply = [0u8; 2 + MAX_FRAME];
    let sender = match sys_recv_timeout(tid, &mut reply, DRV_REPLY_TIMEOUT_TICKS) {
        SyscallResult::Ok(s) => s,
        _ => 0,
    };
    if sender != tid {
        // Timeout or misdelivery: re-resolve in case the provider restarted.
        invalidate_nic_driver(tid);
        return 0;
    }
    let len = u16::from_le_bytes([reply[0], reply[1]]) as usize;
    if len == 0 || len > buf.len() {
        return 0;
    }
    buf[..len].copy_from_slice(&reply[2..2 + len]);
    len
}

fn send_l2_frame(frame: &[u8]) -> bool {
    if frame.is_empty() || frame.len() > MAX_FRAME {
        return false;
    }
    if let Some(tid) = nic_driver_tid() {
        let frame_len = frame.len() as u16;
        let mut request = alloc::vec![OP_TX, (frame_len & 0xFF) as u8, (frame_len >> 8) as u8,];
        request.extend_from_slice(frame);
        if !matches!(sys_send(tid, &request), SyscallResult::Ok(_)) {
            invalidate_nic_driver(tid);
            return false;
        }
        let mut status = [1u8; 1];
        let accepted = match sys_recv_timeout(tid, &mut status, DRV_REPLY_TIMEOUT_TICKS) {
            SyscallResult::Ok(sender) if sender == tid => status[0] == 0,
            _ => {
                invalidate_nic_driver(tid);
                false
            }
        };
        if !FIRST_BRIDGE_TX.swap(true, Ordering::Relaxed) {
            println(&alloc::format!(
                "[net-bridge] first e1000 TX len={} accepted={accepted}",
                frame.len()
            ));
        }
        accepted
    } else {
        sys_net_tx(frame)
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
        let mut buffer = alloc::vec![0u8; len];
        let result = f(&mut buffer);
        let _ = send_l2_frame(&buffer);
        result
    }
}

impl Device for VirtioNetDevice {
    type RxToken<'a>
        = NetRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = NetTxToken
    where
        Self: 'a;

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
    fn default() -> Self {
        Self::new()
    }
}
