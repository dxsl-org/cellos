//! Polled, deadline-bounded request issue for every transport.
//!
//! Interrupts are not routed to this cell, and the blocking virtio-drivers
//! requests spin on the used ring without bound: a stalled device, a DMA
//! fault or a device that raises `DEVICE_NEEDS_RESET` would hang the cell
//! while it holds the state lock, and with it every VFS block request. Reads,
//! writes and flushes are therefore submitted through the non-blocking API
//! and awaited here under [`IO_BUDGET_MS`], checking the device status before
//! submission and at every yield point.
//!
//! # Poisoning
//! A request that misses its deadline or meets `DEVICE_NEEDS_RESET` stays
//! published: the device may still complete it later. The device is then
//! poisoned — logged once, never touched again — and every later request is
//! refused as `NotIssued`. The device only ever addresses bounce memory
//! (`hal_x86` slots, `mmio` grant pages), and a stalled request never reaches
//! `unshare`, so that memory stays claimed and a late completion cannot write
//! cell memory. No reset or recovery is attempted: a reset would race the
//! abandoned request.
//!
//! # Unsafe island
//! The non-blocking virtio-drivers calls are `unsafe fn`s.

#![allow(unsafe_code)]

use crate::device::{outcome, BlkDevice};
use crate::wait::{Deadline, IO_BUDGET_MS};
use core::fmt;
use ostd::io::print_fmt;
use ostd::syscall::{sys_get_time_ms, BlockDevice};
use types::block_wire::BlockOutcome;
use virtio_drivers::device::blk::{BlkReq, BlkResp};

/// Poison latch: set by the first stall and never cleared. While set, every
/// request is refused before it reaches the queue, so it claims no bounce
/// memory (the stalled request keeps its own, and repeated stalls would
/// otherwise exhaust it).
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

/// The device a route names, as the poison line prints it.
struct RouteName(BlockDevice);

impl fmt::Display for RouteName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            #[cfg(target_arch = "x86_64")]
            BlockDevice::Pci { bdf } => write!(f, "pci {}", crate::pci_x86::Bdf(bdf.into())),
            #[cfg(not(target_arch = "x86_64"))]
            BlockDevice::Pci { bdf } => write!(f, "bdf={bdf:#06x}"),
            BlockDevice::Mmio { base } => write!(f, "mmio base={base:#x}"),
        }
    }
}

const NEEDS_RESET: &str = "device reports DEVICE_NEEDS_RESET";

impl BlkDevice {
    /// Whether the device reports `DEVICE_NEEDS_RESET`. On x86_64 the status
    /// is read back through the claimed BAR, where an unreadable register
    /// also counts (fail closed).
    fn needs_reset(&self) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            self.bars.needs_reset()
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            self.blk.needs_reset()
        }
    }

    /// Marks the device dead; the poison line is printed once.
    fn poison(&mut self, reason: &str) {
        if !self.poisoned.set() {
            return;
        }
        let _ = print_fmt(format_args!(
            "[virtio-blk] poisoned {}: {reason}; refusing further requests\n",
            RouteName(self.route)
        ));
    }

    /// Admits a request before anything is published: refused as `NotIssued`
    /// once poisoned, and a device already reporting `DEVICE_NEEDS_RESET` is
    /// poisoned rather than handed a request it will not process.
    fn admit(&mut self) -> Result<(), BlockOutcome> {
        self.poisoned.admit()?;
        if self.needs_reset() {
            self.poison(NEEDS_RESET);
            return Err(BlockOutcome::NotIssued);
        }
        Ok(())
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
            let stall = if self.needs_reset() {
                Some(NEEDS_RESET)
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
        self.admit()?;
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
        self.admit()?;
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

    /// Send a VirtIO FLUSH request. A device that did not negotiate FLUSH
    /// has no volatile cache to flush, so the request completes at once.
    pub fn flush(&mut self) -> Result<(), BlockOutcome> {
        self.admit()?;
        let mut req = BlkReq::default();
        let mut resp = BlkResp::default();
        // SAFETY: as in `read_sectors`, for `req` and `resp`.
        let token = unsafe { self.blk.flush_nb(&mut req, &mut resp) }
            .map_err(|_| BlockOutcome::NotIssued)?;
        let Some(token) = token else {
            return Ok(());
        };
        self.wait_used()?;
        // SAFETY: the same buffers as passed to `flush_nb` for `token`.
        outcome(unsafe { self.blk.complete_flush(token, &req, &mut resp) })
    }
}
