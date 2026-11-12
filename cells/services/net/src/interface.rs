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
    syscall::{sys_lookup_service, sys_net_tx, sys_recv_timeout, sys_try_send, SyscallResult},
};
use smoltcp::{
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant,
};

use crate::bridge_ports::{Bridge, PortError};
use crate::socket_table::SocketOwner;

/// Driver replies are untagged, so only one command may be in flight. A lost
/// reply must not block TCP dispatch; the finite deadline retires that driver
/// TID before any further commands are admitted.
const DRV_REPLY_TIMEOUT_TICKS: u64 = 20;
/// How long one request may wait for the driver to be receiving.
///
/// The driver parks in `Recv` for one tick per turn and spends the rest of the
/// turn inside USB transfers, so an unbounded wait parks this cell for as long
/// as the transfer lasts — past its own liveness window.
const DRV_ACCEPT_TIMEOUT_TICKS: u64 = 20;
const DRIVER_TICKS_PER_SCHEDULER_TICK: u64 = 100_000;
const TX_QUEUE_LIMIT: usize = 32;
const RX_QUEUE_LIMIT: usize = 32;
/// ARP packets held for the link-local engine between loop turns. Probing
/// traffic is a handful of frames; a flood past this bound is dropped from the
/// tap only, never from smoltcp's own RX path.
const ARP_TAP_LIMIT: usize = 16;

/// Attempts a frame gets after the driver refuses it (`status != 0`).
///
/// A refusal means the driver could not put the frame on the wire *now* — a USB
/// transfer error, or its front-end still parked — and the board's ping lost its
/// ARP to exactly that: the frame was dropped on the first refusal. Eight turns
/// is a few hundred milliseconds of retrying at the driver's pace, and the bound
/// is what keeps a driver that refuses everything from pinning the queue head.
const MAX_TX_REFUSALS: u8 = 8;

/// Maximum Ethernet frame size accepted by the Net Cell.
const MAX_FRAME: usize = 1514;
/// Shortest frame a NIC driver can hand back: the Ethernet header alone.
///
/// A driver that cannot serve a request answers with a one-byte status, so a
/// shorter length reading is a status, not a frame.
const MIN_FRAME: usize = 14;

/// NIC Driver Cell IPC op codes shared by VirtIO and e1000 providers.
const OP_TX: u8 = 0;
const OP_RX: u8 = 1;
const OP_GETMAC: u8 = 2;
/// Status bytes a NIC driver answers a request with (see the drivers' wire
/// protocol). Only [`STATUS_OK`] means the frame went out; the rest are refusals
/// this cell retries, and the value is carried to the L2Send caller so its log can
/// say whether the chip refused or the driver was not ready.
const STATUS_OK: u8 = 0;
const STATUS_FAILED: u8 = 1;

/// Zero means no NIC Driver Cell has been discovered yet.
const NOT_PROBED: usize = 0;

/// Cached active NIC Driver Cell TID.
static NIC_DRIVER_TID: AtomicUsize = AtomicUsize::new(NOT_PROBED);
static FIRST_BRIDGE_TX: AtomicBool = AtomicBool::new(false);
static FIRST_BRIDGE_TX_OK: AtomicBool = AtomicBool::new(false);
static FIRST_REPLY_FAILURE: AtomicBool = AtomicBool::new(false);
static FIRST_BRIDGE_RX: AtomicBool = AtomicBool::new(false);
static LEGACY_LOGS: AtomicUsize = AtomicUsize::new(0);
/// Driver commands issued, and how many burned their deadline (`loop-trace`).
///
/// The Net Cell shares one loop between its own smoltcp traffic and the L2
/// bridge, so a driver that answers late is visible here as a stalled loop.
pub static DRV_COMMANDS: AtomicUsize = AtomicUsize::new(0);
pub static DRV_TIMEOUTS: AtomicUsize = AtomicUsize::new(0);

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

#[cfg(test)]
mod driver_mac_tests {
    use super::*;

    #[test]
    fn query_driver_mac_defaults_to_none_without_driver() {
        let mut dev = VirtioNetDevice::new();
        assert_eq!(dev.driver_mac(), None);
        assert_eq!(dev.query_driver_mac(), None);
    }
}

