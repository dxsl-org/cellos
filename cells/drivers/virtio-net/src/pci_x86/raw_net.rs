//! virtio-net over the PCI transport with the virtio 1.x header layout.
//!
//! The PCI transport always negotiates `VIRTIO_F_VERSION_1`, under which every
//! packet is preceded by a 12-byte `virtio_net_hdr` (`num_buffers` is always
//! present; virtio 1.2 §5.1.6). The driver library's `VirtIONet` assumes the
//! legacy 10-byte header, so this path drives `VirtIONetRaw` and owns framing:
//! - TX: one buffer = zeroed 12-byte header ++ frame (no offloads requested).
//! - RX: the device writes header ++ frame into a posted buffer; the frame
//!   starts at byte 12 and is `used_len - 12` bytes long. The library's split
//!   of `used_len` into (header, packet) assumes 10 bytes, so only the sum is
//!   used.
//!
//! Every buffer handed to the library is shared through the authorised bounce
//! pool in `hal`, so heap memory is fine here.

#![allow(unsafe_code)]

use super::claims::BarClaims;
use super::wait::{Deadline, TX_BUDGET_MS};
use super::{Bdf, CheckedTransport, PciHal};
use crate::device::{NET_QUEUE_SIZE, RX_BUF_LEN};
use alloc::boxed::Box;
use alloc::format;
use alloc::vec;
use ostd::syscall::sys_get_time_ms;
use virtio_drivers::device::net::VirtIONetRaw;
use virtio_drivers::transport::DeviceStatus;
use virtio_drivers::Error;

/// `virtio_net_hdr` length when `VIRTIO_F_VERSION_1` is negotiated.
const NET_HDR_LEN: usize = 12;

/// The NIC plus the RX buffers and BAR claims it currently owns.
///
/// Field order matters: `raw` drops first, which resets the device through
/// the transport before the RX buffers it may still be writing are freed and
/// before the BAR claims its registers live in are released.
pub struct PciNet {
    raw: VirtIONetRaw<PciHal, CheckedTransport, NET_QUEUE_SIZE>,
    /// `rx[token]` holds the buffer posted under that descriptor token.
    rx: [Option<Box<[u8]>>; NET_QUEUE_SIZE],
    claims: BarClaims,
    bdf: u32,
    /// Set once the device stopped answering (TX timeout) or asked for a
    /// reset. A dead device is never touched again: `send` fails and
    /// `try_recv` reports nothing.
    dead: bool,
}

impl PciNet {
    /// Initialises the device behind `claims` (function `bdf`) and posts
    /// `NET_QUEUE_SIZE` RX buffers.
    pub fn new(transport: CheckedTransport, claims: BarClaims, bdf: u32) -> Result<Self, Error> {
        let mut net = Self {
            raw: VirtIONetRaw::new(transport)?,
            rx: [const { None }; NET_QUEUE_SIZE],
            claims,
            bdf,
            dead: false,
        };
        for _ in 0..NET_QUEUE_SIZE {
            net.post(vec![0u8; RX_BUF_LEN].into_boxed_slice())?;
        }
        Ok(net)
    }

    /// Marks the device dead and logs why, once.
    fn die(&mut self, reason: &str) {
        if !self.dead {
            self.dead = true;
            let line = format!("[virtio-net] {} marked dead: {reason}", Bdf(self.bdf));
            ostd::io::println(&line);
        }
    }

    /// Whether the device has set `DEVICE_NEEDS_RESET`.
    fn needs_reset(&self) -> bool {
        let bit = DeviceStatus::DEVICE_NEEDS_RESET.bits() as u8;
        self.claims.device_status().is_some_and(|s| s & bit != 0)
    }

    /// Hands `buf` to the device's receive queue and records it by token.
    fn post(&mut self, mut buf: Box<[u8]>) -> Result<(), Error> {
        // SAFETY: the buffer is stored in `self.rx` until `receive_complete`
        // returns it, and is not touched in between.
        let token = unsafe { self.raw.receive_begin(&mut buf)? };
        let slot = self
            .rx
            .get_mut(usize::from(token))
            .ok_or(Error::WrongToken)?;
        if slot.is_some() {
            return Err(Error::WrongToken);
        }
        *slot = Some(buf);
        Ok(())
    }

    pub fn mac_address(&self) -> [u8; 6] {
        self.raw.mac_address()
    }

    /// Transmits `frame` and waits (polled, bounded by `TX_BUDGET_MS`) until
    /// the device consumed it. Returns `true` on success.
    ///
    /// On expiry, or when the device sets `DEVICE_NEEDS_RESET`, the device is
    /// marked dead and the TX buffer is leaked: the queue still references it,
    /// so it must outlive any late device access.
    pub fn send(&mut self, frame: &[u8]) -> bool {
        if self.dead {
            return false;
        }
        let mut tx = vec![0u8; NET_HDR_LEN + frame.len()];
        tx[NET_HDR_LEN..].copy_from_slice(frame);
        // SAFETY: `tx` lives and stays untouched until `transmit_complete`,
        // or forever once the wait is abandoned.
        let Ok(token) = (unsafe { self.raw.transmit_begin(&tx) }) else {
            return false;
        };
        let mut deadline = Deadline::start(TX_BUDGET_MS, sys_get_time_ms());
        while self.raw.poll_transmit() != Some(token) {
            if !deadline.tick() {
                core::hint::spin_loop();
                continue;
            }
            ostd::task::yield_now();
            let reason = if self.needs_reset() {
                "device needs reset"
            } else if deadline.expired(sys_get_time_ms()) {
                "tx completion timeout"
            } else {
                continue;
            };
            self.die(reason);
            core::mem::forget(tx);
            return false;
        }
        // SAFETY: same buffer and token as `transmit_begin`.
        unsafe { self.raw.transmit_complete(token, &tx) }.is_ok()
    }

    /// Copies one received frame into `out` and re-posts its buffer. Returns
    /// the byte count, 0 when nothing is ready or the device is dead. Never
    /// blocks.
    pub fn try_recv(&mut self, out: &mut [u8]) -> usize {
        if self.dead {
            return 0;
        }
        let Some(token) = self.raw.poll_receive() else {
            return 0;
        };
        let Some(mut buf) = self.rx.get_mut(usize::from(token)).and_then(Option::take) else {
            return 0;
        };
        // SAFETY: `buf` is the buffer posted under `token` (see `post`).
        let used = match unsafe { self.raw.receive_complete(token, &mut buf) } {
            Ok((header, packet)) => header + packet,
            // The used length was shorter than the library's header: the
            // descriptor is already reclaimed, so the buffer is re-posted.
            Err(Error::IoError) => 0,
            Err(_) => {
                // Not reclaimed: the device still owns the buffer.
                self.rx[usize::from(token)] = Some(buf);
                return 0;
            }
        };
        let len = used.saturating_sub(NET_HDR_LEN).min(out.len());
        out[..len].copy_from_slice(&buf[NET_HDR_LEN..NET_HDR_LEN + len]);
        // Non-fatal: a failed re-post shrinks the ring by one buffer; the
        // frame is already in `out`.
        let _ = self.post(buf);
        len
    }
}
