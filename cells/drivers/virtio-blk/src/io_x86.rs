//! Polled, deadline-bounded request issue for the x86_64 PCI path.
//!
//! Interrupts are not routed (phase 04b deferred), and the blocking
//! virtio-drivers requests spin on the used ring without bound: a DMA
//! remapping fault or a device that raises `DEVICE_NEEDS_RESET` would hang
//! the cell while it holds the state lock, and with it every VFS block
//! request. Reads and writes are therefore submitted through the
//! non-blocking API and awaited here under [`IO_BUDGET_MS`], checking the
//! device status at every yield point.
//!
//! # Poisoning
//! A request that misses its deadline or meets `DEVICE_NEEDS_RESET` stays
//! published: the device may still complete it later. The device is then
//! poisoned — logged once, never touched again — and every later request is
//! refused as `NotIssued`. The device only ever addresses the authorised
//! bounce slots (`hal_x86`), and a stalled request never reaches `unshare`,
//! so its slots stay claimed and a late completion cannot write cell memory.
//!
//! # Unsafe island
//! The non-blocking virtio-drivers calls are `unsafe fn`s.

#![allow(unsafe_code)]

use crate::device::{outcome, BlkDevice};
use crate::pci_x86::Bdf;
use crate::wait::{Deadline, IO_BUDGET_MS};
use ostd::io::print_fmt;
use ostd::syscall::{sys_get_time_ms, BlockDevice};
use types::block_wire::BlockOutcome;
use virtio_drivers::device::blk::{BlkReq, BlkResp};

/// Poison latch: set by the first stall and never cleared. While set, every
/// request is refused before it reaches the queue, so it claims no bounce slot
/// (the stalled request keeps its slots, and repeated stalls would otherwise
/// exhaust them).
#[derive(Default)]
pub(crate) struct Poison(bool);

impl Poison {
    /// Admits a request, or refuses it as `NotIssued` once poisoned.
    const fn admit(&self) -> Result<(), BlockOutcome> {
        if self.0 {
            Err(BlockOutcome::NotIssued)
        } else {
            Ok(())
        }
    }

    /// Sets the latch; `true` only on the first call.
    const fn set(&mut self) -> bool {
        let first = !self.0;
        self.0 = true;
        first
    }
}

// The cell has no host-test harness, so the latch is checked at build time:
// a stall poisons once, and every later request is refused.
const _: () = {
    let mut poison = Poison(false);
    assert!(poison.admit().is_ok());
    assert!(poison.set());
    assert!(matches!(poison.admit(), Err(BlockOutcome::NotIssued)));
    assert!(!poison.set());
    assert!(matches!(poison.admit(), Err(BlockOutcome::NotIssued)));
};

impl BlkDevice {
    /// Marks the device dead; the poison line is printed once.
    fn poison(&mut self, reason: &str) {
        if !self.poisoned.set() {
            return;
        }
        // The x86_64 path only ever publishes `Pci` routes.
        let bdf = match self.route {
            BlockDevice::Pci { bdf } => u32::from(bdf),
            BlockDevice::Mmio { .. } => 0,
        };
        let _ = print_fmt(format_args!(
            "[virtio-blk] poisoned {}: {reason}; refusing further requests\n",
            Bdf(bdf)
        ));
    }

    /// Waits for the single in-flight request's used entry. On expiry or
    /// `DEVICE_NEEDS_RESET` the device is poisoned and the request, already
    /// notified, is reported `Indeterminate`.
    fn wait_used(&mut self) -> Result<(), BlockOutcome> {
        let mut deadline = Deadline::start(IO_BUDGET_MS, sys_get_time_ms());
        while self.blk.peek_used().is_none() {
            if !deadline.tick() {
                core::hint::spin_loop();
                continue;
            }
            let stall = if self.bars.needs_reset() {
                Some("device reports DEVICE_NEEDS_RESET")
            } else {
                ostd::task::yield_now();
                deadline
                    .expired(sys_get_time_ms())
                    .then_some("completion deadline expired")
            };
            if let Some(reason) = stall {
                self.poison(reason);
                return Err(BlockOutcome::Indeterminate);
            }
        }
        Ok(())
    }

    /// Read `buf.len() / 512` consecutive sectors into `buf` in one request;
    /// `buf` is a whole number of sectors, at most one page (the bounce size).
    pub fn read_sectors(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockOutcome> {
        self.poisoned.admit()?;
        let mut req = BlkReq::default();
        let mut resp = BlkResp::default();
        // SAFETY: `req`, `buf` and `resp` are not accessed again until
        // `complete_read_blocks` below. On a poisoned wait they are abandoned,
        // which is sound because the device holds only their bounce copies.
        // Submission errors arise before the notify, so nothing was issued.
        let token = unsafe {
            self.blk
                .read_blocks_nb(sector as usize, &mut req, buf, &mut resp)
        }
        .map_err(|_| BlockOutcome::NotIssued)?;
        self.wait_used()?;
        // SAFETY: the same buffers as passed to `read_blocks_nb` for `token`.
        outcome(unsafe { self.blk.complete_read_blocks(token, &req, buf, &mut resp) })
    }

    /// Write one 512-byte sector.
    pub fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockOutcome> {
        self.poisoned.admit()?;
        let mut req = BlkReq::default();
        let mut resp = BlkResp::default();
        // SAFETY: as in `read_sectors`.
        let token = unsafe {
            self.blk
                .write_blocks_nb(sector as usize, &mut req, buf, &mut resp)
        }
        .map_err(|_| BlockOutcome::NotIssued)?;
        self.wait_used()?;
        // SAFETY: the same buffers as passed to `write_blocks_nb` for `token`.
        outcome(unsafe { self.blk.complete_write_blocks(token, &req, buf, &mut resp) })
    }

    /// Send a VirtIO FLUSH request.
    pub fn flush(&mut self) -> Result<(), BlockOutcome> {
        self.poisoned.admit()?;
        if self.bars.needs_reset() {
            self.poison("device reports DEVICE_NEEDS_RESET");
            return Err(BlockOutcome::NotIssued);
        }
        // Contract: virtio-drivers 0.7.5 has no non-blocking flush, so this
        // wait is unbounded. The gates above refuse a device already poisoned
        // or reporting DEVICE_NEEDS_RESET; a device that dies while this flush
        // is in flight still hangs the cell (and VFS block I/O with it).
        outcome(self.blk.flush())
    }
}