#[cfg(test)]
mod tx_queue_retry_tests {
    use super::*;

    /// The driver's front-end was not parked: its request never reached the chip.
    /// The Net Cell treats every non-zero status alike, so only this test needs
    /// the name.
    const STATUS_NOT_READY: u8 = 2;

    /// A frame the driver refuses keeps its place for the next turn.
    ///
    /// The board lost a guest ARP this way — one refusal (`first guest TX
    /// accepted=false`) and the frame was gone — and a frame the driver keeps
    /// refusing must still leave the queue at the bound, or one wedged transfer
    /// would pin the head and starve every later frame.
    #[test]
    fn a_refused_frame_is_retried_then_released_at_the_bound() {
        let mut dev = VirtioNetDevice::new();
        dev.queue_tx(Box::from(&[0xAAu8; 64][..]));

        for attempt in 1..MAX_TX_REFUSALS {
            assert!(
                !dev.settle_tx_verdict(STATUS_FAILED),
                "attempt {attempt} is not a send"
            );
            assert_eq!(
                dev.tx_queue.len(),
                1,
                "attempt {attempt} must keep the frame"
            );
            assert_eq!(dev.tx_queue.front().expect("queued").refusals, attempt);
        }
        assert!(!dev.settle_tx_verdict(STATUS_FAILED));
        assert!(dev.tx_queue.is_empty(), "the bound must release the head");
    }

    /// An accepted frame leaves at once, and a refusal answers its L2Send only
    /// once: the retry that follows must not answer the same request twice.
    #[test]
    fn an_accepted_frame_leaves_and_a_refusal_clears_its_reply() {
        let mut dev = VirtioNetDevice::new();
        dev.queue_tx(Box::from(&[0xAAu8; 64][..]));
        dev.tx_queue.front_mut().expect("queued").reply = Some(ReplyTo::Legacy(9));

        assert!(!dev.settle_tx_verdict(STATUS_NOT_READY));
        assert!(dev.tx_queue.front().expect("queued").reply.is_none());

        assert!(dev.settle_tx_verdict(STATUS_OK));
        assert!(dev.tx_queue.is_empty());
    }
}

/// Recycled frame buffers kept for reuse; bounds the pool's memory to a few
/// dozen MTU-sized frames.
const FRAME_POOL_CAPACITY: usize = 32;

/// smoltcp device with a bounded local TX queue and one async driver command.
/// A smoltcp transmit token commits to local queue admission, not peer receipt.
pub struct VirtioNetDevice {
    rx_queue: VecDeque<Box<[u8]>>,
    tx_queue: VecDeque<OutboundFrame>,
    /// Guest L2 ports beside the host stack.
    bridge: Bridge,
    /// The host stack's own address; no bridge port may use it.
    host_mac: [u8; 6],
    driver_mac: Option<([u8; 6], usize)>,
    l2_replies: VecDeque<(ReplyTo, u8, u64)>,
    tx_burst: u8,
    frame_pool: alloc::vec::Vec<Box<[u8]>>,
    /// ARP seen on RX, for the RFC 3927 link-local engine.
    arp_rx: VecDeque<crate::arp::ArpPacket>,
}

struct OutboundFrame {
    bytes: Box<[u8]>,
    reply: Option<ReplyTo>,
    /// Times the driver refused this frame (`status != 0`), so a frame the driver
    /// cannot send is retried a bounded number of turns instead of once.
    refusals: u8,
}

#[derive(Clone, Copy)]
enum ReplyTo {
    Async(usize),
    Legacy(usize),
}

impl VirtioNetDevice {
    pub fn new() -> Self {
        Self {
            rx_queue: VecDeque::new(),
            tx_queue: VecDeque::new(),
            bridge: Bridge::default(),
            host_mac: [0; 6],
            driver_mac: None,
            l2_replies: VecDeque::new(),
            tx_burst: 0,
            frame_pool: alloc::vec::Vec::new(),
            arp_rx: VecDeque::new(),
        }
    }

    /// A recycled buffer of exactly `len` bytes. Buffers of other lengths stay
    /// pooled: frame sizes vary, and dropping a mismatch would empty the pool.
    fn take_pooled(&mut self, len: usize) -> Option<Box<[u8]>> {
        let index = self.frame_pool.iter().rposition(|buf| buf.len() == len)?;
        Some(self.frame_pool.swap_remove(index))
    }

    /// A zeroed TX buffer. smoltcp is not obliged to write every byte of the
    /// frame it emits, and a recycled buffer still holds an earlier frame that
    /// must not leak onto the wire.
    fn alloc_frame(&mut self, len: usize) -> Box<[u8]> {
        match self.take_pooled(len) {
            Some(mut buf) => {
                buf.fill(0);
                buf
            }
            None => alloc::vec![0u8; len].into_boxed_slice(),
        }
    }

    /// Return a sent frame's buffer to the pool, evicting an old buffer when
    /// full so the pool follows the current frame sizes.
    fn recycle_frame(&mut self, frame: Box<[u8]>) {
        if self.frame_pool.len() >= FRAME_POOL_CAPACITY {
            self.frame_pool.remove(0);
        }
        self.frame_pool.push(frame);
    }

    /// Copy a received frame into a pooled buffer (fully overwritten, so no
    /// zeroing), or a fresh exact-size allocation.
    fn clone_into_pooled_frame(&mut self, frame: &[u8]) -> Box<[u8]> {
        match self.take_pooled(frame.len()) {
            Some(mut buf) => {
                buf.copy_from_slice(frame);
                buf
            }
            None => Box::from(frame),
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

    /// Next ARP packet tapped off the RX path. smoltcp still receives the
    /// frame; the tap exists because it neither exposes ARP nor accepts the
    /// sender-IP-0 probes RFC 3927 conflict detection depends on.
    pub(crate) fn take_arp(&mut self) -> Option<crate::arp::ArpPacket> {
        self.arp_rx.pop_front()
    }

    /// Queue a frame built outside smoltcp on the same TX path smoltcp uses.
    /// `false` when the queue is full; the caller's protocol retries.
    pub(crate) fn send_frame(&mut self, frame: &[u8]) -> bool {
        if self.tx_queue.len() >= TX_QUEUE_LIMIT {
            return false;
        }
        let mut buf = self.alloc_frame(frame.len());
        buf.copy_from_slice(frame);
        self.queue_tx(buf);
        true
    }

    /// The station address queried from the bound NIC driver, if any.
    #[allow(dead_code)]
    pub fn driver_mac(&self) -> Option<[u8; 6]> {
        self.driver_mac.map(|(mac, _)| mac)
    }

    /// Query the bound NIC driver's hardware MAC via IPC `OP_GETMAC` (2).
    ///
    /// Validates that the returned MAC is non-zero, not broadcast, and unicast
    /// (least significant bit of first octet is 0). Caches the result by driver TID.
    pub fn query_driver_mac(&mut self) -> Option<[u8; 6]> {
        let tid = nic_driver_tid()?;
        if let Some((mac, cached_tid)) = self.driver_mac {
            if cached_tid == tid {
                return Some(mac);
            }
        }
        let mut reply = [0u8; 6];
        Self::command(tid, &[OP_GETMAC], &mut reply)?;
        if reply == [0; 6] || reply == [0xff; 6] || (reply[0] & 1 != 0) {
            return None;
        }
        self.driver_mac = Some((reply, tid));
        Some(reply)
    }

    /// Record the host stack's address (the interface's hardware address).
    pub fn set_host_mac(&mut self, mac: [u8; 6]) {
        self.host_mac = mac;
    }

    /// Open `owner`'s bridge port at `mac` (`L2Recv` names it on every poll;
    /// an open port keeps the address its guest sends from).
    pub(crate) fn open_port(&mut self, owner: SocketOwner, mac: [u8; 6]) -> Result<(), PortError> {
        self.bridge.open(owner, mac, self.host_mac)
    }

    /// Check a frame `owner`'s guest wants sent; see [`Bridge::admit_tx`].
    pub(crate) fn admit_port_tx(
        &mut self,
        owner: SocketOwner,
        frame: &[u8],
    ) -> Result<(), PortError> {
        self.bridge.admit_tx(owner, frame, self.host_mac)
    }

    /// Close the port of an owner that exited.
    pub(crate) fn release_port(&mut self, owner: SocketOwner) -> bool {
        self.bridge.release(owner)
    }

    /// Next frame for `owner`'s bridge port.
    pub(crate) fn pop_port_rx(&mut self, owner: SocketOwner) -> Option<Box<[u8]>> {
        self.bridge.pop(owner)
    }

    /// Frames waiting for smoltcp (`loop-trace` images).
    #[cfg(feature = "loop-trace")]
    pub fn rx_queue_len(&self) -> usize {
        self.rx_queue.len()
    }

    /// Frames the bridge has routed to guests and nobody has collected.
    #[cfg(feature = "loop-trace")]
    pub fn bridge_queued(&self) -> usize {
        self.bridge.queued()
    }

    /// Frames waiting for the NIC driver (host tests).
    #[cfg(test)]
    pub(crate) fn tx_queue_len(&self) -> usize {
        self.tx_queue.len()
    }

    /// The address `owner`'s bridge port answers to (host tests).
    #[cfg(test)]
    pub(crate) fn port_mac(&self, owner: SocketOwner) -> Option<[u8; 6]> {
        self.bridge.mac(owner)
    }

    /// L2Send returns success only after the NIC driver ACKs. A None result
    /// means the request was queued; pump_rx_split sends its eventual reply.
    pub fn send_l2(&mut self, frame: &[u8], sender: usize) -> Option<bool> {
        if frame.is_empty() || frame.len() > MAX_FRAME {
            return Some(false);
        }
        if nic_driver_tid().is_none() {
            return Some(sys_net_tx(frame));
        }
        if self.tx_queue.len() >= TX_QUEUE_LIMIT {
            return Some(false);
        }
        if self
            .tx_queue
            .iter()
            .filter(|pending| pending.reply.is_some())
            .count()
            + self.l2_replies.len()
            >= TX_QUEUE_LIMIT
        {
            return Some(false);
        }
        let reply = match ostd::ipc::current() {
            Some(op) => ReplyTo::Async(op),
            None => ReplyTo::Legacy(sender),
        };
        self.tx_queue.push_back(OutboundFrame {
            bytes: Box::from(frame),
            reply: Some(reply),
            refusals: 0,
        });
        None
    }

    /// Offer one request to the driver, waiting at most
    /// [`DRV_ACCEPT_TIMEOUT_TICKS`] for it to be receiving.
    ///
    /// The kernel's rendezvous send completes only while the driver is parked in
    /// `Recv`, and a driver inside a USB transfer is not. Blocking instead
    /// (`sys_send` parks the caller in `Sending`) outlives this cell's liveness
    /// window: the board killed the Net Cell with `Sending { target: 4 }` while
    /// the driver was `Running` and its request still sat in the driver's
    /// mailbox, and every queued frame died with it. A bounded offer keeps the
    /// cell alive — a busy driver costs one turn, not the service.
    fn offer(tid: usize, request: &[u8]) -> bool {
        let started = crate::service_runtime::sys_get_time();
        loop {
            if matches!(sys_try_send(tid, request), SyscallResult::Ok(0)) {
                return true;
            }
            if crate::service_runtime::sys_get_time().wrapping_sub(started)
                >= DRV_ACCEPT_TIMEOUT_TICKS * DRIVER_TICKS_PER_SCHEDULER_TICK
            {
                return false;
            }
            ostd::syscall::sys_yield();
        }
    }

    /// Take replies this mailbox no longer has a request for.
    ///
    /// The driver's reply is queued rather than droppable, so an abandoned
    /// request leaves a late reply behind, and replies are untagged (TX is a
    /// status byte, RX is a length-prefixed frame) — the next command would read
    /// it as its own answer. This also releases a driver parked in a blocking
    /// reply to *this* cell, which is what lets the next offer be received.
    fn drain_replies(tid: usize) {
        let mut stale = [0u8; MAX_FRAME + 2];
        for _ in 0..4 {
            match ostd::syscall::sys_try_recv(tid, &mut stale) {
                SyscallResult::Ok(sender) if sender == tid => {}
                _ => break,
            }
        }
    }

    /// One driver command per turn, awaited inline through a masked receive.
    ///
    /// The wait is bounded by [`DRV_REPLY_TIMEOUT_TICKS`]: a wedged driver costs
    /// one deadline and its cache entry is dropped so a restarted driver is
    /// rediscovered, instead of parking TCP dispatch indefinitely.
    fn command(tid: usize, request: &[u8], reply: &mut [u8]) -> Option<u64> {
        DRV_COMMANDS.fetch_add(1, Ordering::Relaxed);
        let now = crate::service_runtime::sys_get_time();
        if !Self::offer(tid, request) {
            // Not a wedged driver: it is alive and mid-transfer. Keep the cached
            // tid, release any reply it is blocked on, and retry next turn.
            DRV_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
            Self::drain_replies(tid);
            return None;
        }
        match sys_recv_timeout(tid, reply, DRV_REPLY_TIMEOUT_TICKS) {
            SyscallResult::Ok(sender) if sender == tid => Some(now),
            _ => {
                DRV_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
                Self::drain_replies(tid);
                invalidate_nic_driver(tid);
                if !FIRST_REPLY_FAILURE.swap(true, Ordering::Relaxed) {
                    println("[net-bridge] NIC driver reply timeout; frame not acknowledged");
                }
                None
            }
        }
    }

    /// At most one driver request is outstanding, so an untagged reply can never
    /// be matched to another opcode. Receive frames are pumped one per turn.
    pub fn pump_rx_split(&mut self) -> usize {
        self.flush_l2_replies(crate::service_runtime::sys_get_time());
        let Some(tid) = nic_driver_tid() else {
            if LEGACY_LOGS.fetch_add(1, Ordering::Relaxed) < 3 {
                println("[net] NIC driver unavailable: legacy frame path");
            }
            let now = crate::service_runtime::sys_get_time();
            if let Some(sent) = self.tx_queue.pop_front() {
                let accepted = sys_net_tx(&sent.bytes);
                if let Some(target) = sent.reply {
                    // The status byte the driver path would have carried; the
                    // legacy syscall has no finer verdict than yes/no.
                    let status = if accepted { STATUS_OK } else { STATUS_FAILED };
                    self.l2_replies.push_back((target, status, now));
                }
                self.recycle_frame(sent.bytes);
            }
            let mut scratch = [0u8; MAX_FRAME];
            let n = ostd::syscall::sys_net_rx(&mut scratch);
            return if n > 0 && n <= MAX_FRAME && self.rx_queue.len() < RX_QUEUE_LIMIT {
                self.route_rx(&scratch[..n]);
                1
            } else {
                0
            };
        };

        let mut reply = [0u8; MAX_FRAME + 2];
        // Transmit up to four queued frames. A frame the driver was too busy to
        // accept keeps its place in the queue and is offered again next turn:
        // dropping it on the first busy moment lost the guest's ARP and echo
        // requests, and the queue bound is what keeps a wedged driver from
        // growing the queue without limit. Reception is polled below regardless,
        // so a stuck head frame never starves it.
        let mut attempts = 0usize;
        while attempts < 4 && !self.tx_queue.is_empty() {
            let mut request = [0u8; MAX_FRAME + 3];
            let (len, target) = {
                let frame = self.tx_queue.front().expect("not empty");
                request[0] = OP_TX;
                request[1..3].copy_from_slice(&(frame.bytes.len() as u16).to_le_bytes());
                request[3..3 + frame.bytes.len()].copy_from_slice(&frame.bytes);
                (frame.bytes.len() + 3, frame.reply)
            };
            attempts += 1;
            // Replies are untagged and the driver answers a request it cannot
            // serve with a single status byte, so a shorter reply must not be
            // read with a previous reply's trailing bytes.
            reply.fill(0);
            let Some(now) = Self::command(tid, &request[..len], &mut reply) else {
                break;
            };
            self.tx_burst = self.tx_burst.saturating_add(1);
            // The driver's own verdict byte: 0 accepted, 1 the USB transfer
            // failed, 2 its front-end was not parked. It is carried through to the
            // caller instead of being flattened to one "failed" value, because
            // "the chip refused" and "the driver was busy" need different fixes.
            let status = reply.first().copied().unwrap_or(STATUS_FAILED);
            let accepted = self.settle_tx_verdict(status);
            if let Some(target) = target {
                self.l2_replies.push_back((target, status, now));
            }
            if !FIRST_BRIDGE_TX.swap(true, Ordering::Relaxed) {
                println(&alloc::format!(
                    "[net-bridge] first e1000 TX accepted={accepted}"
                ));
            }
            if accepted {
                FIRST_BRIDGE_TX_OK.store(true, Ordering::Relaxed);
            } else {
                invalidate_nic_driver(tid);
                // One refusal per turn for this frame: the burst exists to drain
                // *other* queued frames, and the driver just said it cannot send.
                break;
            }
        }

        // Reception is polled every turn regardless of transmit progress, and a
        // burst is drained before returning: the NIC driver only recycles one
        // receive buffer per request, so returning after a single frame lets
        // the virtqueue's available buffers run out under a burst and the
        // device then drops every later packet.
        let mut received = 0usize;
        // A full guest port drops its own frames (`Bridge::route`), so only the
        // host stack's queue bounds the drain.
        while received < RX_QUEUE_LIMIT && self.rx_queue.len() < RX_QUEUE_LIMIT {
            self.tx_burst = 0;
            reply.fill(0);
            if Self::command(tid, &[OP_RX], &mut reply).is_none() {
                break;
            }
            // A status reply is one byte, so `reply[1]` is only a length byte
            // when the driver actually sent a frame; the buffer is zeroed above
            // and an Ethernet frame is never shorter than its 14-byte header, so
            // a shorter reading is "no frame" rather than a frame to route. The
            // board showed the alternative: `first e1000 RX len=1`, a status
            // byte routed into the stack as a frame.
            let n = u16::from_le_bytes([reply[0], reply[1]]) as usize;
            if !(MIN_FRAME..=MAX_FRAME).contains(&n) {
                break;
            }
            self.route_rx(&reply[2..n + 2]);
            received += 1;
        }
        received.min(1)
    }

    fn route_rx(&mut self, frame: &[u8]) {
        if !FIRST_BRIDGE_RX.swap(true, Ordering::Relaxed) {
            println(&alloc::format!(
                "[net-bridge] first e1000 RX len={}",
                frame.len()
            ));
        }
        if self.arp_rx.len() < ARP_TAP_LIMIT {
            if let Some(packet) = crate::arp::ArpPacket::parse_frame(frame) {
                self.arp_rx.push_back(packet);
            }
        }
        if self.bridge.route(frame) {
            let b = self.clone_into_pooled_frame(frame);
            self.rx_queue.push_back(b);
        }
    }

    fn queue_tx(&mut self, frame: Box<[u8]>) {
        debug_assert!(self.tx_queue.len() < TX_QUEUE_LIMIT);
        self.tx_queue.push_back(OutboundFrame {
            bytes: frame,
            reply: None,
            refusals: 0,
        });
    }

    /// Apply one driver verdict to the head of the TX queue.
    ///
    /// `true` means the frame went out and has left the queue. A refusal
    /// (`status != STATUS_OK`) keeps the frame at the head for the next turn,
    /// which is the same policy the "driver was busy" case already used: popping
    /// it dropped the guest's ARP on the first USB transfer error — the board
    /// logged `first guest TX accepted=false` and the ping that followed had no
    /// packets received, with no second chance for the frame. The retry is bounded
    /// by [`MAX_TX_REFUSALS`] so a driver that refuses everything cannot pin the
    /// head, and the frame's pending reply is cleared on the first refusal because
    /// its verdict is answered exactly once (a later attempt that succeeds must not
    /// answer the same request twice).
    fn settle_tx_verdict(&mut self, status: u8) -> bool {
        if status == STATUS_OK {
            if let Some(frame) = self.tx_queue.pop_front() {
                self.recycle_frame(frame.bytes);
            }
            return true;
        }
        let refusals = {
            let Some(frame) = self.tx_queue.front_mut() else {
                return false;
            };
            frame.reply = None;
            frame.refusals = frame.refusals.saturating_add(1);
            frame.refusals
        };
        if refusals >= MAX_TX_REFUSALS {
            if let Some(frame) = self.tx_queue.pop_front() {
                self.recycle_frame(frame.bytes);
            }
            static FIRST_TX_DROP: core::sync::atomic::AtomicBool =
                core::sync::atomic::AtomicBool::new(false);
            if !FIRST_TX_DROP.swap(true, Ordering::Relaxed) {
                println(&alloc::format!(
                    "[net-bridge] dropped a frame the driver refused {refusals}x (status {status})"
                ));
            }
        }
        false
    }

    fn flush_l2_replies(&mut self, now: u64) {
        for _ in 0..4 {
            let Some((target, status, since)) = self.l2_replies.front() else {
                break;
            };
            // The driver's verdict for a frame the guest sent — `l2_replies` only
            // ever holds L2Send targets. "Did the command I typed in the guest
            // leave the board" is the first half of any such question, and one
            // static for the first attempt answered it only when that attempt was
            // the interesting one: the board's run showed `accepted=false` and the
            // retry that followed went unrecorded. Both halves print once, and the
            // status byte names which refusal it was.
            let status = *status;
            let accepted = status == 0;
            {
                static FIRST_GUEST_TX_OK: core::sync::atomic::AtomicBool =
                    core::sync::atomic::AtomicBool::new(false);
                static FIRST_GUEST_TX_FAIL: core::sync::atomic::AtomicBool =
                    core::sync::atomic::AtomicBool::new(false);
                let witness = if accepted {
                    &FIRST_GUEST_TX_OK
                } else {
                    &FIRST_GUEST_TX_FAIL
                };
                if !witness.swap(true, core::sync::atomic::Ordering::Relaxed) {
                    println(&alloc::format!(
                        "[net-bridge] first guest TX accepted={accepted} status={status}"
                    ));
                }
            }
            let mut encoded = [0u8; 8];
            let response = if accepted {
                api::ipc::NetResponse::Ok
            } else {
                api::ipc::NetResponse::Err(status)
            };
            let Ok(bytes) = api::ipc::encode(&response, &mut encoded) else {
                break;
            };
            let delivered = match target {
                ReplyTo::Async(op) => {
                    let _ = ostd::ipc::reply(*op, bytes);
                    true
                }
                ReplyTo::Legacy(sender) => {
                    matches!(sys_try_send(*sender, bytes), SyscallResult::Ok(0))
                        || now.saturating_sub(*since)
                            > DRV_REPLY_TIMEOUT_TICKS * DRIVER_TICKS_PER_SCHEDULER_TICK
                }
            };
            if delivered {
                self.l2_replies.pop_front();
            } else {
                break;
            }
        }
    }
}

pub struct NetRxToken(Box<[u8]>);
pub struct NetTxToken<'a>(&'a mut VirtioNetDevice);

impl RxToken for NetRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut frame = self.0;
        f(&mut frame)
    }
}

impl TxToken for NetTxToken<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut frame = self.0.alloc_frame(len);
        let result = f(&mut frame);
        self.0.queue_tx(frame);
        result
    }
}

impl Device for VirtioNetDevice {
    type RxToken<'a>
        = NetRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = NetTxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, _ts: Instant) -> Option<(NetRxToken, NetTxToken<'_>)> {
        if self.tx_queue.len() >= TX_QUEUE_LIMIT {
            return None;
        }
        self.rx_queue
            .pop_front()
            .map(|frame| (NetRxToken(frame), NetTxToken(self)))
    }

    fn transmit(&mut self, _ts: Instant) -> Option<NetTxToken<'_>> {
        (self.tx_queue.len() < TX_QUEUE_LIMIT).then_some(NetTxToken(self))
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
